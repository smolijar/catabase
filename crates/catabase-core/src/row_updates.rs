use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{CoreError, PgClient};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowUpdate {
    pub schema: String,
    pub table: String,
    pub values: Map<String, Value>,
    pub original: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyUpdatesResult {
    pub updated_rows: usize,
    pub in_transaction: bool,
}

#[derive(Clone, Debug)]
struct EditableColumn {
    primary_key: bool,
    generated: bool,
    json_type: Option<&'static str>,
}

#[derive(Debug)]
struct PreparedUpdate {
    sql: String,
    json_parameters: Vec<Option<String>>,
}

/// Apply a batch atomically. An existing user transaction is preserved with a
/// savepoint; otherwise this function owns and commits the transaction.
pub async fn apply_row_updates(
    client: &PgClient,
    updates: &[RowUpdate],
    in_transaction: bool,
    cancelled: &AtomicBool,
) -> Result<ApplyUpdatesResult, CoreError> {
    if updates.is_empty() || updates.len() > 1000 {
        return Err(CoreError::message(
            "Apply between 1 and 1,000 row changes at a time.",
        ));
    }
    check_cancelled(cancelled)?;
    let savepoint = format!("catabase_edit_{}", uuid::Uuid::new_v4().simple());
    let begin = if in_transaction {
        format!("SAVEPOINT {savepoint}")
    } else {
        "BEGIN".to_string()
    };
    client.batch_execute(&begin).await?;

    let outcome = apply_batch(client, updates, cancelled).await;
    match outcome {
        Ok(()) => {
            if let Err(error) = check_cancelled(cancelled) {
                rollback(client, in_transaction, &savepoint).await?;
                return Err(error);
            }
            let finish = if in_transaction {
                format!("RELEASE SAVEPOINT {savepoint}")
            } else {
                "COMMIT".to_string()
            };
            if let Err(error) = client.batch_execute(&finish).await {
                rollback(client, in_transaction, &savepoint).await?;
                return Err(error.into());
            }
            Ok(ApplyUpdatesResult {
                updated_rows: updates.len(),
                in_transaction,
            })
        }
        Err(error) => {
            rollback(client, in_transaction, &savepoint).await?;
            Err(error)
        }
    }
}

async fn rollback(
    client: &PgClient,
    in_transaction: bool,
    savepoint: &str,
) -> Result<(), CoreError> {
    let sql = if in_transaction {
        format!("ROLLBACK TO SAVEPOINT {savepoint}; RELEASE SAVEPOINT {savepoint}")
    } else {
        "ROLLBACK".to_string()
    };
    client.batch_execute(&sql).await?;
    Ok(())
}

fn check_cancelled(cancelled: &AtomicBool) -> Result<(), CoreError> {
    if cancelled.load(Ordering::Acquire) {
        return Err(CoreError::Cancelled(
            "Row changes cancelled; no changes were applied.".into(),
        ));
    }
    Ok(())
}

async fn apply_batch(
    client: &PgClient,
    updates: &[RowUpdate],
    cancelled: &AtomicBool,
) -> Result<(), CoreError> {
    let mut tables = HashMap::new();
    for (index, update) in updates.iter().enumerate() {
        check_cancelled(cancelled)?;
        let table_key = (update.schema.clone(), update.table.clone());
        if !tables.contains_key(&table_key) {
            let rows = client
                .query(
                    "SELECT a.attname, a.attgenerated <> '', EXISTS (
                    SELECT 1 FROM pg_index i WHERE i.indrelid = c.oid
                    AND i.indisprimary AND a.attnum = ANY(i.indkey)
                 ), a.atttypid = 'pg_catalog.json'::regtype, a.atttypid = 'pg_catalog.jsonb'::regtype
                 FROM pg_class c
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 JOIN pg_attribute a ON a.attrelid = c.oid
                 WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p')
                   AND a.attnum > 0 AND NOT a.attisdropped",
                    &[&update.schema, &update.table],
                )
                .await?;
            let columns = rows
                .into_iter()
                .map(|row| {
                    (
                        row.get::<_, String>(0),
                        EditableColumn {
                            generated: row.get(1),
                            primary_key: row.get(2),
                            json_type: if row.get(3) {
                                Some("json")
                            } else if row.get(4) {
                                Some("jsonb")
                            } else {
                                None
                            },
                        },
                    )
                })
                .collect::<HashMap<_, _>>();
            tables.insert(table_key.clone(), columns);
        }
        let columns = &tables[&table_key];
        let prepared = build_update_sql(update, columns)?;
        check_cancelled(cancelled)?;
        let values = Value::Object(update.values.clone());
        let original = Value::Object(update.original.clone());
        let mut parameters: Vec<&(dyn tokio_postgres::types::ToSql + Sync)> =
            vec![&values, &original];
        parameters.extend(
            prepared
                .json_parameters
                .iter()
                .map(|value| value as &(dyn tokio_postgres::types::ToSql + Sync)),
        );
        let affected = client.execute(&prepared.sql, &parameters).await?;
        ensure_one_row(affected, index)?;
    }
    Ok(())
}

fn ensure_one_row(affected: u64, index: usize) -> Result<(), CoreError> {
    if affected != 1 {
        return Err(CoreError::message(format!(
            "Row {} matched {affected} rows instead of one. The data may have changed; refresh the result and retry. No changes were applied.",
            index + 1
        )));
    }
    Ok(())
}

fn build_update_sql(
    update: &RowUpdate,
    columns: &HashMap<String, EditableColumn>,
) -> Result<PreparedUpdate, CoreError> {
    if columns.is_empty() {
        return Err(CoreError::message(
            "This source table is unavailable or is not an editable table.",
        ));
    }
    if update.values.is_empty() {
        return Err(CoreError::message("No changed values were provided."));
    }
    let primary_keys = columns
        .iter()
        .filter(|(_, column)| column.primary_key)
        .collect::<Vec<_>>();
    if primary_keys.is_empty()
        || primary_keys
            .iter()
            .any(|(name, _)| update.original.get(*name).is_none_or(Value::is_null))
    {
        return Err(CoreError::message(
            "Editing requires every primary-key column in the result. Include the complete primary key and run the query again.",
        ));
    }
    for name in update.original.keys().chain(update.values.keys()) {
        if !columns.contains_key(name) {
            return Err(CoreError::message(format!("Unknown source column: {name}")));
        }
    }
    for name in update.values.keys() {
        if columns[name].generated {
            return Err(CoreError::message(format!(
                "Generated column {name} cannot be edited."
            )));
        }
        if !update.original.contains_key(name) {
            return Err(CoreError::message(format!(
                "Missing original value for {name}; refresh the result before editing."
            )));
        }
    }
    let table = format!(
        "{}.{}",
        quote_ident(&update.schema),
        quote_ident(&update.table)
    );
    let mut json_parameters = Vec::new();
    let assignments = update
        .values
        .keys()
        .map(|name| {
            let quoted = quote_ident(name);
            if let Some(json_type) = columns[name].json_type {
                json_parameters.push(json_parameter(&update.values[name]));
                format!(
                    "{quoted} = (${}::text)::{json_type}",
                    json_parameters.len() + 2
                )
            } else {
                format!("{quoted} = new_values.{quoted}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let predicates = update.original.keys().map(|name| {
        let quoted = quote_ident(name);
        if let Some(json_type) = columns[name].json_type {
            json_parameters.push(json_parameter(&update.original[name]));
            return format!("to_jsonb(target.{quoted}) IS NOT DISTINCT FROM to_jsonb((${}::text)::{json_type})", json_parameters.len() + 2);
        }
        // Primary-key comparisons retain their indexable native types. JSONB
        // comparison also supports json columns, which lack an equality operator.
        if columns[name].primary_key {
            format!("target.{quoted} IS NOT DISTINCT FROM original_values.{quoted}")
        } else {
            format!("to_jsonb(target.{quoted}) IS NOT DISTINCT FROM to_jsonb(original_values.{quoted})")
        }
    }).collect::<Vec<_>>().join(" AND ");
    Ok(PreparedUpdate {
        sql: format!(
            "UPDATE {table} AS target SET {assignments} FROM jsonb_populate_record(NULL::{table}, $1::jsonb) AS new_values, jsonb_populate_record(NULL::{table}, $2::jsonb) AS original_values WHERE {predicates}"
        ),
        json_parameters,
    })
}

fn json_parameter(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(raw) => Some(raw.clone()),
        value => Some(value.to_string()),
    }
}

fn quote_ident(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn columns() -> HashMap<String, EditableColumn> {
        [
            ("id", true, false),
            ("tenant_id", true, false),
            ("note", false, false),
            ("calculated", false, true),
        ]
        .into_iter()
        .map(|(name, primary_key, generated)| {
            (
                name.to_string(),
                EditableColumn {
                    primary_key,
                    generated,
                    json_type: None,
                },
            )
        })
        .collect()
    }

    fn update() -> RowUpdate {
        RowUpdate {
            schema: "odd\"schema".into(),
            table: "orders".into(),
            values: json!({"note": "'; DROP TABLE orders; --"})
                .as_object()
                .unwrap()
                .clone(),
            original: json!({"id": "9223372036854775807", "tenant_id": 7, "note": null})
                .as_object()
                .unwrap()
                .clone(),
        }
    }

    #[test]
    fn update_sql_keeps_values_parameterized_and_identifiers_quoted() {
        let sql = build_update_sql(&update(), &columns()).unwrap().sql;
        assert!(sql.starts_with("UPDATE \"odd\"\"schema\".\"orders\""));
        assert!(!sql.contains("DROP TABLE"));
        assert!(!sql.contains("9223372036854775807"));
        assert!(sql.contains("$1::jsonb"));
        assert!(sql.contains("$2::jsonb"));
        assert!(sql.contains("target.\"id\" IS NOT DISTINCT FROM original_values.\"id\""));
        assert!(sql.contains(
            "to_jsonb(target.\"note\") IS NOT DISTINCT FROM to_jsonb(original_values.\"note\")"
        ));
    }

    #[test]
    fn rejects_partial_primary_keys_generated_and_unknown_columns() {
        let mut row = update();
        row.original.remove("tenant_id");
        assert!(
            build_update_sql(&row, &columns())
                .unwrap_err()
                .to_string()
                .contains("primary-key")
        );
        row = update();
        row.values.insert("calculated".into(), json!(10));
        assert!(
            build_update_sql(&row, &columns())
                .unwrap_err()
                .to_string()
                .contains("Generated")
        );
        row = update();
        row.values.insert("unknown".into(), json!(10));
        assert!(
            build_update_sql(&row, &columns())
                .unwrap_err()
                .to_string()
                .contains("Unknown")
        );
    }

    #[test]
    fn requires_exactly_one_affected_row_and_honors_cancellation() {
        assert!(ensure_one_row(1, 0).is_ok());
        assert!(ensure_one_row(0, 0).is_err());
        assert!(ensure_one_row(2, 0).is_err());
        assert!(check_cancelled(&AtomicBool::new(true)).is_err());
    }

    #[test]
    fn json_parameters_preserve_raw_numbers_and_sql_null_distinction() {
        let mut metadata = columns();
        metadata.insert(
            "payload".into(),
            EditableColumn {
                primary_key: false,
                generated: false,
                json_type: Some("jsonb"),
            },
        );
        let mut row = update();
        let raw = r#"{"id":9223372036854775807,"amount":0.123456789012345678901}"#;
        row.values
            .insert("payload".into(), Value::String(raw.into()));
        row.original
            .insert("payload".into(), Value::String("null".into()));
        let prepared = build_update_sql(&row, &metadata).unwrap();
        assert!(prepared.sql.contains("\"payload\" = ($3::text)::jsonb"));
        assert!(prepared.sql.contains("to_jsonb(($4::text)::jsonb)"));
        assert_eq!(
            prepared.json_parameters,
            vec![Some(raw.into()), Some("null".into())]
        );
        row.values.insert("payload".into(), Value::Null);
        assert_eq!(
            build_update_sql(&row, &metadata).unwrap().json_parameters[0],
            None
        );
    }
}
