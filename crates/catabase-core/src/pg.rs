use std::{collections::HashMap, time::Instant};

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_postgres::{
    CancelToken, Client, Config, NoTls, Row, SimpleQueryMessage, config::SslMode, types::Type,
};
use tokio_postgres_rustls::MakeRustlsConnect;
use uuid::Uuid;

use crate::{ConnectionFields, CoreError, sql::auto_limit_sql};

pub type PgClient = Client;
pub type PgCancelToken = CancelToken;

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
    pub rows: Vec<Value>,
    pub row_count: usize,
    pub duration_ms: u128,
    pub auto_limited: bool,
    pub has_more: bool,
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
    pub columns: Vec<SchemaColumn>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchemaColumn {
    pub name: String,
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
    let requested_page = page_size.clamp(1, 10_000);
    let limited_sql = auto_limit_sql(sql, offset, requested_page);
    let auto_limited = limited_sql.is_some();
    let exec_sql = limited_sql.as_deref().unwrap_or(sql).trim();
    let start = Instant::now();

    if exec_sql.contains(';') && crate::split_statements(exec_sql).len() > 1 {
        return execute_simple(client, exec_sql, start).await;
    }

    let statement = client.prepare(exec_sql).await?;
    let columns = column_metas(client, statement.columns()).await?;
    let mut rows = client.query(&statement, &[]).await?;
    let mut has_more = false;
    if auto_limited && rows.len() > requested_page {
        has_more = true;
        rows.truncate(requested_page);
    }

    let rows = rows.iter().map(row_to_json).collect::<Vec<_>>();
    Ok(QueryResult {
        row_count: rows.len(),
        columns,
        rows,
        duration_ms: start.elapsed().as_millis(),
        auto_limited,
        has_more,
    })
}

pub async fn connection_stats(
    fields: &ConnectionFields,
    pool_size: u32,
) -> Result<ConnectionStats, CoreError> {
    let client = connect_client(fields).await?;
    let row = client
        .query_one(
            "SELECT
                COUNT(*) FILTER (WHERE state = 'active')::bigint AS active,
                COUNT(*) FILTER (WHERE state = 'idle')::bigint AS idle
             FROM pg_stat_activity
             WHERE datname = current_database()",
            &[],
        )
        .await?;
    Ok(ConnectionStats {
        active: row.get::<_, Option<i64>>(0).unwrap_or(0),
        idle: row.get::<_, Option<i64>>(1).unwrap_or(0),
        pool_size,
    })
}

pub async fn introspect_schema(fields: &ConnectionFields) -> Result<SchemaTree, CoreError> {
    let client = connect_client(fields).await?;
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
                ) AS primary_key
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
                columns: vec![column],
            });
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
    let row = client
        .query_one(
            "SELECT c.relkind
             FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
             WHERE n.nspname = $1 AND c.relname = $2",
            &[&schema, &name],
        )
        .await?;
    let relkind: i8 = row.get(0);
    let ident = format!("{}.{}", quote_ident(schema), quote_ident(name));

    if relkind as u8 as char == 'v' || relkind as u8 as char == 'm' {
        let view = client
            .query_one("SELECT pg_get_viewdef($1::regclass, true)", &[&ident])
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
             WHERE a.attrelid = $1::regclass
               AND a.attnum > 0
               AND NOT a.attisdropped
             ORDER BY a.attnum",
            &[&ident],
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
             WHERE i.indrelid = $1::regclass
             ORDER BY i.indisprimary DESC, i.indexrelid::regclass::text",
            &[&ident],
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
    client: &Client,
    sql: &str,
    start: Instant,
) -> Result<QueryResult, CoreError> {
    let messages = client.simple_query(sql).await?;
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

    Ok(QueryResult {
        row_count: rows.len(),
        columns,
        rows,
        duration_ms: start.elapsed().as_millis(),
        auto_limited: false,
        has_more: false,
    })
}

pub async fn connect_client(fields: &ConnectionFields) -> Result<PgClient, CoreError> {
    let mut config = Config::new();
    config
        .host(&fields.host)
        .port(fields.port)
        .user(&fields.username)
        .dbname(&fields.database)
        .application_name("catabase");
    if let Some(password) = &fields.password {
        config.password(password);
    }

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

async fn column_metas(
    client: &PgClient,
    columns: &[tokio_postgres::Column],
) -> Result<Vec<ColumnMeta>, CoreError> {
    let mut catalog = HashMap::new();
    for column in columns {
        let Some(table_oid) = column.table_oid() else {
            continue;
        };
        let Some(column_id) = column.column_id() else {
            continue;
        };
        let key = (table_oid, column_id);
        if catalog.contains_key(&key) {
            continue;
        }

        if let Some(row) = client
            .query_opt(
                "SELECT
                    n.nspname,
                    c.relname,
                    a.attname,
                    NOT a.attnotnull,
                    a.attgenerated <> '',
                    EXISTS (
                        SELECT 1
                        FROM pg_index i
                        WHERE i.indrelid = c.oid
                          AND i.indisprimary
                          AND a.attnum = ANY(i.indkey)
                    )
                 FROM pg_class c
                 JOIN pg_namespace n ON n.oid = c.relnamespace
                 JOIN pg_attribute a ON a.attrelid = c.oid
                 WHERE c.oid = $1::oid
                   AND a.attnum = $2
                   AND NOT a.attisdropped",
                &[&table_oid.to_string(), &column_id],
            )
            .await?
        {
            catalog.insert(
                key,
                (
                    row.get::<_, String>(0),
                    row.get::<_, String>(1),
                    row.get::<_, String>(2),
                    row.get::<_, bool>(3),
                    row.get::<_, bool>(4),
                    row.get::<_, bool>(5),
                ),
            );
        }
    }

    Ok(columns
        .iter()
        .map(|column| {
            let key = column.table_oid().zip(column.column_id());
            let catalog_entry = key.and_then(|key| catalog.get(&key));
            ColumnMeta {
                name: column.name().to_string(),
                source_name: catalog_entry.map(|(_, _, source_name, _, _, _)| source_name.clone()),
                type_oid: column.type_().oid(),
                type_name: column.type_().name().to_string(),
                table_oid: column.table_oid(),
                table_schema: catalog_entry.map(|(schema, _, _, _, _, _)| schema.clone()),
                table_name: catalog_entry.map(|(_, table, _, _, _, _)| table.clone()),
                column_id: column.column_id(),
                nullable: catalog_entry.map(|(_, _, _, nullable, _, _)| *nullable),
                generated: catalog_entry
                    .map(|(_, _, _, _, generated, _)| *generated)
                    .unwrap_or(false),
                primary_key: catalog_entry
                    .map(|(_, _, _, _, _, primary_key)| *primary_key)
                    .unwrap_or(false),
            }
        })
        .collect())
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
        Type::INT8 => get_optional!(i64),
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
            .try_get::<usize, Option<Value>>(index)
            .ok()
            .flatten()
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
