//! Metadata belongs to the backend connection/pool, never to frontend labels.
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use tokio::sync::{Mutex, RwLock};

use crate::{
    ColumnMeta, CoreError, PgClient, RowKey, SchemaTree, pg::check_query_cancelled,
    row_keys::load_row_keys,
};

const MAX_AGE: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct ReflectedColumn {
    name: String,
    type_oid: u32,
    nullable: bool,
    generated: bool,
    primary_key: bool,
}

#[derive(Clone, Default)]
struct ReflectedTable {
    schema: String,
    name: String,
    columns: HashMap<i16, ReflectedColumn>,
    keys: Vec<RowKey>,
}

struct CachedTable {
    generation: u64,
    loaded_at: Instant,
    table: Arc<ReflectedTable>,
}

/// Create a new cache for each pool/configuration and each manual transaction.
/// Invalidation epochs prevent late reflection requests from reviving old data.
#[derive(Default)]
pub struct ReflectionCache {
    generation: AtomicU64,
    tables: RwLock<HashMap<u32, CachedTable>>,
    loading: Mutex<()>,
}

impl ReflectionCache {
    pub fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Explicit sidebar reflection refreshes both the UI tree and backend cache.
    pub async fn refresh(&self, client: &PgClient) -> Result<SchemaTree, CoreError> {
        self.invalidate();
        let _loading = self.loading.lock().await;
        let generation = self.generation.load(Ordering::Acquire);
        let tree = crate::introspect_schema_on_client(client).await?;
        let tables = reflected_tables(&tree);
        self.publish(generation, tables, true).await;
        Ok(tree)
    }

    async fn publish(&self, generation: u64, tables: HashMap<u32, ReflectedTable>, replace: bool) {
        let mut cached = self.tables.write().await;
        if generation != self.generation.load(Ordering::Acquire) {
            return;
        }
        // Keep the cache bounded, including after many temporary table/DDL runs.
        cached.retain(|_, entry| {
            entry.generation == generation && entry.loaded_at.elapsed() < MAX_AGE
        });
        if replace {
            cached.clear();
        }
        for (oid, table) in tables {
            cached.insert(
                oid,
                CachedTable {
                    generation,
                    loaded_at: Instant::now(),
                    table: Arc::new(table),
                },
            );
        }
    }

    async fn available(&self, oids: &[u32]) -> HashMap<u32, Arc<ReflectedTable>> {
        let cached = self.tables.read().await;
        let generation = self.generation.load(Ordering::Acquire);
        oids.iter()
            .filter_map(|oid| {
                cached
                    .get(oid)
                    .filter(|entry| {
                        entry.generation == generation && entry.loaded_at.elapsed() < MAX_AGE
                    })
                    .map(|entry| (*oid, entry.table.clone()))
            })
            .collect()
    }

    pub(crate) async fn enrich(
        &self,
        client: &PgClient,
        columns: &mut [ColumnMeta],
        cancelled: Option<&std::sync::atomic::AtomicBool>,
    ) -> Result<Vec<RowKey>, CoreError> {
        let oids = columns
            .iter()
            .filter_map(|column| column.table_oid)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if oids.is_empty() {
            return Ok(Vec::new());
        }
        let mut tables = self.available(&oids).await;
        if !missing_tables(columns, &tables).is_empty() {
            let _loading = self.loading.lock().await;
            check_query_cancelled(cancelled)?;
            tables = self.available(&oids).await;
            let missing = missing_tables(columns, &tables);
            if !missing.is_empty() {
                let generation = self.generation.load(Ordering::Acquire);
                let fresh = load_tables(client, &missing, cancelled).await?;
                // Apply this result even if invalidated concurrently, but never
                // retain its metadata for subsequent queries in that case.
                for (oid, table) in &fresh {
                    tables.insert(*oid, Arc::new(table.clone()));
                }
                self.publish(generation, fresh, false).await;
            }
        }
        check_query_cancelled(cancelled)?;
        Ok(apply_tables(columns, &tables))
    }
}

fn reflected_tables(tree: &SchemaTree) -> HashMap<u32, ReflectedTable> {
    tree.schemas
        .iter()
        .filter(|item| item.table_oid != 0)
        .map(|item| {
            (
                item.table_oid,
                ReflectedTable {
                    schema: item.schema.clone(),
                    name: item.name.clone(),
                    columns: item
                        .columns
                        .iter()
                        .filter(|column| column.column_id > 0 && column.type_oid != 0)
                        .map(|column| {
                            (
                                column.column_id,
                                ReflectedColumn {
                                    name: column.name.clone(),
                                    type_oid: column.type_oid,
                                    nullable: column.nullable,
                                    generated: column.generated,
                                    primary_key: column.primary_key,
                                },
                            )
                        })
                        .collect(),
                    keys: item
                        .row_keys
                        .iter()
                        .filter(|key| key.schema == item.schema && key.table == item.name)
                        .cloned()
                        .collect(),
                },
            )
        })
        .collect()
}

fn missing_tables(columns: &[ColumnMeta], tables: &HashMap<u32, Arc<ReflectedTable>>) -> Vec<u32> {
    columns
        .iter()
        .filter_map(|column| {
            let oid = column.table_oid?;
            let table = tables.get(&oid);
            let missing = table.is_none()
                || column.column_id.filter(|id| *id > 0).is_some_and(|id| {
                    table
                        .and_then(|table| table.columns.get(&id))
                        .is_none_or(|cached| cached.type_oid != column.type_oid)
                });
            missing.then_some(oid)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn apply_tables(
    columns: &mut [ColumnMeta],
    tables: &HashMap<u32, Arc<ReflectedTable>>,
) -> Vec<RowKey> {
    for column in columns.iter_mut() {
        let Some((oid, id)) = column.table_oid.zip(column.column_id) else {
            continue;
        };
        let Some(table) = tables.get(&oid) else {
            continue;
        };
        let Some(source) = table.columns.get(&id) else {
            continue;
        };
        column.source_name = Some(source.name.clone());
        column.table_schema = Some(table.schema.clone());
        column.table_name = Some(table.name.clone());
        column.nullable = Some(source.nullable);
        column.generated = source.generated;
        column.primary_key = source.primary_key;
    }
    columns
        .iter()
        .filter_map(|column| column.table_oid)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|oid| tables.get(&oid))
        .flat_map(|table| table.keys.clone())
        .collect()
}

/// Unknown/expired tables are loaded as a set, including system/temporary tables
/// not present in the sidebar tree. No query-per-column fallback is allowed.
async fn load_tables(
    client: &PgClient,
    oids: &[u32],
    cancelled: Option<&std::sync::atomic::AtomicBool>,
) -> Result<HashMap<u32, ReflectedTable>, CoreError> {
    let rows = client
        .query(
            "SELECT c.oid, n.nspname, c.relname, a.attnum, a.attname, a.atttypid,
                NOT a.attnotnull, a.attgenerated <> '',
                EXISTS (SELECT 1 FROM pg_index i WHERE i.indrelid = c.oid
                        AND i.indisprimary AND a.attnum = ANY(i.indkey))
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
         JOIN pg_attribute a ON a.attrelid = c.oid
         WHERE c.oid = ANY($1::oid[]) AND a.attnum > 0 AND NOT a.attisdropped",
            &[&oids],
        )
        .await?;
    check_query_cancelled(cancelled)?;
    let mut tables = HashMap::<u32, ReflectedTable>::new();
    for row in rows {
        let oid: u32 = row.get(0);
        let table = tables.entry(oid).or_insert_with(|| ReflectedTable {
            schema: row.get(1),
            name: row.get(2),
            ..Default::default()
        });
        table.columns.insert(
            row.get(3),
            ReflectedColumn {
                name: row.get(4),
                type_oid: row.get(5),
                nullable: row.get(6),
                generated: row.get(7),
                primary_key: row.get(8),
            },
        );
    }
    for key in load_row_keys(client, oids).await? {
        if let Some(table) = tables
            .values_mut()
            .find(|table| table.schema == key.schema && table.name == key.table)
        {
            table.keys.push(key);
        }
    }
    check_query_cancelled(cancelled)?;
    Ok(tables)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tree(oid: u32, table: &str) -> SchemaTree {
        serde_json::from_value(json!({ "schemas": [{
            "schema": "public", "name": table, "kind": "table", "tableOid": oid,
            "columns": [
                { "name": "id", "columnId": 1, "typeOid": 23, "typeName": "integer", "nullable": false, "generated": false, "primaryKey": true },
                { "name": "code", "columnId": 4, "typeOid": 25, "typeName": "text", "nullable": true, "generated": true, "primaryKey": false }
            ],
            "rowKeys": [
                { "schema": "public", "table": table, "columns": ["id", "code"], "primaryKey": true, "nullsNotDistinct": false },
                { "schema": "public", "table": table, "columns": ["code"], "primaryKey": false, "nullsNotDistinct": true }
            ]
        }]})).unwrap()
    }

    fn column(name: &str, oid: Option<u32>, id: Option<i16>, type_oid: u32) -> ColumnMeta {
        ColumnMeta {
            name: name.into(),
            source_name: None,
            type_oid,
            type_name: "text".into(),
            table_oid: oid,
            table_schema: None,
            table_name: None,
            column_id: id,
            nullable: None,
            generated: false,
            primary_key: false,
        }
    }

    #[test]
    fn reflection_maps_physical_ids_not_aliases_or_matching_display_names() {
        let mut reflected = reflected_tables(&tree(42, "items"));
        reflected.extend(reflected_tables(&tree(43, "other")));
        let tables = reflected
            .into_iter()
            .map(|(oid, table)| (oid, Arc::new(table)))
            .collect();
        let mut columns = vec![
            column("renamed_key", Some(42), Some(1), 23),
            column("id", Some(43), Some(4), 25),
            column("id", None, None, 23),
        ];
        let keys = apply_tables(&mut columns, &tables);
        assert_eq!(columns[0].name, "renamed_key");
        assert_eq!(columns[0].source_name.as_deref(), Some("id"));
        assert_eq!(columns[0].table_name.as_deref(), Some("items"));
        assert_eq!(columns[0].nullable, Some(false));
        assert!(columns[0].primary_key);
        assert_eq!(columns[1].source_name.as_deref(), Some("code"));
        assert_eq!(columns[1].table_name.as_deref(), Some("other"));
        assert!(columns[1].generated);
        assert!(columns[2].source_name.is_none());
        assert!(columns[2].table_schema.is_none());
        assert_eq!(keys.len(), 4);
        assert_eq!(keys[0].columns, ["id", "code"]);
        assert!(keys[1].nulls_not_distinct);
    }

    #[test]
    fn fallback_is_deduplicated_by_table_and_detects_new_columns_or_type_changes() {
        let tables = reflected_tables(&tree(42, "items"))
            .into_iter()
            .map(|(oid, table)| (oid, Arc::new(table)))
            .collect();
        assert!(missing_tables(&[column("alias", Some(42), Some(1), 23)], &tables).is_empty());
        assert_eq!(
            missing_tables(
                &[
                    column("alias", Some(42), Some(1), 25),
                    column("new", Some(42), Some(8), 25),
                    column("join", Some(43), Some(1), 23)
                ],
                &tables
            ),
            [42, 43]
        );
        assert!(missing_tables(&[column("id", None, None, 23)], &tables).is_empty());
    }

    #[tokio::test]
    async fn invalidation_rejects_in_flight_reflection_and_refresh_replaces_old_tables() {
        let cache = ReflectionCache::default();
        cache
            .publish(0, reflected_tables(&tree(42, "before")), true)
            .await;
        assert_eq!(cache.available(&[42]).await[&42].name, "before");
        cache.invalidate();
        cache
            .publish(0, reflected_tables(&tree(42, "late")), true)
            .await;
        assert!(cache.available(&[42]).await.is_empty());
        cache
            .publish(1, reflected_tables(&tree(43, "fresh")), true)
            .await;
        assert!(cache.available(&[42]).await.is_empty());
        assert_eq!(cache.available(&[43]).await[&43].name, "fresh");
    }

    #[tokio::test]
    async fn cache_scopes_cannot_mix_connections_or_transaction_private_metadata() {
        let shared = ReflectionCache::default();
        let private = ReflectionCache::default();
        shared
            .publish(0, reflected_tables(&tree(42, "committed")), true)
            .await;
        private
            .publish(0, reflected_tables(&tree(42, "uncommitted")), true)
            .await;
        private.invalidate();
        assert!(private.available(&[42]).await.is_empty());
        assert_eq!(shared.available(&[42]).await[&42].name, "committed");
    }

    #[tokio::test]
    async fn entries_expire_and_partial_refresh_does_not_extend_other_tables_age() {
        let cache = ReflectionCache::default();
        cache
            .publish(0, reflected_tables(&tree(42, "expired")), true)
            .await;
        cache.tables.write().await.get_mut(&42).unwrap().loaded_at =
            Instant::now() - MAX_AGE - Duration::from_secs(1);
        assert!(cache.available(&[42]).await.is_empty());
        cache
            .publish(0, reflected_tables(&tree(43, "fresh")), false)
            .await;
        assert!(cache.available(&[42]).await.is_empty());
        assert_eq!(cache.available(&[43]).await[&43].name, "fresh");
        assert_eq!(cache.tables.read().await.len(), 1);
    }

    #[test]
    fn legacy_reflection_without_ids_cannot_be_mapped_by_name() {
        let old: SchemaTree = serde_json::from_value(json!({"schemas":[{"schema":"public","name":"items","kind":"table","columns":[{"name":"id","typeName":"integer","nullable":false,"generated":false,"primaryKey":true}]}]})).unwrap();
        assert!(reflected_tables(&old).is_empty());
        assert!(old.schemas[0].row_keys.is_empty());
    }
}
