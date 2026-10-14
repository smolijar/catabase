//! Opt-in test: only the saved localhost zenbase docker connection is allowed.
//! Real tables are read-only. DDL/data fixtures live in a rolled-back temp table.
use std::{
    error::Error,
    io::{Read, Write},
    net::{Shutdown, TcpListener, TcpStream},
    sync::{Arc, Mutex, atomic::AtomicBool},
    thread,
};

use catabase_core::{
    ConnectionFields, MetadataSource, QueryResult, ReflectionCache, RowUpdate, apply_row_updates,
    connect_client, execute_on_client_with_reflection,
};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;

#[derive(Clone, Debug, Default, PartialEq)]
struct Requests {
    queries: usize,
    columns: usize,
    keys: usize,
}

type CountedConnection = (ConnectionFields, Arc<Mutex<Requests>>);

fn assert_timing_totals(result: &QueryResult) {
    let timing = result
        .timings
        .as_ref()
        .expect("new results include timings");
    let sum = timing.prepare_ms + timing.metadata_ms + timing.execute_fetch_ms + timing.decode_ms;
    assert!((sum - timing.total_ms).abs() < 0.000_001);
    assert_eq!(result.duration_ms, timing.total_ms as u128);
    for ms in [
        timing.prepare_ms,
        timing.metadata_ms,
        timing.execute_fetch_ms,
        timing.decode_ms,
    ] {
        assert!(ms.is_finite() && ms >= 0.0);
    }
}

fn docker_fields() -> Result<ConnectionFields, Box<dyn Error>> {
    let path = std::env::var("CATABASE_TEST_CONNECTION_STORE")?;
    let storage = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let fields = storage.query_row(
        "SELECT name,host,port,database,username,password_fallback,ssl FROM connections WHERE name = 'zenbase docker'",
        [], |row| Ok(ConnectionFields { name: row.get(0)?, host: row.get(1)?, port: row.get(2)?, database: row.get(3)?, username: row.get(4)?, password: row.get(5)?, ssl: row.get(6)? }))?;
    assert_eq!(fields.name, "zenbase docker");
    assert!(matches!(fields.host.as_str(), "localhost" | "127.0.0.1"));
    assert_eq!(fields.port, 5432);
    assert_eq!(fields.database, "zenbase");
    assert!(!fields.ssl);
    assert!(
        fields.password.is_some(),
        "Only the saved docker fallback credential may be used."
    );
    Ok(fields)
}

// Count Parse messages without storing SQL, credentials or row payloads.
// This verifies absence of round trips, not a timing threshold that could flake.
fn counted_connection(fields: &ConnectionFields) -> Result<CountedConnection, Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let mut proxied = fields.clone();
    proxied.host = "127.0.0.1".into();
    proxied.port = listener.local_addr()?.port();
    let endpoint = (fields.host.clone(), fields.port);
    let requests = Arc::new(Mutex::new(Requests::default()));
    let counted = requests.clone();
    thread::spawn(move || {
        let Ok((mut downstream, _)) = listener.accept() else {
            return;
        };
        let Ok(mut upstream) = TcpStream::connect(endpoint) else {
            return;
        };
        let _ = downstream.set_nodelay(true);
        let _ = upstream.set_nodelay(true);
        let mut back = upstream.try_clone().unwrap();
        let mut destination = downstream.try_clone().unwrap();
        thread::spawn(move || {
            let _ = std::io::copy(&mut back, &mut destination);
            let _ = destination.shutdown(Shutdown::Write);
        });
        let mut startup = [0; 4];
        if downstream.read_exact(&mut startup).is_err() {
            return;
        }
        let length = u32::from_be_bytes(startup) as usize;
        let mut packet = vec![0; length];
        packet[..4].copy_from_slice(&startup);
        if downstream.read_exact(&mut packet[4..]).is_err() || upstream.write_all(&packet).is_err()
        {
            return;
        }
        loop {
            let mut header = [0; 5];
            if downstream.read_exact(&mut header).is_err() {
                break;
            }
            let length = u32::from_be_bytes(header[1..].try_into().unwrap()) as usize;
            if !(4..=16 * 1024 * 1024).contains(&length) {
                break;
            }
            let mut packet = vec![0; length + 1];
            packet[..5].copy_from_slice(&header);
            if downstream.read_exact(&mut packet[5..]).is_err() {
                break;
            }
            if header[0] == b'P' {
                let parts = packet[5..].split(|byte| *byte == 0).collect::<Vec<_>>();
                let sql = String::from_utf8_lossy(parts.get(1).copied().unwrap_or_default());
                let mut counts = counted.lock().unwrap();
                counts.queries += 1;
                if sql.contains("FROM pg_class c") && sql.contains("JOIN pg_attribute a") {
                    counts.columns += 1;
                }
                if sql.contains("array_agg(a.attname::text ORDER BY k.position)") {
                    counts.keys += 1;
                }
            }
            if upstream.write_all(&packet).is_err() {
                break;
            }
        }
        let _ = upstream.shutdown(Shutdown::Write);
    });
    Ok((proxied, requests))
}

#[tokio::test]
#[ignore = "requires explicit CATABASE_TEST_CONNECTION_STORE pointing at the saved zenbase docker connection"]
async fn reflected_queries_have_no_metadata_round_trips_and_writes_still_revalidate()
-> Result<(), Box<dyn Error>> {
    let (fields, requests) = counted_connection(&docker_fields()?)?;
    let client = connect_client(&fields).await?;
    let cache = ReflectionCache::default();
    let tree = cache.refresh(&client).await?;
    let templates = tree
        .schemas
        .iter()
        .find(|item| item.schema == "public" && item.name == "notification_templates")
        .unwrap();
    assert!(templates.table_oid > 0);
    assert!(
        templates
            .columns
            .iter()
            .all(|column| column.column_id > 0 && column.type_oid > 0)
    );
    assert!(
        templates
            .row_keys
            .iter()
            .any(|key| key.primary_key && key.columns == ["id"])
    );

    client.simple_query("SELECT 1").await?;
    *requests.lock().unwrap() = Requests::default();
    let sql = "SELECT * FROM public.notification_templates WHERE type = 2";
    let warm = execute_on_client_with_reflection(&client, sql, 0, 200, None, &cache).await?;
    assert_timing_totals(&warm);
    assert_eq!(
        warm.timings.as_ref().unwrap().metadata_source,
        MetadataSource::Cache
    );
    assert_eq!(warm.timings.as_ref().unwrap().metadata_queries, 0);
    assert_eq!(
        *requests.lock().unwrap(),
        Requests {
            queries: 1,
            columns: 0,
            keys: 0
        }
    );
    assert!(
        warm.columns
            .iter()
            .all(|column| column.source_name.is_some())
    );
    assert!(
        warm.row_keys
            .iter()
            .any(|key| key.primary_key && key.columns == ["id"])
    );
    println!(
        "Warm reflection: {} rows, {} ms, zero metadata requests",
        warm.row_count, warm.duration_ms
    );
    let timing = warm.timings.as_ref().unwrap();
    println!(
        "Query spans: prepare {:.3} ms, metadata {:.3} ms, execute/fetch {:.3} ms, decode {:.3} ms; timing payload {} bytes",
        timing.prepare_ms,
        timing.metadata_ms,
        timing.execute_fetch_ms,
        timing.decode_ms,
        serde_json::to_vec(timing)?.len()
    );

    let aliases = execute_on_client_with_reflection(&client, "SELECT n.id AS renamed_key, n.tag AS label, upper(n.name) AS computed FROM public.notification_templates n WHERE type = 2", 0, 200, None, &cache).await?;
    assert_eq!(aliases.columns[0].source_name.as_deref(), Some("id"));
    assert_eq!(aliases.columns[0].name, "renamed_key");
    assert_eq!(aliases.columns[1].source_name.as_deref(), Some("tag"));
    assert!(aliases.columns[2].source_name.is_none());
    let joined = execute_on_client_with_reflection(&client, "SELECT n.id AS template_id, n.name AS template_name, l.id AS landlord_id, l.name AS landlord_name FROM public.notification_templates n CROSS JOIN public.landlords l LIMIT 1", 0, 200, None, &cache).await?;
    assert_eq!(
        joined.columns[0].table_name.as_deref(),
        Some("notification_templates")
    );
    assert_eq!(joined.columns[2].table_name.as_deref(), Some("landlords"));
    assert_eq!(requests.lock().unwrap().columns, 0);
    assert_eq!(requests.lock().unwrap().keys, 0);

    client.simple_query("SELECT 1").await?;
    *requests.lock().unwrap() = Requests::default();
    let cold =
        execute_on_client_with_reflection(&client, sql, 0, 200, None, &ReflectionCache::default())
            .await?;
    assert_timing_totals(&cold);
    assert_eq!(
        cold.timings.as_ref().unwrap().metadata_source,
        MetadataSource::Refreshed
    );
    assert_eq!(cold.timings.as_ref().unwrap().metadata_queries, 2);
    assert_eq!(
        *requests.lock().unwrap(),
        Requests {
            queries: 3,
            columns: 1,
            keys: 1
        }
    );
    assert_eq!(
        serde_json::to_value(&warm.columns)?,
        serde_json::to_value(&cold.columns)?
    );
    assert_eq!(
        serde_json::to_value(&warm.row_keys)?,
        serde_json::to_value(&cold.row_keys)?
    );
    assert_eq!(warm.rows, cold.rows);

    let delayed =
        execute_on_client_with_reflection(&client, "SELECT pg_sleep(0.05)", 0, 200, None, &cache)
            .await?;
    assert_timing_totals(&delayed);
    let timing = delayed.timings.as_ref().unwrap();
    assert_eq!(timing.metadata_source, MetadataSource::NotNeeded);
    assert_eq!(timing.metadata_queries, 0);
    assert!(
        timing.execute_fetch_ms >= 45.0,
        "server delay belongs to execute/fetch, not decoding"
    );
    let batch =
        execute_on_client_with_reflection(&client, "SELECT 1; SELECT 2", 0, 200, None, &cache)
            .await?;
    assert_timing_totals(&batch);
    let timing = batch.timings.as_ref().unwrap();
    assert_eq!(timing.prepare_ms, 0.0);
    assert_eq!(timing.metadata_ms, 0.0);
    assert_eq!(timing.metadata_source, MetadataSource::NotNeeded);

    let cancelled = AtomicBool::new(true);
    let error = execute_on_client_with_reflection(&client, sql, 0, 200, Some(&cancelled), &cache)
        .await
        .unwrap_err();
    assert_eq!(error.pg_info().unwrap().code.as_deref(), Some("57014"));

    // Dedicated transaction cache; every mutation below affects only pg_temp.
    let private = ReflectionCache::default();
    client.batch_execute("BEGIN").await?;
    let create = "CREATE TEMP TABLE catabase_reflection_probe (tenant int NOT NULL, id bigint NOT NULL, code text UNIQUE, label text, computed text GENERATED ALWAYS AS (upper(label)) STORED, CONSTRAINT reflection_probe_pk PRIMARY KEY(tenant,id))";
    execute_on_client_with_reflection(&client, create, 0, 200, None, &private).await?;
    client.batch_execute("CREATE UNIQUE INDEX reflection_probe_partial ON catabase_reflection_probe(id) WHERE id > 0; CREATE UNIQUE INDEX reflection_probe_expression ON catabase_reflection_probe(lower(code)); CREATE UNIQUE INDEX reflection_probe_nondefault ON catabase_reflection_probe(label text_pattern_ops); INSERT INTO catabase_reflection_probe(tenant,id,code,label) VALUES (1,9007199254740993,'a','first'),(2,2,'b','second')").await?;
    let version: i32 = client
        .query_one("SELECT current_setting('server_version_num')::int", &[])
        .await?
        .get(0);
    if version >= 150000 {
        client
            .batch_execute(
                "ALTER TABLE catabase_reflection_probe ADD UNIQUE NULLS NOT DISTINCT (label)",
            )
            .await?;
    }
    let fixture = execute_on_client_with_reflection(&client, "SELECT tenant AS tenant_alias, id AS key_alias, code, label, computed FROM catabase_reflection_probe", 0, 200, None, &private).await?;
    assert_eq!(fixture.rows[0]["key_alias"], "9007199254740993");
    assert!(fixture.columns[4].generated);
    assert!(
        fixture
            .row_keys
            .iter()
            .any(|key| key.primary_key && key.columns == ["tenant", "id"])
    );
    assert!(
        fixture
            .row_keys
            .iter()
            .any(|key| !key.primary_key && key.columns == ["code"])
    );
    assert!(!fixture.row_keys.iter().any(|key| key.columns == ["id"]));
    if version >= 150000 {
        assert!(
            fixture
                .row_keys
                .iter()
                .any(|key| key.columns == ["label"] && key.nulls_not_distinct)
        );
    }

    execute_on_client_with_reflection(
        &client,
        "ALTER TABLE catabase_reflection_probe RENAME COLUMN code TO renamed_code",
        0,
        200,
        None,
        &private,
    )
    .await?;
    let renamed = execute_on_client_with_reflection(
        &client,
        "SELECT renamed_code AS code FROM catabase_reflection_probe",
        0,
        200,
        None,
        &private,
    )
    .await?;
    assert_eq!(
        renamed.columns[0].source_name.as_deref(),
        Some("renamed_code")
    );
    execute_on_client_with_reflection(
        &client,
        "SAVEPOINT reflection_probe_savepoint",
        0,
        200,
        None,
        &private,
    )
    .await?;
    execute_on_client_with_reflection(
        &client,
        "ALTER TABLE catabase_reflection_probe RENAME COLUMN label TO new_label",
        0,
        200,
        None,
        &private,
    )
    .await?;
    let changed = execute_on_client_with_reflection(
        &client,
        "SELECT new_label FROM catabase_reflection_probe",
        0,
        200,
        None,
        &private,
    )
    .await?;
    assert_eq!(changed.columns[0].source_name.as_deref(), Some("new_label"));
    execute_on_client_with_reflection(
        &client,
        "ROLLBACK TO SAVEPOINT reflection_probe_savepoint",
        0,
        200,
        None,
        &private,
    )
    .await?;
    let restored = execute_on_client_with_reflection(
        &client,
        "SELECT label FROM catabase_reflection_probe",
        0,
        200,
        None,
        &private,
    )
    .await?;
    assert_eq!(restored.columns[0].source_name.as_deref(), Some("label"));

    // A stale reflected key must never authorize an UPDATE after its removal.
    client
        .batch_execute("ALTER TABLE catabase_reflection_probe DROP CONSTRAINT reflection_probe_pk")
        .await?;
    let update = RowUpdate {
        schema: fixture.columns[0].table_schema.clone().unwrap(),
        table: "catabase_reflection_probe".into(),
        key_columns: vec!["tenant".into(), "id".into()],
        values: json!({"label":"must not write"})
            .as_object()
            .unwrap()
            .clone(),
        original: json!({"tenant":1,"id":"9007199254740993","label":"first"})
            .as_object()
            .unwrap()
            .clone(),
    };
    let rejected = apply_row_updates(&client, &[update], true, &AtomicBool::new(false))
        .await
        .unwrap_err();
    assert!(
        rejected
            .to_string()
            .contains("complete primary key or usable unique key")
    );
    let unchanged: String = client
        .query_one(
            "SELECT label FROM catabase_reflection_probe WHERE tenant=1",
            &[],
        )
        .await?
        .get(0);
    assert_eq!(unchanged, "first");
    execute_on_client_with_reflection(&client, "ROLLBACK", 0, 200, None, &private).await?;

    client.simple_query("SELECT 1").await?;
    *requests.lock().unwrap() = Requests::default();
    execute_on_client_with_reflection(&client, sql, 0, 200, None, &cache).await?;
    assert_eq!(
        *requests.lock().unwrap(),
        Requests {
            queries: 1,
            columns: 0,
            keys: 0
        }
    );
    Ok(())
}
