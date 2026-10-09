use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{CoreError, PgClient, SchemaForeignKey};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyLookup {
    pub schema: String,
    pub table: String,
    pub constraint: String,
    pub search: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForeignKeyValues {
    pub foreign_key: SchemaForeignKey,
    pub preview_columns: Vec<String>,
    // All keys are text, including int8/numeric, to preserve exact values.
    pub rows: Vec<BTreeMap<String, Option<String>>>,
    pub has_more: bool,
}

const PAGE_SIZE: usize = 50;
const CONSTRAINT_SQL: &str = "SELECT fk.confrelid, rn.nspname, rc.relname,
        array_agg(a.attname::text ORDER BY k.ordinality),
        array_agg(ra.attname::text ORDER BY k.ordinality)
    FROM pg_catalog.pg_constraint fk
    JOIN pg_catalog.pg_class c ON c.oid = fk.conrelid
    JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace
    JOIN pg_catalog.pg_class rc ON rc.oid = fk.confrelid
    JOIN pg_catalog.pg_namespace rn ON rn.oid = rc.relnamespace
    JOIN LATERAL unnest(fk.conkey, fk.confkey)
        WITH ORDINALITY AS k(attnum, ref_attnum, ordinality) ON true
    JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum
    JOIN pg_catalog.pg_attribute ra ON ra.attrelid = rc.oid AND ra.attnum = k.ref_attnum
    WHERE fk.contype = 'f' AND n.nspname = $1 AND c.relname = $2 AND fk.conname = $3
    GROUP BY fk.oid, fk.confrelid, rn.nspname, rc.relname";

/// Called inside the caller's isolated READ ONLY transaction. Catalog data,
/// not client-supplied target names or SQL, determines what can be previewed.
pub async fn lookup_foreign_key_values(
    client: &PgClient,
    request: &ForeignKeyLookup,
) -> Result<ForeignKeyValues, CoreError> {
    if request.search.chars().count() > 512 {
        return Err(CoreError::message("Search is limited to 512 characters."));
    }
    let row = client
        .query_opt(
            CONSTRAINT_SQL,
            &[&request.schema, &request.table, &request.constraint],
        )
        .await?
        .ok_or_else(|| {
            CoreError::message(
                "This foreign key no longer exists. Refresh the schema and re-run the query.",
            )
        })?;
    let referenced_oid: u32 = row.get(0);
    let key = SchemaForeignKey {
        name: request.constraint.clone(),
        referenced_schema: row.get(1),
        referenced_table: row.get(2),
        columns: row.get(3),
        referenced_columns: row.get(4),
    };
    let previews = client
        .query(
            "SELECT a.attname::text FROM pg_catalog.pg_attribute a
         JOIN pg_catalog.pg_type t ON t.oid = a.atttypid
         WHERE a.attrelid = $1 AND a.attnum > 0 AND NOT a.attisdropped
           AND t.typcategory = 'S' AND NOT (a.attname::text = ANY($2::text[]))
           AND has_column_privilege($1, a.attnum, 'SELECT')
         ORDER BY CASE lower(a.attname)
             WHEN 'name' THEN 0 WHEN 'title' THEN 1 WHEN 'label' THEN 2
             WHEN 'description' THEN 3 WHEN 'code' THEN 4 WHEN 'email' THEN 5 ELSE 6 END,
             a.attnum LIMIT 3",
            &[&referenced_oid, &key.referenced_columns],
        )
        .await?;
    let preview_columns: Vec<String> = previews.iter().map(|row| row.get(0)).collect();
    let columns = key
        .referenced_columns
        .iter()
        .chain(&preview_columns)
        .cloned()
        .collect::<Vec<_>>();
    let sql = preview_sql(&key, &preview_columns)?;
    let mut rows = client.query(&sql, &[&request.search]).await?;
    let has_more = rows.len() > PAGE_SIZE;
    rows.truncate(PAGE_SIZE);
    let rows = rows
        .iter()
        .map(|row| {
            let values: Vec<Option<String>> = row.get(0);
            columns.iter().cloned().zip(values).collect()
        })
        .collect();
    Ok(ForeignKeyValues {
        foreign_key: key,
        preview_columns,
        rows,
        has_more,
    })
}

fn quote_identifier(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

fn preview_sql(key: &SchemaForeignKey, previews: &[String]) -> Result<String, CoreError> {
    if key.columns.is_empty() || key.columns.len() != key.referenced_columns.len() {
        return Err(CoreError::message(
            "The foreign key has no complete column mapping.",
        ));
    }
    let keys = key
        .referenced_columns
        .iter()
        .map(|name| quote_identifier(name))
        .collect::<Vec<_>>();
    let columns = keys
        .iter()
        .cloned()
        .chain(previews.iter().map(|name| quote_identifier(name)))
        .collect::<Vec<_>>();
    let projection = columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            if index < keys.len() {
                format!("{column}::text")
            } else {
                format!("left({column}::text, 240)")
            }
        })
        .collect::<Vec<_>>()
        .join(", ");
    let non_null = keys
        .iter()
        .map(|column| format!("{column} IS NOT NULL"))
        .collect::<Vec<_>>()
        .join(" AND ");
    let search = columns
        .iter()
        .map(|column| format!("strpos(lower({column}::text), lower($1::text)) > 0"))
        .collect::<Vec<_>>()
        .join(" OR ");
    Ok(format!(
        "SELECT ARRAY[{projection}] AS fk_values FROM {}.{}
        WHERE {non_null} AND ($1::text = '' OR {search}) ORDER BY {} LIMIT {}",
        quote_identifier(&key.referenced_schema),
        quote_identifier(&key.referenced_table),
        keys.join(", "),
        PAGE_SIZE + 1
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> SchemaForeignKey {
        SchemaForeignKey {
            name: "fk".into(),
            columns: vec!["parent_id".into(), "tenant".into()],
            referenced_schema: "Mixed.Schema".into(),
            referenced_table: "Parent\"s".into(),
            referenced_columns: vec!["ID".into(), "tenant_id".into()],
        }
    }

    #[test]
    fn previews_quote_identifiers_use_literal_parameterized_search_and_bound_rows() {
        let sql = preview_sql(&key(), &["display\"name".into()]).unwrap();
        assert!(sql.contains("FROM \"Mixed.Schema\".\"Parent\"\"s\""));
        assert!(sql.contains(
            "ARRAY[\"ID\"::text, \"tenant_id\"::text, left(\"display\"\"name\"::text, 240)]"
        ));
        assert!(sql.contains("\"ID\" IS NOT NULL AND \"tenant_id\" IS NOT NULL"));
        assert!(sql.contains("strpos(lower(\"display\"\"name\"::text), lower($1::text)) > 0"));
        assert!(sql.ends_with("ORDER BY \"ID\", \"tenant_id\" LIMIT 51"));
        assert!(!sql.contains("ILIKE")); // % and _ in user search are literal.
        assert!(CONSTRAINT_SQL.contains("n.nspname = $1 AND c.relname = $2 AND fk.conname = $3"));
    }

    #[test]
    fn incomplete_composite_keys_fail_closed() {
        let mut key = key();
        key.columns.pop();
        assert!(preview_sql(&key, &[]).is_err());
        key.columns.clear();
        key.referenced_columns.clear();
        assert!(preview_sql(&key, &[]).is_err());
    }

    #[test]
    fn foreign_key_values_serialize_large_numbers_as_strings_not_doubles() {
        let values = ForeignKeyValues {
            foreign_key: key(),
            preview_columns: vec![],
            has_more: false,
            rows: vec![BTreeMap::from([(
                "ID".into(),
                Some("9223372036854775807".into()),
            )])],
        };
        let json = serde_json::to_value(values).unwrap();
        assert_eq!(json["rows"][0]["ID"], "9223372036854775807");
        assert_eq!(json["foreignKey"]["referencedColumns"][1], "tenant_id");
        assert_eq!(json["hasMore"], false);
    }
}
