use serde::{Deserialize, Serialize};

use crate::{CoreError, PgClient};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowKey {
    pub schema: String,
    pub table: String,
    pub columns: Vec<String>,
    pub primary_key: bool,
    pub nulls_not_distinct: bool,
}

/// Only globally enforced, immediate, plain-column keys can identify an edit.
/// INCLUDE columns are not part of a key. Partial/expression/invalid/deferred
/// indexes cannot establish uniqueness for an arbitrary result projection.
pub(crate) async fn load_row_keys(
    client: &PgClient,
    table_oids: &[u32],
) -> Result<Vec<RowKey>, CoreError> {
    if table_oids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = client
        .query(
            "SELECT n.nspname, c.relname, i.indisprimary,
                    COALESCE((to_jsonb(i)->>'indnullsnotdistinct')::boolean, false),
                    array_agg(a.attname::text ORDER BY k.position)
             FROM pg_index i
             JOIN pg_class c ON c.oid = i.indrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             CROSS JOIN LATERAL unnest(i.indkey) WITH ORDINALITY AS k(attnum, position)
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum
             JOIN pg_opclass op ON op.oid = i.indclass[(k.position - 1)::integer]
             WHERE c.oid = ANY($1::oid[]) AND c.relkind IN ('r', 'p')
               AND i.indisunique AND i.indisvalid AND i.indisready AND i.indislive
               AND i.indimmediate AND i.indpred IS NULL AND i.indexprs IS NULL
               AND op.opcdefault AND i.indcollation[(k.position - 1)::integer] = a.attcollation
               AND (c.relkind = 'p' OR NOT EXISTS (SELECT 1 FROM pg_inherits h WHERE h.inhparent = c.oid))
               AND k.position <= i.indnkeyatts AND a.attnum > 0 AND NOT a.attisdropped
             GROUP BY n.nspname, c.relname, i.indexrelid, i.indisprimary,
                      i.indnkeyatts, COALESCE((to_jsonb(i)->>'indnullsnotdistinct')::boolean, false)
             HAVING count(*) = i.indnkeyatts
             ORDER BY i.indisprimary DESC, i.indnkeyatts, i.indexrelid",
            &[&table_oids],
        )
        .await?;
    Ok(rows
        .into_iter()
        .map(|row| RowKey {
            schema: row.get(0),
            table: row.get(1),
            primary_key: row.get(2),
            nulls_not_distinct: row.get(3),
            columns: row.get(4),
        })
        .collect())
}
