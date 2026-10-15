use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod, Runtime};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_postgres::{
    CancelToken, Client as TokioPgClient, Config as PgConfig, NoTls, Row, SimpleQueryMessage,
    config::SslMode,
    types::{FromSql, Kind, Type},
};
use tokio_postgres_rustls::MakeRustlsConnect;
use uuid::Uuid;

use crate::{
    ConnectionFields, CoreError, ReflectionCache, RowKey, row_keys::load_row_keys,
    sql::auto_limit_sql,
};

pub type PgClient = TokioPgClient;
pub type PgPool = Pool;
pub type PooledPgClient = deadpool_postgres::Client;
pub type PgCancelToken = CancelToken;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(8);

mod arrays;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColumnMeta {
    pub name: String,
    pub source_name: Option<String>,
    pub type_oid: u32,
    pub type_name: String,
    pub table_oid: Option<u32>,
    pub table_schema: Option<String>,
    pub table_name: Option<String>,
    pub column_id: Option<i16>,
    pub nullable: Option<bool>,
    pub generated: bool,
    pub primary_key: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryResult {
    pub columns: Vec<ColumnMeta>,
    #[serde(default)]
    pub row_keys: Vec<RowKey>,
    pub rows: Vec<Value>,
    pub row_count: usize,
    pub duration_ms: u128,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timings: Option<QueryTimings>,
    pub auto_limited: bool,
    pub has_more: bool,
    #[serde(default)]
    pub json_values_as_text: bool,
}

/// Monotonic client-side spans, not PostgreSQL's EXPLAIN execution time.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryTimings {
    pub prepare_ms: f64,
    pub metadata_ms: f64,
    pub execute_fetch_ms: f64,
    pub decode_ms: f64,
    pub total_ms: f64,
    #[serde(default)]
    pub prepare_mode: PrepareMode,
    pub metadata_source: MetadataSource,
    pub metadata_queries: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<CommandTimings>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum PrepareMode {
    #[default]
    Separate,
    Combined,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum MetadataSource {
    Cache,
    Refreshed,
    NotNeeded,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandTimings {
    /// Connection lookup/pool checkout, transaction routing and cancellation setup/cleanup.
    pub setup_ms: f64,
    pub history_ms: f64,
    /// Excludes response serialization/IPC, which happens after the command returns.
    pub total_ms: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStats {
    pub active: i64,
    pub idle: i64,
    pub pool_size: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaTree {
    pub schemas: Vec<SchemaItem>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaItem {
    pub schema: String,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub table_oid: u32,
    pub columns: Vec<SchemaColumn>,
    #[serde(default)]
    pub row_keys: Vec<RowKey>,
    #[serde(default)]
    pub foreign_keys: Vec<SchemaForeignKey>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SchemaForeignKey {
    pub name: String,
    pub columns: Vec<String>,
    pub referenced_schema: String,
    pub referenced_table: String,
    pub referenced_columns: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaColumn {
    pub name: String,
    #[serde(default)]
    pub column_id: i16,
    #[serde(default)]
    pub type_oid: u32,
    pub type_name: String,
    pub nullable: bool,
    pub generated: bool,
    pub primary_key: bool,
}

pub async fn test_connection(fields: &ConnectionFields) -> Result<(), CoreError> {
    let client = connect_client(fields).await?;
    client.simple_query("SELECT 1").await?;
    Ok(())
}

pub async fn checkout_pool_client(pool: &PgPool) -> Result<PooledPgClient, CoreError> {
    pool.get().await.map_err(|error| match error {
        deadpool_postgres::PoolError::Backend(error) => CoreError::Postgres(error),
        error => CoreError::message(format!("postgres pool error: {error}")),
    })
}

pub fn create_pool(fields: &ConnectionFields, pool_size: u32) -> Result<PgPool, CoreError> {
    let mut config = postgres_config(fields);
    let max_size = pool_size.clamp(1, 32) as usize;
    let manager_config = ManagerConfig {
        // A held manual transaction is never returned to the pool. Any client
        // that is returned must shed an abandoned/failed transaction before reuse.
        recycling_method: RecyclingMethod::Custom("ROLLBACK".to_string()),
    };

    if fields.ssl {
        config.ssl_mode(SslMode::Require);
        let manager = Manager::from_config(config, make_tls_connector(), manager_config);
        Pool::builder(manager)
            .max_size(max_size)
            .runtime(Runtime::Tokio1)
            .wait_timeout(Some(CONNECT_TIMEOUT))
            .create_timeout(Some(CONNECT_TIMEOUT))
            .recycle_timeout(Some(CONNECT_TIMEOUT))
            .build()
            .map_err(|error| CoreError::message(format!("postgres pool error: {error}")))
    } else {
        config.ssl_mode(SslMode::Disable);
        let manager = Manager::from_config(config, NoTls, manager_config);
        Pool::builder(manager)
            .max_size(max_size)
            .runtime(Runtime::Tokio1)
            .wait_timeout(Some(CONNECT_TIMEOUT))
            .create_timeout(Some(CONNECT_TIMEOUT))
            .recycle_timeout(Some(CONNECT_TIMEOUT))
            .build()
            .map_err(|error| CoreError::message(format!("postgres pool error: {error}")))
    }
}

pub async fn execute_query(
    fields: &ConnectionFields,
    sql: &str,
    offset: usize,
    page_size: usize,
) -> Result<QueryResult, CoreError> {
    let client = connect_client(fields).await?;
    execute_on_client(&client, sql, offset, page_size).await
}

pub async fn execute_on_client(
    client: &PgClient,
    sql: &str,
    offset: usize,
    page_size: usize,
) -> Result<QueryResult, CoreError> {
    execute_on_client_cancellable(client, sql, offset, page_size, None).await
}

pub async fn execute_on_client_cancellable(
    client: &PgClient,
    sql: &str,
    offset: usize,
    page_size: usize,
    cancelled: Option<&AtomicBool>,
) -> Result<QueryResult, CoreError> {
    execute_on_client_with_reflection(
        client,
        sql,
        offset,
        page_size,
        cancelled,
        &ReflectionCache::default(),
    )
    .await
}

pub async fn execute_on_client_with_reflection(
    client: &PgClient,
    sql: &str,
    offset: usize,
    page_size: usize,
    cancelled: Option<&AtomicBool>,
    reflection: &ReflectionCache,
) -> Result<QueryResult, CoreError> {
    // Conservative for DDL, DO/CALL, savepoint rollbacks and compound batches.
    // Invalidate before AND after, including failures/partial batch execution.
    let invalidates = crate::split_statements(sql).iter().any(|statement| {
        crate::classify_statement(&statement.text) != crate::StatementKind::SelectLike
    });
    if invalidates {
        reflection.invalidate();
    }
    let result = execute_reflected(client, sql, offset, page_size, cancelled, reflection).await;
    if invalidates {
        reflection.invalidate();
    }
    result
}

async fn execute_reflected(
    client: &PgClient,
    sql: &str,
    offset: usize,
    page_size: usize,
    cancelled: Option<&AtomicBool>,
    reflection: &ReflectionCache,
) -> Result<QueryResult, CoreError> {
    check_query_cancelled(cancelled)?;
    let requested_page = page_size.clamp(1, 10_000);
    let limited_sql = auto_limit_sql(sql, offset, requested_page);
    let auto_limited = limited_sql.is_some();
    let exec_sql = limited_sql.as_deref().unwrap_or(sql).trim();
    let start = Instant::now();

    if exec_sql.contains(';') && crate::split_statements(exec_sql).len() > 1 {
        return execute_simple(client, exec_sql, start).await;
    }

    // Use a fresh unnamed Parse/Bind/Describe/Execute request for SELECT-like
    // SQL. This saves a round trip without caching stale column origins after
    // search_path/DDL changes. Never retry execution (WITH may perform writes).
    let combined = crate::classify_statement(exec_sql) == crate::StatementKind::SelectLike;
    let (mut columns, prefetched, statement, first_end, described_end) = if combined {
        let rows = client.query_typed(exec_sql, &[]).await?;
        if let Some(first) = rows.first() {
            let columns = column_metadata(first.columns());
            let end = Instant::now();
            (columns, Some(rows), None, end, end)
        } else {
            // The driver's Vec<Row> API does not expose headers for zero rows.
            // Describe with prepare only; do NOT execute SQL a second time.
            let first_end = Instant::now();
            check_query_cancelled(cancelled)?;
            let description = client.prepare(exec_sql).await?;
            let columns = column_metadata(description.columns());
            (columns, Some(rows), None, first_end, Instant::now())
        }
    } else {
        let statement = client.prepare(exec_sql).await?;
        let columns = column_metadata(statement.columns());
        let end = Instant::now();
        (columns, None, Some(statement), end, end)
    };
    check_query_cancelled(cancelled)?;
    let enrichment = reflection.enrich(client, &mut columns, cancelled).await?;
    check_query_cancelled(cancelled)?;
    let enriched_at = Instant::now();
    let (mut rows, fetched_at) = match prefetched {
        Some(rows) => (rows, enriched_at),
        None => {
            let rows = client
                .query(
                    statement
                        .as_ref()
                        .expect("separate preparation has a statement"),
                    &[],
                )
                .await?;
            (rows, Instant::now())
        }
    };
    let mut has_more = false;
    if auto_limited && rows.len() > requested_page {
        has_more = true;
        rows.truncate(requested_page);
    }

    let rows = rows.iter().map(row_to_json).collect::<Vec<_>>();
    let decoded_at = Instant::now();
    let total_ms = decoded_at.duration_since(start).as_secs_f64() * 1000.0;
    Ok(QueryResult {
        row_count: rows.len(),
        columns,
        row_keys: enrichment.keys,
        rows,
        duration_ms: total_ms as u128,
        timings: Some(QueryTimings {
            prepare_ms: if combined {
                described_end.duration_since(first_end)
            } else {
                first_end.duration_since(start)
            }
            .as_secs_f64()
                * 1000.0,
            metadata_ms: enriched_at.duration_since(described_end).as_secs_f64() * 1000.0,
            execute_fetch_ms: if combined {
                first_end.duration_since(start)
            } else {
                fetched_at.duration_since(enriched_at)
            }
            .as_secs_f64()
                * 1000.0,
            decode_ms: decoded_at.duration_since(fetched_at).as_secs_f64() * 1000.0,
            total_ms,
            prepare_mode: if combined {
                PrepareMode::Combined
            } else {
                PrepareMode::Separate
            },
            metadata_source: enrichment.source,
            metadata_queries: enrichment.queries,
            command: None,
        }),
        auto_limited,
        has_more,
        json_values_as_text: true,
    })
}

fn column_metadata(columns: &[tokio_postgres::Column]) -> Vec<ColumnMeta> {
    columns
        .iter()
        .map(|column| ColumnMeta {
            name: column.name().to_string(),
            source_name: None,
            type_oid: column.type_().oid(),
            type_name: column.type_().name().to_string(),
            table_oid: column.table_oid(),
            column_id: column.column_id(),
            table_schema: None,
            table_name: None,
            nullable: None,
            generated: false,
            primary_key: false,
        })
        .collect()
}

pub(crate) fn check_query_cancelled(cancelled: Option<&AtomicBool>) -> Result<(), CoreError> {
    if cancelled.is_some_and(|flag| flag.load(Ordering::Acquire)) {
        return Err(CoreError::Cancelled(
            "Query cancelled before execution.".into(),
        ));
    }
    Ok(())
}

pub fn connection_stats(pool: &PgPool) -> ConnectionStats {
    let status = pool.status();
    ConnectionStats {
        active: status.size.saturating_sub(status.available) as i64,
        idle: status.available as i64,
        pool_size: status.max_size as u32,
    }
}

pub async fn introspect_schema(fields: &ConnectionFields) -> Result<SchemaTree, CoreError> {
    let client = connect_client(fields).await?;
    introspect_schema_on_client(&client).await
}

pub async fn introspect_schema_on_client(client: &PgClient) -> Result<SchemaTree, CoreError> {
    let rows = client
        .query(
            "SELECT
                n.nspname AS schema_name,
                c.relname AS object_name,
                CASE c.relkind
                    WHEN 'r' THEN 'table'
                    WHEN 'p' THEN 'partitioned table'
                    WHEN 'v' THEN 'view'
                    WHEN 'm' THEN 'materialized view'
                    ELSE c.relkind::text
                END AS object_kind,
                a.attname AS column_name,
                pg_catalog.format_type(a.atttypid, a.atttypmod) AS type_name,
                NOT a.attnotnull AS nullable,
                a.attgenerated <> '' AS generated,
                EXISTS (
                    SELECT 1
                    FROM pg_index i
                    WHERE i.indrelid = c.oid
                      AND i.indisprimary
                      AND a.attnum = ANY(i.indkey)
                ) AS primary_key,
                c.oid AS table_oid, a.attnum AS column_id, a.atttypid AS type_oid
             FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
             JOIN pg_attribute a ON a.attrelid = c.oid
             WHERE c.relkind IN ('r', 'p', 'v', 'm')
               AND a.attnum > 0
               AND NOT a.attisdropped
               AND n.nspname NOT IN ('pg_catalog', 'information_schema')
             ORDER BY n.nspname, c.relname, a.attnum",
            &[],
        )
        .await?;

    let mut items: Vec<SchemaItem> = Vec::new();
    for row in rows {
        let schema: String = row.get(0);
        let name: String = row.get(1);
        let kind: String = row.get(2);
        let column = SchemaColumn {
            name: row.get(3),
            column_id: row.get(9),
            type_oid: row.get(10),
            type_name: row.get(4),
            nullable: row.get(5),
            generated: row.get(6),
            primary_key: row.get(7),
        };

        if let Some(item) = items
            .iter_mut()
            .find(|item| item.schema == schema && item.name == name)
        {
            item.columns.push(column);
        } else {
            items.push(SchemaItem {
                schema,
                name,
                kind,
                table_oid: row.get(8),
                columns: vec![column],
                row_keys: Vec::new(),
                foreign_keys: Vec::new(),
            });
        }
    }

    let foreign_keys = client
        .query(
            "SELECT
                n.nspname, c.relname, fk.conname,
                rn.nspname, rc.relname,
                array_agg(a.attname::text ORDER BY k.ordinality),
                array_agg(ra.attname::text ORDER BY k.ordinality)
             FROM pg_constraint fk
             JOIN pg_class c ON c.oid = fk.conrelid
             JOIN pg_namespace n ON n.oid = c.relnamespace
             JOIN pg_class rc ON rc.oid = fk.confrelid
             JOIN pg_namespace rn ON rn.oid = rc.relnamespace
             JOIN LATERAL unnest(fk.conkey, fk.confkey)
                 WITH ORDINALITY AS k(attnum, ref_attnum, ordinality) ON true
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = k.attnum
             JOIN pg_attribute ra ON ra.attrelid = rc.oid AND ra.attnum = k.ref_attnum
             WHERE fk.contype = 'f'
               AND c.relkind IN ('r', 'p')
               AND n.nspname NOT IN ('pg_catalog', 'information_schema')
             GROUP BY fk.oid, n.nspname, c.relname, fk.conname, rn.nspname, rc.relname
             ORDER BY n.nspname, c.relname, fk.conname",
            &[],
        )
        .await?;
    for row in foreign_keys {
        let schema: String = row.get(0);
        let table: String = row.get(1);
        if let Some(item) = items
            .iter_mut()
            .find(|item| item.schema == schema && item.name == table)
        {
            item.foreign_keys.push(SchemaForeignKey {
                name: row.get(2),
                referenced_schema: row.get(3),
                referenced_table: row.get(4),
                columns: row.get(5),
                referenced_columns: row.get(6),
            });
        }
    }
    let table_oids = items.iter().map(|item| item.table_oid).collect::<Vec<_>>();
    for key in load_row_keys(client, &table_oids).await? {
        if let Some(item) = items
            .iter_mut()
            .find(|item| item.schema == key.schema && item.name == key.table)
        {
            item.row_keys.push(key);
        }
    }
    Ok(SchemaTree { schemas: items })
}

pub async fn get_ddl(
    fields: &ConnectionFields,
    schema: &str,
    name: &str,
) -> Result<String, CoreError> {
    let client = connect_client(fields).await?;
    get_ddl_on_client(&client, schema, name).await
}

pub async fn get_ddl_on_client(
    client: &PgClient,
    schema: &str,
    name: &str,
) -> Result<String, CoreError> {
    let row = client
        .query_one(
            "SELECT c.oid, c.relkind
             FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2",
            &[&schema, &name],
        )
        .await?;
    let object_oid: u32 = row.get(0);
    let relkind: i8 = row.get(1);
    let ident = format!("{}.{}", quote_ident(schema), quote_ident(name));

    if relkind as u8 as char == 'v' || relkind as u8 as char == 'm' {
        let view = client
            .query_one("SELECT pg_get_viewdef($1::oid, true)", &[&object_oid])
            .await?;
        let definition: String = view.get(0);
        return Ok(format!("CREATE VIEW {ident} AS\n{definition};"));
    }

    let columns = client
        .query(
            "SELECT
                a.attname,
                pg_catalog.format_type(a.atttypid, a.atttypmod),
                a.attnotnull,
                pg_get_expr(ad.adbin, ad.adrelid) AS default_expr,
                a.attgenerated <> '' AS generated
             FROM pg_attribute a
             LEFT JOIN pg_attrdef ad ON ad.adrelid = a.attrelid AND ad.adnum = a.attnum
             WHERE a.attrelid = $1::oid
               AND a.attnum > 0
               AND NOT a.attisdropped
             ORDER BY a.attnum",
            &[&object_oid],
        )
        .await?;
    let mut lines = Vec::new();
    for row in columns {
        let mut line = format!(
            "  {} {}",
            quote_ident(row.get::<_, String>(0).as_str()),
            row.get::<_, String>(1)
        );
        if let Some(default_expr) = row.get::<_, Option<String>>(3) {
            let generated: bool = row.get(4);
            if generated {
                line.push_str(&format!(" GENERATED ALWAYS AS ({default_expr}) STORED"));
            } else {
                line.push_str(&format!(" DEFAULT {default_expr}"));
            }
        }
        if row.get::<_, bool>(2) {
            line.push_str(" NOT NULL");
        }
        lines.push(line);
    }

    let indexes = client
        .query(
            "SELECT pg_get_indexdef(i.indexrelid)
             FROM pg_index i
             WHERE i.indrelid = $1::oid
             ORDER BY i.indisprimary DESC, i.indexrelid::regclass::text",
            &[&object_oid],
        )
        .await?;
    let index_sql = indexes
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect::<Vec<_>>()
        .join(";\n");

    let mut ddl = format!("CREATE TABLE {ident} (\n{}\n);", lines.join(",\n"));
    if !index_sql.is_empty() {
        ddl.push_str("\n\n");
        ddl.push_str(&index_sql);
        ddl.push(';');
    }
    Ok(ddl)
}

async fn execute_simple(
    client: &PgClient,
    sql: &str,
    start: Instant,
) -> Result<QueryResult, CoreError> {
    let messages = client.simple_query(sql).await?;
    let fetched_at = Instant::now();
    let mut columns = Vec::new();
    let mut rows = Vec::new();
    for message in messages {
        match message {
            SimpleQueryMessage::Row(row) => {
                if columns.is_empty() {
                    columns = row
                        .columns()
                        .iter()
                        .enumerate()
                        .map(|(index, column)| ColumnMeta {
                            name: column.name().to_string(),
                            source_name: None,
                            type_oid: index as u32,
                            type_name: "text".to_string(),
                            table_oid: None,
                            table_schema: None,
                            table_name: None,
                            column_id: None,
                            nullable: None,
                            generated: false,
                            primary_key: false,
                        })
                        .collect();
                }
                let mut object = serde_json::Map::new();
                for (index, column) in row.columns().iter().enumerate() {
                    object.insert(
                        column.name().to_string(),
                        row.get(index).map(Value::from).unwrap_or(Value::Null),
                    );
                }
                rows.push(Value::Object(object));
            }
            SimpleQueryMessage::CommandComplete(_) => {}
            _ => {}
        }
    }

    let decoded_at = Instant::now();
    let total_ms = decoded_at.duration_since(start).as_secs_f64() * 1000.0;
    Ok(QueryResult {
        row_count: rows.len(),
        columns,
        row_keys: Vec::new(),
        rows,
        duration_ms: total_ms as u128,
        timings: Some(QueryTimings {
            prepare_ms: 0.0,
            metadata_ms: 0.0,
            execute_fetch_ms: fetched_at.duration_since(start).as_secs_f64() * 1000.0,
            decode_ms: decoded_at.duration_since(fetched_at).as_secs_f64() * 1000.0,
            total_ms,
            prepare_mode: PrepareMode::Separate,
            metadata_source: MetadataSource::NotNeeded,
            metadata_queries: 0,
            command: None,
        }),
        auto_limited: false,
        has_more: false,
        json_values_as_text: false,
    })
}

pub async fn connect_client(fields: &ConnectionFields) -> Result<PgClient, CoreError> {
    let mut config = postgres_config(fields);

    if fields.ssl {
        config.ssl_mode(SslMode::Require);
        let (client, connection) = config.connect(make_tls_connector()).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(client)
    } else {
        config.ssl_mode(SslMode::Disable);
        let (client, connection) = config.connect(NoTls).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(client)
    }
}

fn postgres_config(fields: &ConnectionFields) -> PgConfig {
    let mut config = PgConfig::new();
    config
        .host(&fields.host)
        .port(fields.port)
        .user(&fields.username)
        .dbname(&fields.database)
        .application_name("catabase")
        .connect_timeout(CONNECT_TIMEOUT);
    if let Some(password) = &fields.password {
        config.password(password);
    }
    config
}

pub async fn cancel_query_token(token: PgCancelToken, ssl: bool) -> Result<(), CoreError> {
    if ssl {
        token.cancel_query(make_tls_connector()).await?;
    } else {
        token.cancel_query(NoTls).await?;
    }
    Ok(())
}

fn make_tls_connector() -> MakeRustlsConnect {
    let certs = rustls_native_certs::load_native_certs();
    let mut root_store = rustls::RootCertStore::empty();
    root_store.add_parsable_certificates(certs.certs);
    let tls_config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();
    MakeRustlsConnect::new(tls_config)
}

fn row_to_json(row: &Row) -> Value {
    let mut object = serde_json::Map::new();
    for (index, column) in row.columns().iter().enumerate() {
        object.insert(
            column.name().to_string(),
            cell_to_json(row, index, column.type_()),
        );
    }
    Value::Object(object)
}

fn cell_to_json(row: &Row, index: usize, ty: &Type) -> Value {
    if matches!(ty.kind(), Kind::Array(_)) {
        return match row.try_get::<_, Option<arrays::PgArray>>(index) {
            Ok(Some(value)) => Value::String(value.0),
            Ok(None) => Value::Null,
            Err(_) => Value::String(format!("<{}>", ty.name())),
        };
    }
    macro_rules! get_optional {
        ($type:ty) => {
            row.try_get::<usize, Option<$type>>(index)
                .ok()
                .flatten()
                .map(|value| json!(value))
                .unwrap_or(Value::Null)
        };
    }

    match *ty {
        Type::BOOL => get_optional!(bool),
        Type::INT2 => get_optional!(i16),
        Type::INT4 => get_optional!(i32),
        Type::INT8 => row
            .try_get::<_, Option<i64>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
        Type::NUMERIC => row
            .try_get::<_, Option<ExactNumeric>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.0))
            .unwrap_or(Value::Null),
        Type::FLOAT4 => get_optional!(f32),
        Type::FLOAT8 => get_optional!(f64),
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => get_optional!(String),
        Type::UUID => row
            .try_get::<usize, Option<Uuid>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
        Type::JSON | Type::JSONB => row
            .try_get::<usize, Option<RawJson>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.0))
            .unwrap_or(Value::Null),
        Type::DATE => row
            .try_get::<usize, Option<NaiveDate>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
        Type::TIMESTAMP => row
            .try_get::<usize, Option<NaiveDateTime>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
        Type::TIMESTAMPTZ => row
            .try_get::<usize, Option<DateTime<Utc>>>(index)
            .ok()
            .flatten()
            .map(|value| Value::String(value.to_rfc3339()))
            .unwrap_or(Value::Null),
        _ => row
            .try_get::<usize, Option<String>>(index)
            .ok()
            .flatten()
            .map(Value::String)
            .unwrap_or(Value::String(format!("<{}>", ty.name()))),
    }
}

fn quote_ident(input: &str) -> String {
    format!("\"{}\"", input.replace('"', "\"\""))
}

/// PostgreSQL NUMERIC uses base-10,000 digits. Decode directly into text to avoid
/// passing exact decimals or large integers through JavaScript's IEEE-754 Number.
struct ExactNumeric(String);

struct RawJson(String);

impl<'a> FromSql<'a> for RawJson {
    fn from_sql(
        ty: &Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        let text = if *ty == Type::JSONB {
            if raw.first() != Some(&1) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Unsupported JSONB format",
                )
                .into());
            }
            &raw[1..]
        } else {
            raw
        };
        Ok(Self(std::str::from_utf8(text)?.to_string()))
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::JSON || *ty == Type::JSONB
    }
}

impl<'a> FromSql<'a> for ExactNumeric {
    fn from_sql(_: &Type, raw: &'a [u8]) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        decode_numeric(raw).map(Self).map_err(Into::into)
    }

    fn accepts(ty: &Type) -> bool {
        *ty == Type::NUMERIC
    }
}

fn decode_numeric(raw: &[u8]) -> Result<String, std::io::Error> {
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid PostgreSQL numeric value",
        )
    };
    if raw.len() < 8 {
        return Err(invalid());
    }
    let count = u16::from_be_bytes([raw[0], raw[1]]) as usize;
    let weight = i16::from_be_bytes([raw[2], raw[3]]) as i32;
    let sign = u16::from_be_bytes([raw[4], raw[5]]);
    let scale = u16::from_be_bytes([raw[6], raw[7]]) as usize;
    match sign {
        0xc000 => return Ok("NaN".into()),
        0xd000 => return Ok("Infinity".into()),
        0xf000 => return Ok("-Infinity".into()),
        0 | 0x4000 => {}
        _ => return Err(invalid()),
    }
    if raw.len() != 8 + count * 2 {
        return Err(invalid());
    }
    let digits = raw[8..]
        .chunks_exact(2)
        .map(|part| u16::from_be_bytes([part[0], part[1]]))
        .collect::<Vec<_>>();
    if digits.iter().any(|digit| *digit > 9999) {
        return Err(invalid());
    }
    let digit_at = |power: i32| -> u16 {
        let index = weight - power;
        if index < 0 {
            0
        } else {
            digits.get(index as usize).copied().unwrap_or(0)
        }
    };
    let mut integer = String::new();
    if weight < 0 {
        integer.push('0');
    } else {
        for power in (0..=weight).rev() {
            integer.push_str(&format!("{:04}", digit_at(power)));
        }
        let trimmed = integer.trim_start_matches('0');
        integer = if trimmed.is_empty() {
            "0".into()
        } else {
            trimmed.to_string()
        };
    }
    if scale > 0 {
        let mut fraction = String::new();
        for index in 1..=scale.div_ceil(4) {
            fraction.push_str(&format!("{:04}", digit_at(-(index as i32))));
        }
        fraction.truncate(scale);
        integer.push('.');
        integer.push_str(&fraction);
    }
    if sign == 0x4000 {
        integer.insert(0, '-');
    }
    Ok(integer)
}

#[cfg(test)]
mod schema_tests {
    use super::*;

    #[test]
    fn legacy_schema_items_default_to_no_foreign_keys() {
        let item: SchemaItem = serde_json::from_value(json!({
            "schema": "public", "name": "properties", "kind": "table", "columns": []
        }))
        .unwrap();
        assert!(item.foreign_keys.is_empty());
    }

    #[test]
    fn foreign_key_wire_metadata_preserves_ordered_composite_column_pairs() {
        let key = SchemaForeignKey {
            name: "parent_fk".into(),
            columns: vec!["parent_id".into(), "tenant_id".into()],
            referenced_schema: "pms".into(),
            referenced_table: "parents".into(),
            referenced_columns: vec!["id".into(), "tenant".into()],
        };
        let encoded = serde_json::to_value(&key).unwrap();
        assert_eq!(encoded["referencedSchema"], "pms");
        assert_eq!(encoded["columns"], json!(["parent_id", "tenant_id"]));
        assert_eq!(encoded["referencedColumns"], json!(["id", "tenant"]));
        assert_eq!(
            serde_json::from_value::<SchemaForeignKey>(encoded).unwrap(),
            key
        );
    }
}

#[cfg(test)]
mod numeric_tests {
    use super::*;

    fn numeric(weight: i16, sign: u16, scale: u16, digits: &[u16]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(digits.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&weight.to_be_bytes());
        bytes.extend_from_slice(&sign.to_be_bytes());
        bytes.extend_from_slice(&scale.to_be_bytes());
        for digit in digits {
            bytes.extend_from_slice(&digit.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn numeric_preserves_large_values_scale_and_fractional_zero_groups() {
        assert_eq!(
            decode_numeric(&numeric(4, 0, 0, &[922, 3372, 368, 5477, 5807])).unwrap(),
            "9223372036854775807"
        );
        assert_eq!(
            decode_numeric(&numeric(0, 0x4000, 8, &[12, 3456, 7890])).unwrap(),
            "-12.34567890"
        );
        assert_eq!(
            decode_numeric(&numeric(-2, 0, 8, &[1])).unwrap(),
            "0.00000001"
        );
        assert_eq!(
            decode_numeric(&numeric(2, 0, 2, &[12])).unwrap(),
            "1200000000.00"
        );
        assert_eq!(decode_numeric(&numeric(0, 0, 3, &[])).unwrap(), "0.000");
    }

    #[test]
    fn numeric_handles_special_values_and_rejects_malformed_input() {
        assert_eq!(decode_numeric(&numeric(0, 0xc000, 0, &[])).unwrap(), "NaN");
        assert_eq!(
            decode_numeric(&numeric(0, 0xf000, 0, &[])).unwrap(),
            "-Infinity"
        );
        assert!(decode_numeric(&[0, 0]).is_err());
        assert!(decode_numeric(&numeric(0, 0, 0, &[10000])).is_err());
    }

    #[test]
    fn json_keeps_exact_number_literals_and_literal_null() {
        let raw = br#"{"number":9223372036854775807,"decimal":0.123456789012345678901}"#;
        assert_eq!(
            RawJson::from_sql(&Type::JSON, raw).unwrap().0.as_bytes(),
            raw
        );
        assert_eq!(
            RawJson::from_sql(&Type::JSONB, b"\x01null").unwrap().0,
            "null"
        );
        assert!(RawJson::from_sql(&Type::JSONB, b"\x02{}").is_err());
    }

    #[test]
    fn legacy_results_default_to_decoded_json_values() {
        let result: QueryResult = serde_json::from_value(json!({
            "columns": [], "rows": [{"payload": "null"}], "rowCount": 1,
            "durationMs": 0, "autoLimited": false, "hasMore": false
        }))
        .unwrap();
        assert!(!result.json_values_as_text);
        assert!(result.timings.is_none());
        assert!(result.row_keys.is_empty());
        assert_eq!(result.rows[0]["payload"], "null");
    }

    #[test]
    fn timing_details_keep_fractional_milliseconds_and_use_camel_case() {
        let timing = QueryTimings {
            prepare_ms: 1.25,
            metadata_ms: 0.01,
            execute_fetch_ms: 3.5,
            decode_ms: 0.24,
            total_ms: 5.0,
            prepare_mode: PrepareMode::Separate,
            metadata_source: MetadataSource::Cache,
            metadata_queries: 0,
            command: Some(CommandTimings {
                setup_ms: 2.0,
                history_ms: 1.0,
                total_ms: 8.0,
            }),
        };
        let encoded = serde_json::to_value(&timing).unwrap();
        assert_eq!(encoded["prepareMs"], 1.25);
        assert_eq!(encoded["metadataSource"], "cache");
        assert_eq!(encoded["metadataQueries"], 0);
        assert_eq!(encoded["command"]["historyMs"], 1.0);
        let restored: QueryTimings = serde_json::from_value(encoded).unwrap();
        assert_eq!(restored.total_ms, 5.0);
        assert_eq!(restored.prepare_mode, PrepareMode::Separate);
        let mut legacy = serde_json::to_value(&timing).unwrap();
        legacy.as_object_mut().unwrap().remove("prepareMode");
        let restored: QueryTimings = serde_json::from_value(legacy).unwrap();
        assert_eq!(restored.prepare_mode, PrepareMode::Separate);
    }

    #[test]
    fn cancellation_remains_effective_after_preparation_or_metadata_work() {
        let flag = AtomicBool::new(false);
        assert!(check_query_cancelled(Some(&flag)).is_ok());
        flag.store(true, Ordering::Release);
        let error = check_query_cancelled(Some(&flag)).unwrap_err();
        assert_eq!(error.pg_info().unwrap().code.as_deref(), Some("57014"));
        assert!(check_query_cancelled(None).is_ok());
    }
}
