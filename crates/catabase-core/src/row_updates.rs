use std::{
    collections::{BTreeSet, HashMap},
    sync::atomic::{AtomicBool, Ordering},
};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{CoreError, PgClient, RowKey, row_keys::load_row_keys};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowUpdate {
    pub schema: String,
    pub table: String,
    #[serde(default)]
    pub key_columns: Vec<String>,
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
    generated: bool,
    json_type: Option<&'static str>,
    array: bool,
}

#[derive(Debug)]
struct PreparedUpdate {
    sql: String,
    json_parameters: Vec<Option<String>>,
    original: Map<String, Value>,
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
                    "SELECT a.attname, a.attgenerated <> '',
                 a.atttypid = 'pg_catalog.json'::regtype, a.atttypid = 'pg_catalog.jsonb'::regtype, c.oid,
                 t.typelem <> 0 AND t.typlen = -1
                 FROM pg_class c
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 JOIN pg_attribute a ON a.attrelid = c.oid
                 JOIN pg_type t ON t.oid = a.atttypid
                 WHERE n.nspname = $1 AND c.relname = $2 AND c.relkind IN ('r', 'p')
                   AND a.attnum > 0 AND NOT a.attisdropped",
                    &[&update.schema, &update.table],
                )
                .await?;
            let table_oids = rows
                .first()
                .map(|row| row.get::<_, u32>(4))
                .into_iter()
                .collect::<Vec<_>>();
            let keys = load_row_keys(client, &table_oids).await?;
            let columns = rows
                .into_iter()
                .map(|row| {
                    (
                        row.get::<_, String>(0),
                        EditableColumn {
                            generated: row.get(1),
                            array: row.get(5),
                            json_type: if row.get(2) {
                                Some("json")
                            } else if row.get(3) {
                                Some("jsonb")
                            } else {
                                None
                            },
                        },
                    )
                })
                .collect::<HashMap<_, _>>();
            tables.insert(table_key.clone(), (columns, keys));
        }
        let (columns, keys) = &tables[&table_key];
        let prepared = build_update_sql(update, columns, keys)?;
        check_cancelled(cancelled)?;
        let values = Value::Object(update.values.clone());
        let original = Value::Object(prepared.original.clone());
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
    keys: &[RowKey],
) -> Result<PreparedUpdate, CoreError> {
    if columns.is_empty() {
        return Err(CoreError::message(
            "This source table is unavailable or is not an editable table.",
        ));
    }
    if update.values.is_empty() {
        return Err(CoreError::message("No changed values were provided."));
    }
    let key = keys.iter().find(|key| {
        key.schema == update.schema && key.table == update.table
            && !key.columns.is_empty()
            && key.columns.len() == update.key_columns.len()
            && key.columns.iter().all(|name| update.key_columns.contains(name))
    }).ok_or_else(|| CoreError::message(
        "Editing requires a complete primary key or usable unique key in the result. Include every key column and run the query again."
    ))?;
    if key.columns.iter().any(|name| {
        update
            .original
            .get(name)
            .is_none_or(|value| value.is_null() && (key.primary_key || !key.nulls_not_distinct))
    }) {
        return Err(CoreError::message(
            "The selected row key is missing a value or contains NULL without NULLS NOT DISTINCT uniqueness. Refresh the result before editing.",
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
    let predicate_names = key
        .columns
        .iter()
        .chain(update.values.keys())
        .collect::<BTreeSet<_>>();
    let predicates = predicate_names.iter().map(|name| {
        let quoted = quote_ident(name);
        let is_key = key.columns.contains(name);
        if is_key && update.original[*name].is_null() {
            return format!("target.{quoted} IS NULL");
        }
        if let Some(json_type) = columns[*name].json_type {
            json_parameters.push(json_parameter(&update.original[*name]));
            if is_key {
                return format!("target.{quoted} = (${}::text)::{json_type}", json_parameters.len() + 2);
            }
            return format!("to_jsonb(target.{quoted}) IS NOT DISTINCT FROM to_jsonb((${}::text)::{json_type})", json_parameters.len() + 2);
        }
        // Key comparisons retain their indexable native types. Only edited
        // non-key cells need an optimistic stale-value check.
        if is_key {
            format!("target.{quoted} = original_values.{quoted}")
        } else if columns[*name].array {
            // JSON conversion omits array lower bounds. Check dimensions too
            // so a concurrent change of bounds cannot be overwritten silently.
            format!("(array_dims(target.{quoted}) IS NOT DISTINCT FROM array_dims(original_values.{quoted}) AND to_jsonb(target.{quoted}) IS NOT DISTINCT FROM to_jsonb(original_values.{quoted}))")
        } else {
            format!("to_jsonb(target.{quoted}) IS NOT DISTINCT FROM to_jsonb(original_values.{quoted})")
        }
    }).collect::<Vec<_>>().join(" AND ");
    Ok(PreparedUpdate {
        sql: format!(
            "UPDATE {table} AS target SET {assignments} FROM jsonb_populate_record(NULL::{table}, $1::jsonb) AS new_values, jsonb_populate_record(NULL::{table}, $2::jsonb) AS original_values WHERE {predicates}"
        ),
        json_parameters,
        original: predicate_names
            .into_iter()
            .map(|name| (name.clone(), update.original[name].clone()))
            .collect(),
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
            ("id", false),
            ("tenant_id", false),
            ("note", false),
            ("calculated", true),
        ]
        .into_iter()
        .map(|(name, generated)| {
            (
                name.to_string(),
                EditableColumn {
                    generated,
                    json_type: None,
                    array: false,
                },
            )
        })
        .collect()
    }

    fn keys() -> Vec<RowKey> {
        vec![RowKey {
            schema: "odd\"schema".into(),
            table: "orders".into(),
            columns: vec!["id".into(), "tenant_id".into()],
            primary_key: true,
            nulls_not_distinct: false,
        }]
    }

    fn update() -> RowUpdate {
        RowUpdate {
            schema: "odd\"schema".into(),
            table: "orders".into(),
            key_columns: vec!["id".into(), "tenant_id".into()],
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
        let sql = build_update_sql(&update(), &columns(), &keys())
            .unwrap()
            .sql;
        assert!(sql.starts_with("UPDATE \"odd\"\"schema\".\"orders\""));
        assert!(!sql.contains("DROP TABLE"));
        assert!(!sql.contains("9223372036854775807"));
        assert!(sql.contains("$1::jsonb"));
        assert!(sql.contains("$2::jsonb"));
        assert!(sql.contains("target.\"id\" = original_values.\"id\""));
        assert!(sql.contains(
            "to_jsonb(target.\"note\") IS NOT DISTINCT FROM to_jsonb(original_values.\"note\")"
        ));
    }

    #[test]
    fn rejects_partial_primary_keys_generated_and_unknown_columns() {
        let mut row = update();
        row.key_columns.pop();
        assert!(
            build_update_sql(&row, &columns(), &keys())
                .unwrap_err()
                .to_string()
                .contains("complete primary key")
        );
        row = update();
        row.values.insert("calculated".into(), json!(10));
        assert!(
            build_update_sql(&row, &columns(), &keys())
                .unwrap_err()
                .to_string()
                .contains("Generated")
        );
        row = update();
        row.values.insert("unknown".into(), json!(10));
        assert!(
            build_update_sql(&row, &columns(), &keys())
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
                generated: false,
                json_type: Some("jsonb"),
                array: false,
            },
        );
        let mut row = update();
        let raw = r#"{"id":9223372036854775807,"amount":0.123456789012345678901}"#;
        row.values
            .insert("payload".into(), Value::String(raw.into()));
        row.original
            .insert("payload".into(), Value::String("null".into()));
        let prepared = build_update_sql(&row, &metadata, &keys()).unwrap();
        assert!(prepared.sql.contains("\"payload\" = ($3::text)::jsonb"));
        assert!(prepared.sql.contains("to_jsonb(($4::text)::jsonb)"));
        assert_eq!(
            prepared.json_parameters,
            vec![Some(raw.into()), Some("null".into())]
        );
        row.values.insert("payload".into(), Value::Null);
        assert_eq!(
            build_update_sql(&row, &metadata, &keys())
                .unwrap()
                .json_parameters[0],
            None
        );
    }

    #[test]
    fn ignores_unmodified_columns_but_guards_every_changed_cell() {
        let mut row = update();
        // This value must not be fed into jsonb_populate_record or the WHERE.
        row.original
            .insert("calculated".into(), json!("<unsupported>"));
        let prepared = build_update_sql(&row, &columns(), &keys()).unwrap();
        assert!(!prepared.sql.contains("calculated"));
        assert!(!prepared.original.contains_key("calculated"));
        assert_eq!(prepared.original.len(), 3);
        row.values.insert("id".into(), json!("42"));
        let prepared = build_update_sql(&row, &columns(), &keys()).unwrap();
        assert_eq!(prepared.original["id"], "9223372036854775807");
        assert_eq!(prepared.sql.matches("target.\"id\"").count(), 1);
        row.original.remove("note");
        assert!(
            build_update_sql(&row, &columns(), &keys())
                .unwrap_err()
                .to_string()
                .contains("Missing original")
        );
    }

    #[test]
    fn unique_keys_are_authoritative_and_nulls_require_non_distinct_uniqueness() {
        let mut row = update();
        row.key_columns = vec!["note".into()];
        let mut key = keys().remove(0);
        key.columns = row.key_columns.clone();
        key.primary_key = false;
        assert!(build_update_sql(&row, &columns(), &keys()).is_err());
        assert!(build_update_sql(&row, &columns(), &[key.clone()]).is_err());
        key.nulls_not_distinct = true;
        let prepared = build_update_sql(&row, &columns(), &[key.clone()]).unwrap();
        assert!(prepared.sql.contains("target.\"note\" IS NULL"));
        assert!(!prepared.sql.contains("target.\"id\""));
        row.original.insert("note".into(), json!("unique value"));
        key.nulls_not_distinct = false;
        assert!(
            build_update_sql(&row, &columns(), &[key.clone()])
                .unwrap()
                .sql
                .contains("target.\"note\" = original_values.\"note\"")
        );
        key.table = "other".into();
        assert!(build_update_sql(&row, &columns(), &[key]).is_err());
        row = update();
        row.original.remove("tenant_id");
        assert!(build_update_sql(&row, &columns(), &keys()).is_err());
    }
}
