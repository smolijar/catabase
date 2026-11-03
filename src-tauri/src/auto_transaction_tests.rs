//! Explicitly opted-in native transaction tests. Only the saved localhost
//! `zenbase docker` connection is permitted. All writes use session-local
//! temporary tables; the application's storage is opened read-only.
use super::*;

use std::error::Error;

use rusqlite::{Connection, OpenFlags};
use serde_json::json;

const FIXTURE: &str = "catabase_auto_transaction_probe";

fn docker_fields() -> Result<ConnectionFields, Box<dyn Error>> {
    let path = std::env::var("CATABASE_TEST_CONNECTION_STORE")?;
    let storage = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let fields = storage.query_row(
        "SELECT name,host,port,database,username,password_fallback,ssl
         FROM connections WHERE name = 'zenbase docker'",
        [],
        |row| {
            Ok(ConnectionFields {
                name: row.get(0)?,
                host: row.get(1)?,
                port: row.get(2)?,
                database: row.get(3)?,
                username: row.get(4)?,
                password: row.get(5)?,
                ssl: row.get(6)?,
                options: Default::default(),
            })
        },
    )?;
    assert_eq!(fields.name, "zenbase docker");
    assert!(matches!(fields.host.as_str(), "localhost" | "127.0.0.1"));
    assert_eq!(fields.port, 5432);
    assert_eq!(fields.database, "zenbase");
    assert!(!fields.ssl);
    assert!(
        fields.password.is_some(),
        "Use only the saved Docker credential."
    );
    Ok(fields)
}

async fn fixture_pool() -> Result<(ConnectionRecord, PgPool), Box<dyn Error>> {
    let fields = docker_fields()?;
    let pool = create_pool(&fields, 1)?;
    let client = checkout_pool_client(&pool).await?;
    client
        .batch_execute(
            "CREATE TEMP TABLE catabase_auto_transaction_probe
                 (id int PRIMARY KEY, value int NOT NULL, note text NOT NULL);
             INSERT INTO catabase_auto_transaction_probe VALUES (1, 10, 'before')",
        )
        .await?;
    drop(client);
    let record = ConnectionRecord {
        id: "docker-auto-transaction-test".into(),
        name: fields.name,
        host: fields.host,
        port: fields.port,
        database: fields.database,
        username: fields.username,
        password: fields.password,
        ssl: fields.ssl,
        options: fields.options,
        is_active: true,
        created_at: String::new(),
        updated_at: String::new(),
    };
    Ok((record, pool))
}

fn query_run<'a>(
    history: &'a StatementHistory<'a>,
    active: &'a ConnectionRecord,
    pool: &'a PgPool,
    reflection: &'a ReflectionCache,
    sql: &'a str,
) -> QueryRun<'a> {
    QueryRun {
        history,
        active,
        pool,
        reflection,
        ssl: false,
        sql,
        query_token: "auto-transaction-test",
        offset: 0,
        page_size: 200,
        read_only_replay: false,
    }
}

async fn execute_held(
    session: &TxSession,
    history: &StatementHistory<'_>,
    cancellation: &QueryCancellation,
    sql: &str,
) -> Result<QueryResult, CoreError> {
    cancellation
        .run(
            &session.client,
            session.ssl,
            execute_on_client_observed(
                &session.client,
                sql,
                0,
                200,
                Some(cancellation.flag()),
                &session.reflection,
                Some(history),
            ),
        )
        .await
}

#[tokio::test]
#[ignore = "requires explicit CATABASE_TEST_CONNECTION_STORE for the saved zenbase docker connection"]
async fn automatic_begin_keeps_cte_writes_uncommitted_and_records_each_statement()
-> Result<(), Box<dyn Error>> {
    let (active, pool) = fixture_pool().await?;
    let storage = Storage::in_memory()?;
    let notify = |saved: Result<(), CoreError>| saved.unwrap();
    let history = StatementHistory::new(&storage, &active.id, &notify);
    let reflection = ReflectionCache::default();
    let sql = "WITH source AS (SELECT id FROM catabase_auto_transaction_probe),
               changed AS (UPDATE catabase_auto_transaction_probe SET value = value + 1
                   WHERE id IN (SELECT id FROM source) RETURNING id, value)
               SELECT * FROM changed";
    let run = query_run(&history, &active, &pool, &reflection, sql);
    let cancellation = QueryCancellation::default();
    let (session, _) =
        begin_transaction_session(&storage, &run, "BEGIN", true, &cancellation).await?;
    assert!(session.automatic);
    let transaction_id = session.history_id.clone();
    let entries = storage.history_transaction_page(&transaction_id, 0, 50)?;
    assert_eq!(entries.total, 1);
    assert_eq!(entries.entries[0].query, "BEGIN");
    assert_eq!(
        entries.entries[0].transaction.as_ref().unwrap().kind,
        "auto"
    );
    assert_eq!(
        entries.entries[0].transaction.as_ref().unwrap().status,
        "active"
    );
    assert_eq!(
        pool.status().available,
        0,
        "The transaction holds its pool client."
    );

    let result = execute_held(&session, &history, &cancellation, sql).await?;
    assert!(
        !result.auto_limited,
        "Writing CTEs must never be paged/replayed."
    );
    assert_eq!(result.rows[0]["value"], json!(11));
    assert_eq!(
        storage
            .history_transaction_page(&transaction_id, 0, 50)?
            .total,
        2
    );
    let read = execute_held(
        &session,
        &history,
        &cancellation,
        "SELECT value FROM catabase_auto_transaction_probe",
    )
    .await?;
    assert_eq!(read.rows[0]["value"], json!(11));
    assert_eq!(
        storage
            .history_transaction_page(&transaction_id, 0, 50)?
            .total,
        3
    );

    execute_held(&session, &history, &cancellation, "ROLLBACK").await?;
    storage.finish_history_transaction(&transaction_id, "rolledBack")?;
    let restored: i32 = session
        .client
        .query_one("SELECT value FROM catabase_auto_transaction_probe", &[])
        .await?
        .get(0);
    assert_eq!(
        restored, 10,
        "No implicit commit may occur after an automatic BEGIN."
    );
    let entries = storage.history_transaction_page(&transaction_id, 0, 50)?;
    assert_eq!(entries.total, 4);
    assert!(entries.entries.iter().all(|entry| {
        let transaction = entry.transaction.as_ref().unwrap();
        transaction.id == transaction_id && transaction.status == "rolledBack"
    }));
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicit CATABASE_TEST_CONNECTION_STORE for the saved zenbase docker connection"]
async fn reviewed_edits_use_savepoints_inside_automatic_transaction_until_commit()
-> Result<(), Box<dyn Error>> {
    let (active, pool) = fixture_pool().await?;
    let storage = Storage::in_memory()?;
    let notify = |saved: Result<(), CoreError>| saved.unwrap();
    let history = StatementHistory::new(&storage, &active.id, &notify);
    let reflection = ReflectionCache::default();
    let run = query_run(&history, &active, &pool, &reflection, "UPDATE");
    let cancellation = QueryCancellation::default();
    let (session, _) =
        begin_transaction_session(&storage, &run, "BEGIN", true, &cancellation).await?;
    let original = execute_held(
        &session,
        &history,
        &cancellation,
        "SELECT * FROM catabase_auto_transaction_probe",
    )
    .await?;
    let update = RowUpdate {
        schema: original.columns[0].table_schema.clone().unwrap(),
        table: FIXTURE.into(),
        key_columns: vec!["id".into()],
        original: original.rows[0].as_object().unwrap().clone(),
        values: json!({"note": "reviewed"}).as_object().unwrap().clone(),
    };
    let result = cancellation
        .run(
            &session.client,
            false,
            apply_rows(
                &session.client,
                &[update],
                true,
                cancellation.flag(),
                Some(&history),
            ),
        )
        .await?;
    assert!(result.in_transaction);
    let entries = storage.history_transaction_page(&session.history_id, 0, 50)?;
    assert_eq!(entries.total, 5);
    assert!(entries.entries[2].query.starts_with("SAVEPOINT "));
    assert!(entries.entries[3].query.starts_with("UPDATE "));
    assert!(entries.entries[4].query.starts_with("RELEASE SAVEPOINT "));
    assert!(entries.entries.iter().all(|entry| entry.query != "COMMIT"));

    execute_held(&session, &history, &cancellation, "COMMIT").await?;
    storage.finish_history_transaction(&session.history_id, "finished")?;
    let committed: String = session
        .client
        .query_one("SELECT note FROM catabase_auto_transaction_probe", &[])
        .await?
        .get(0);
    assert_eq!(committed, "reviewed");
    assert_eq!(
        storage
            .history_transaction_page(&session.history_id, 0, 50)?
            .total,
        6
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicit CATABASE_TEST_CONNECTION_STORE for the saved zenbase docker connection"]
async fn same_cancellation_signal_prevents_write_after_automatic_begin()
-> Result<(), Box<dyn Error>> {
    let (active, pool) = fixture_pool().await?;
    let storage = Storage::in_memory()?;
    let notify = |saved: Result<(), CoreError>| saved.unwrap();
    let history = StatementHistory::new(&storage, &active.id, &notify);
    let reflection = ReflectionCache::default();
    let sql = "UPDATE catabase_auto_transaction_probe SET value = 99";
    let run = query_run(&history, &active, &pool, &reflection, sql);
    let cancellation = QueryCancellation::default();
    let (session, _) =
        begin_transaction_session(&storage, &run, "BEGIN", true, &cancellation).await?;
    cancellation.request();
    assert!(matches!(
        execute_held(&session, &history, &cancellation, sql).await,
        Err(CoreError::Cancelled(_))
    ));
    let unchanged: i32 = session
        .client
        .query_one("SELECT value FROM catabase_auto_transaction_probe", &[])
        .await?
        .get(0);
    assert_eq!(unchanged, 10);
    assert_eq!(
        storage
            .history_transaction_page(&session.history_id, 0, 50)?
            .total,
        1
    );
    session.client.batch_execute("ROLLBACK").await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicit CATABASE_TEST_CONNECTION_STORE for the saved zenbase docker connection"]
async fn failed_begin_detaches_its_client_and_records_interrupted_transaction()
-> Result<(), Box<dyn Error>> {
    let (active, pool) = fixture_pool().await?;
    let previous = checkout_pool_client(&pool).await?;
    let previous_pid: i32 = previous
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    drop(previous);
    let storage = Storage::in_memory()?;
    let notify = |saved: Result<(), CoreError>| saved.unwrap();
    let history = StatementHistory::new(&storage, &active.id, &notify);
    let reflection = ReflectionCache::default();
    let sql = "BEGIN ISOLATION LEVEL invalid_isolation";
    let run = query_run(&history, &active, &pool, &reflection, sql);
    let cancellation = QueryCancellation::default();
    assert!(
        begin_transaction_session(&storage, &run, sql, false, &cancellation)
            .await
            .is_err()
    );
    let entries = storage.history_page(None, 0, 50)?;
    assert_eq!(entries.total, 1);
    assert!(entries.entries[0].is_error);
    assert_eq!(entries.entries[0].query, sql);
    assert_eq!(
        entries.entries[0].transaction.as_ref().unwrap().status,
        "interrupted"
    );
    let fresh = checkout_pool_client(&pool).await?;
    let fresh_pid: i32 = fresh
        .query_one("SELECT pg_backend_pid()", &[])
        .await?
        .get(0);
    assert_ne!(
        previous_pid, fresh_pid,
        "A failed BEGIN must not recycle its client."
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicit CATABASE_TEST_CONNECTION_STORE for the saved zenbase docker connection"]
async fn auto_transaction_policy_routes_reads_writes_and_existing_sessions()
-> Result<(), Box<dyn Error>> {
    let (active, pool) = fixture_pool().await?;
    let storage = Storage::in_memory()?;
    let notify = |saved: Result<(), CoreError>| saved.unwrap();
    let history = StatementHistory::new(&storage, &active.id, &notify);
    let reflection = ReflectionCache::default();
    let cancellation = QueryCancellation::default();
    let mut tx = None;

    for sql in [
        "SELECT * FROM catabase_auto_transaction_probe",
        "WITH outer_read AS (WITH inner_read AS (SELECT 1 AS value)
             SELECT * FROM inner_read) SELECT * FROM outer_read",
    ] {
        let run = query_run(&history, &active, &pool, &reflection, sql);
        assert!(!maybe_begin_automatic_transaction(&storage, &mut tx, &run, &cancellation).await?);
        assert!(tx.is_none());
        assert_eq!(pool.status().available, 1);
        assert_eq!(
            storage.history_page(None, 0, 50)?.total,
            0,
            "Read routing must not execute BEGIN."
        );
    }

    let sql = "WITH changed AS (UPDATE catabase_auto_transaction_probe
                   SET value = 21 RETURNING *) SELECT * FROM changed";
    let run = query_run(&history, &active, &pool, &reflection, sql);
    storage.set_auto_transaction(false)?;
    assert!(!maybe_begin_automatic_transaction(&storage, &mut tx, &run, &cancellation).await?);
    assert!(
        tx.is_none(),
        "Explicit opt-out leaves writes on the standalone path."
    );
    storage.set_auto_transaction(true)?;
    assert!(maybe_begin_automatic_transaction(&storage, &mut tx, &run, &cancellation).await?);
    let auto_id = tx.as_ref().unwrap().history_id.clone();
    assert!(transaction_status(tx.as_ref()).automatic);
    let result = execute_held(tx.as_ref().unwrap(), &history, &cancellation, sql).await?;
    assert_eq!(result.rows[0]["value"], json!(21));
    assert!(!maybe_begin_automatic_transaction(&storage, &mut tx, &run, &cancellation).await?);
    assert_eq!(tx.as_ref().unwrap().history_id, auto_id);
    assert_eq!(
        storage.history_transaction_page(&auto_id, 0, 50)?.total,
        2,
        "An existing transaction must not receive another BEGIN."
    );

    storage.set_auto_transaction(false)?;
    assert!(!maybe_begin_automatic_transaction(&storage, &mut tx, &run, &cancellation).await?);
    assert!(transaction_status(tx.as_ref()).active);
    assert!(transaction_status(tx.as_ref()).automatic);
    assert_eq!(tx.as_ref().unwrap().history_id, auto_id);
    execute_held(tx.as_ref().unwrap(), &history, &cancellation, "ROLLBACK").await?;
    storage.finish_history_transaction(&auto_id, "rolledBack")?;
    let restored: i32 = tx
        .as_ref()
        .unwrap()
        .client
        .query_one("SELECT value FROM catabase_auto_transaction_probe", &[])
        .await?
        .get(0);
    assert_eq!(
        restored, 10,
        "Disabling the mode must not commit existing work."
    );
    drop(tx.take());

    let (manual, _) =
        begin_transaction_session(&storage, &run, "BEGIN", false, &cancellation).await?;
    let manual_id = manual.history_id.clone();
    tx = Some(manual);
    storage.set_auto_transaction(true)?;
    assert!(!maybe_begin_automatic_transaction(&storage, &mut tx, &run, &cancellation).await?);
    assert_eq!(tx.as_ref().unwrap().history_id, manual_id);
    assert!(!transaction_status(tx.as_ref()).automatic);
    assert_eq!(
        storage.history_transaction_page(&manual_id, 0, 50)?.total,
        1
    );
    execute_held(tx.as_ref().unwrap(), &history, &cancellation, "ROLLBACK").await?;
    Ok(())
}
