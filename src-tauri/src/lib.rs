use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use catabase_core::{
    AiCompletionRequest, AiHealth, ApplyUpdatesResult, CommandTimings, ConnectionFields,
    ConnectionInput, ConnectionRecord, ConnectionStats, CoreError, ForeignKeyLookup,
    ForeignKeyValues, HistoryPage, HistoryStats, NewHistoryEntry, OpenCodeClient, PgCancelToken,
    PgClient, PgErrorInfo, PgPool, PooledPgClient, QueryResult, ReflectionCache, RowUpdate,
    SchemaTree, Settings, StatementKind, StatementRange, Storage, apply_row_updates as apply_rows,
    build_connection_url as build_url, cancel_query_token, checkout_pool_client,
    classify_statement, connect_client, connection_stats as load_connection_stats, create_pool,
    execute_on_client, execute_on_client_with_reflection, get_ddl_on_client as load_ddl,
    lookup_foreign_key_values as lookup_fk_values, parse_connection_url as parse_url,
    split_editor_statements, split_statements, test_connection as test_pg_connection,
    validate_transaction_sql,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{Mutex, RwLock};

struct AppState {
    storage: Storage,
    active: RwLock<Option<ConnectionRecord>>,
    pool: RwLock<Option<ActivePool>>,
    tx: Mutex<Option<TxSession>>,
    cancels: Mutex<HashMap<String, CancelEntry>>,
    opencode: OpenCodeClient,
    exit_guard_enabled: AtomicBool,
    exit_confirmed: AtomicBool,
}

#[derive(Clone)]
struct ActivePool {
    connection_id: String,
    pool_size: u32,
    pool: PgPool,
    reflection: Arc<ReflectionCache>,
}

struct TxSession {
    connection_id: String,
    client: PooledPgClient,
    ssl: bool,
    started_at: String,
    reflection: Arc<ReflectionCache>,
}

struct CancelEntry {
    token: Option<PgCancelToken>,
    ssl: bool,
    cancelled: Arc<AtomicBool>,
    created_at: Instant,
}

impl CancelEntry {
    fn pending() -> Self {
        Self {
            token: None,
            ssl: false,
            cancelled: Arc::new(AtomicBool::new(false)),
            created_at: Instant::now(),
        }
    }
}

struct QueryRun<'a> {
    active: &'a ConnectionRecord,
    pool: &'a PgPool,
    reflection: &'a ReflectionCache,
    ssl: bool,
    sql: &'a str,
    query_token: &'a str,
    offset: usize,
    page_size: usize,
    read_only_replay: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppError {
    message: String,
    severity: Option<String>,
    detail: Option<String>,
    hint: Option<String>,
    code: Option<String>,
    position: Option<usize>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TxStatus {
    active: bool,
    started_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CancelResult {
    cancelled: bool,
    message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HistoryResult {
    result: Option<serde_json::Value>,
}

impl From<CoreError> for AppError {
    fn from(error: CoreError) -> Self {
        if let Some(PgErrorInfo {
            message,
            severity,
            detail,
            hint,
            code,
            position,
        }) = error.pg_info()
        {
            return Self {
                message,
                severity,
                detail,
                hint,
                code,
                position,
            };
        }

        Self {
            message: error.to_string(),
            severity: None,
            detail: None,
            hint: None,
            code: None,
            position: None,
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(error: std::io::Error) -> Self {
        CoreError::from(error).into()
    }
}

#[tauri::command]
async fn list_connections(state: State<'_, AppState>) -> Result<Vec<ConnectionRecord>, AppError> {
    state.storage.list_connections().map_err(Into::into)
}

#[tauri::command]
async fn save_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    input: ConnectionInput,
) -> Result<ConnectionRecord, AppError> {
    let _tx_guard = ensure_no_transaction(&state).await?;
    let record = state
        .storage
        .save_connection(input)
        .map_err(AppError::from)?;
    refresh_active(&state).await?;
    reset_pool(&state).await;
    emit_pool_reset(&app);
    Ok(record)
}

#[tauri::command]
async fn duplicate_connection(
    state: State<'_, AppState>,
    id: String,
) -> Result<ConnectionRecord, AppError> {
    // A duplicate is inactive configuration only. The existing pool, checked-out
    // transaction and active connection must not be reset by creating it.
    state.storage.duplicate_connection(&id).map_err(Into::into)
}

#[tauri::command]
async fn delete_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let _tx_guard = ensure_no_transaction(&state).await?;
    state
        .storage
        .delete_connection(&id)
        .map_err(AppError::from)?;
    refresh_active(&state).await?;
    reset_pool(&state).await;
    emit_pool_reset(&app);
    Ok(())
}

#[tauri::command]
async fn activate_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<ConnectionRecord, AppError> {
    let _tx_guard = ensure_no_transaction(&state).await?;
    let record = state
        .storage
        .activate_connection(&id)
        .map_err(AppError::from)?;
    *state.active.write().await = Some(record.clone());
    reset_pool(&state).await;
    emit_pool_reset(&app);
    Ok(record)
}

#[tauri::command]
async fn test_connection(input: ConnectionInput) -> Result<(), AppError> {
    test_pg_connection(&ConnectionFields::from(&input))
        .await
        .map_err(Into::into)
}

#[tauri::command]
async fn parse_connection_url(input: String) -> Result<ConnectionFields, AppError> {
    parse_url(&input).map_err(Into::into)
}

#[tauri::command]
async fn build_connection_url(fields: ConnectionFields) -> Result<String, AppError> {
    build_url(&fields).map_err(Into::into)
}

#[tauri::command]
async fn get_settings(state: State<'_, AppState>) -> Result<Settings, AppError> {
    state.storage.get_settings().map_err(Into::into)
}

#[tauri::command]
async fn set_pool_size(
    app: AppHandle,
    state: State<'_, AppState>,
    pool_size: u32,
) -> Result<u32, AppError> {
    let _tx_guard = ensure_no_transaction(&state).await?;
    let normalized = state
        .storage
        .set_pool_size(pool_size)
        .map_err(AppError::from)?;
    reset_pool(&state).await;
    emit_pool_reset(&app);
    Ok(normalized)
}

#[tauri::command]
async fn set_ai_model(
    state: State<'_, AppState>,
    model: Option<String>,
) -> Result<Option<String>, AppError> {
    state.storage.set_ai_model(model).map_err(Into::into)
}

#[tauri::command]
async fn split_sql(sql: String) -> Result<Vec<StatementRange>, AppError> {
    Ok(split_editor_statements(&sql)
        .into_iter()
        .map(|range| StatementRange {
            // Monaco offsets are UTF-16 units; the lexer uses UTF-8 bytes.
            start: sql[..range.start].encode_utf16().count(),
            end: sql[..range.end].encode_utf16().count(),
            text: range.text,
        })
        .collect())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)] // Tauri command arguments are the public IPC shape.
async fn execute_query(
    app: AppHandle,
    state: State<'_, AppState>,
    sql: String,
    query_token: String,
    offset: u32,
    page_size: u32,
    connection_id: Option<String>,
    read_only_replay: Option<bool>,
) -> Result<QueryResult, AppError> {
    let command_start = Instant::now();
    begin_operation(&state, &query_token).await;
    let outcome = async {
        let active = requested_connection(&state, connection_id.as_deref()).await?;
        ensure_transaction_connection(&state, &active.id).await?;
        let pool = active_pool(&state, &active).await?;
        let result = run_query_with_state(
            &app,
            &state,
            QueryRun {
                active: &active,
                pool: &pool.pool,
                reflection: &pool.reflection,
                ssl: active.ssl,
                sql: &sql,
                query_token: &query_token,
                offset: offset as usize,
                page_size: page_size as usize,
                read_only_replay: read_only_replay.unwrap_or(false),
            },
        )
        .await;

        match result {
            Ok(mut result) => {
                let before_history_ms = command_start.elapsed().as_secs_f64() * 1000.0;
                let _ = state.storage.insert_history(NewHistoryEntry {
                    connection_id: Some(active.id),
                    query: sql,
                    duration_ms: result.duration_ms,
                    row_count: Some(result.row_count),
                    is_error: false,
                    error_message: None,
                    result_json: serde_json::to_value(&result).ok(),
                });
                let after_history_ms = command_start.elapsed().as_secs_f64() * 1000.0;
                if let Some(timings) = result.timings.as_mut() {
                    timings.command = Some(CommandTimings {
                        setup_ms: (before_history_ms - timings.total_ms).max(0.0),
                        history_ms: after_history_ms - before_history_ms,
                        total_ms: after_history_ms,
                    });
                }
                Ok(result)
            }
            Err(error) => {
                let app_error = AppError::from(error);
                let _ = state.storage.insert_history(NewHistoryEntry {
                    connection_id: Some(active.id),
                    query: sql,
                    duration_ms: 0,
                    row_count: None,
                    is_error: true,
                    error_message: Some(app_error.message.clone()),
                    result_json: None,
                });
                Err(app_error)
            }
        }
    }
    .await;
    state.cancels.lock().await.remove(&query_token);
    outcome.map(|mut result| {
        if let Some(command) = result
            .timings
            .as_mut()
            .and_then(|timings| timings.command.as_mut())
        {
            let total_ms = command_start.elapsed().as_secs_f64() * 1000.0;
            command.setup_ms += total_ms - command.total_ms;
            command.total_ms = total_ms;
        }
        result
    })
}

#[tauri::command]
async fn apply_row_updates(
    state: State<'_, AppState>,
    connection_id: String,
    query_token: String,
    updates: Vec<RowUpdate>,
) -> Result<ApplyUpdatesResult, AppError> {
    begin_operation(&state, &query_token).await;
    let outcome = async {
        let active = requested_connection(&state, Some(&connection_id)).await?;
        let tx = state.tx.lock().await;
        if let Some(session) = tx.as_ref() {
            check_transaction_connection(&session.connection_id, &active.id)?;
            let cancelled =
                register_cancel(&state, &query_token, &session.client, session.ssl).await?;
            let result = apply_rows(&session.client, &updates, true, &cancelled)
                .await
                .map_err(AppError::from);
            state.cancels.lock().await.remove(&query_token);
            result
        } else {
            // Hold the transaction-state lock so another request cannot begin a
            // manual transaction halfway through an independently committed batch.
            let pool = active_pool(&state, &active).await?;
            let client = checkout_pool_client(&pool.pool)
                .await
                .map_err(AppError::from)?;
            let cancelled = register_cancel(&state, &query_token, &client, active.ssl).await?;
            let result = apply_rows(&client, &updates, false, &cancelled)
                .await
                .map_err(AppError::from);
            state.cancels.lock().await.remove(&query_token);
            result
        }
    }
    .await;
    state.cancels.lock().await.remove(&query_token);
    outcome
}

#[tauri::command]
async fn cancel_query(
    state: State<'_, AppState>,
    query_token: String,
) -> Result<CancelResult, AppError> {
    let mut cancels = state.cancels.lock().await;
    prune_pending_cancels(&mut cancels);
    let entry = cancels
        .entry(query_token)
        .or_insert_with(CancelEntry::pending);
    entry.cancelled.store(true, Ordering::Release);
    if let Some(token) = entry.token.clone() {
        cancel_query_token(token, entry.ssl)
            .await
            .map_err(AppError::from)?;
        return Ok(CancelResult {
            cancelled: true,
            message: "Server-side cancel sent.".to_string(),
        });
    }

    Ok(CancelResult {
        cancelled: true,
        message: "Cancellation requested before query execution.".to_string(),
    })
}

#[tauri::command]
async fn connection_stats(
    state: State<'_, AppState>,
    connection_id: Option<String>,
) -> Result<Option<ConnectionStats>, AppError> {
    let connection_id = match connection_id {
        Some(id) => Some(id),
        None => state
            .active
            .read()
            .await
            .as_ref()
            .map(|connection| connection.id.clone()),
    };
    let Some(connection_id) = connection_id else {
        return Ok(None);
    };
    let cached_pool = state.pool.read().await;
    Ok(cached_connection_stats(
        cached_pool.as_ref(),
        &connection_id,
    ))
}

fn cached_connection_stats(
    cached_pool: Option<&ActivePool>,
    connection_id: &str,
) -> Option<ConnectionStats> {
    // Reading the status bar must never create a pool or connect to another
    // saved database. A missing pool is unavailable, not a zero-usage sample.
    cached_pool
        .filter(|cached| cached.connection_id == connection_id)
        .map(|cached| load_connection_stats(&cached.pool))
}

#[tauri::command]
async fn tx_status(state: State<'_, AppState>) -> Result<TxStatus, AppError> {
    Ok(tx_status_inner(&state).await)
}

#[tauri::command]
async fn tx_commit(app: AppHandle, state: State<'_, AppState>) -> Result<TxStatus, AppError> {
    finish_tx(&app, &state, "COMMIT").await?;
    let status = tx_status_inner(&state).await;
    let _ = app.emit("tx-changed", &status);
    Ok(status)
}

#[tauri::command]
async fn tx_rollback(app: AppHandle, state: State<'_, AppState>) -> Result<TxStatus, AppError> {
    finish_tx(&app, &state, "ROLLBACK").await?;
    let status = tx_status_inner(&state).await;
    let _ = app.emit("tx-changed", &status);
    Ok(status)
}

#[tauri::command]
async fn introspect_schema(
    state: State<'_, AppState>,
    connection_id: Option<String>,
) -> Result<SchemaTree, AppError> {
    let active = requested_connection(&state, connection_id.as_deref()).await?;
    // Reflect uncommitted DDL on the held transaction client, without borrowing
    // its one-client pool or publishing private metadata to the shared cache.
    let tx = state.tx.lock().await;
    if let Some(session) = tx
        .as_ref()
        .filter(|session| session.connection_id == active.id)
    {
        return session
            .reflection
            .refresh(&session.client)
            .await
            .map_err(Into::into);
    }
    drop(tx);
    let pool = active_pool(&state, &active).await?;
    let client = checkout_pool_client(&pool.pool)
        .await
        .map_err(AppError::from)?;
    pool.reflection.refresh(&client).await.map_err(Into::into)
}

#[tauri::command]
async fn foreign_key_values(
    state: State<'_, AppState>,
    connection_id: String,
    query_token: String,
    request: ForeignKeyLookup,
) -> Result<ForeignKeyValues, AppError> {
    begin_operation(&state, &query_token).await;
    let outcome = async {
        let active = requested_connection(&state, Some(&connection_id)).await?;
        // An isolated connection cannot steal, mutate or abort the user's held
        // transaction, even with a one-client pool. Preview committed rows only.
        let client = connect_client(&ConnectionFields::from(&active)).await.map_err(AppError::from)?;
        let cancelled = register_cancel(&state, &query_token, &client, active.ssl).await?;
        let outcome = async {
            if cancelled.load(Ordering::Acquire) {
                return Err(CoreError::Cancelled("Foreign-key preview cancelled.".into()));
            }
            client.batch_execute("BEGIN READ ONLY; SET LOCAL statement_timeout = '5s'; SET LOCAL lock_timeout = '1s'").await?;
            lookup_fk_values(&client, &request).await
        }.await;
        // Late cancellation must never interrupt cleanup or another request.
        state.cancels.lock().await.remove(&query_token);
        client.batch_execute("ROLLBACK").await.map_err(|error| AppError::from(CoreError::from(error)))?;
        outcome.map_err(AppError::from)
    }.await;
    state.cancels.lock().await.remove(&query_token);
    outcome
}

#[tauri::command]
async fn get_ddl(
    state: State<'_, AppState>,
    schema: String,
    name: String,
    connection_id: Option<String>,
) -> Result<String, AppError> {
    let active = requested_connection(&state, connection_id.as_deref()).await?;
    let pool = active_pool(&state, &active).await?;
    let client = checkout_pool_client(&pool.pool)
        .await
        .map_err(AppError::from)?;
    load_ddl(&client, &schema, &name).await.map_err(Into::into)
}

#[tauri::command]
async fn history_page(
    state: State<'_, AppState>,
    search: Option<String>,
    offset: u32,
    limit: u32,
) -> Result<HistoryPage, AppError> {
    state
        .storage
        .history_page(search, offset, limit)
        .map_err(Into::into)
}

#[tauri::command]
async fn history_stats(state: State<'_, AppState>) -> Result<HistoryStats, AppError> {
    state.storage.history_stats().map_err(Into::into)
}

#[tauri::command]
async fn history_get_result(
    state: State<'_, AppState>,
    id: i64,
) -> Result<HistoryResult, AppError> {
    state
        .storage
        .history_result(id)
        .map(|result| HistoryResult { result })
        .map_err(Into::into)
}

#[tauri::command]
async fn history_delete(state: State<'_, AppState>, id: i64) -> Result<(), AppError> {
    state.storage.history_delete(id).map_err(Into::into)
}

#[tauri::command]
async fn history_purge(state: State<'_, AppState>) -> Result<(), AppError> {
    state.storage.history_purge().map_err(Into::into)
}

#[tauri::command]
async fn save_draft(
    state: State<'_, AppState>,
    tab_kind: String,
    content: String,
) -> Result<(), AppError> {
    state
        .storage
        .save_draft(&tab_kind, &content)
        .map_err(Into::into)
}

#[tauri::command]
async fn load_draft(
    state: State<'_, AppState>,
    tab_kind: String,
) -> Result<Option<String>, AppError> {
    state.storage.load_draft(&tab_kind).map_err(Into::into)
}

#[tauri::command]
fn enable_exit_guard(state: State<'_, AppState>) {
    state.exit_guard_enabled.store(true, Ordering::Release);
}

#[tauri::command]
fn finish_exit(app: AppHandle, state: State<'_, AppState>) {
    state.exit_confirmed.store(true, Ordering::Release);
    app.exit(0);
}

#[tauri::command]
async fn ai_health(state: State<'_, AppState>) -> Result<AiHealth, AppError> {
    Ok(state.opencode.health().await)
}

#[tauri::command]
async fn ai_models(state: State<'_, AppState>) -> Result<Vec<String>, AppError> {
    state.opencode.models().await.map_err(Into::into)
}

#[tauri::command]
async fn ai_complete(
    state: State<'_, AppState>,
    request: AiCompletionRequest,
) -> Result<Option<String>, AppError> {
    state.opencode.complete(request).await.map_err(Into::into)
}

#[tauri::command]
async fn export_write_file(path: String, content: String) -> Result<(), AppError> {
    tokio::fs::write(path, content).await.map_err(Into::into)
}

fn emit_pool_reset(app: &AppHandle) {
    let _ = app.emit("pool-reset", ());
}

async fn run_query_with_state(
    app: &AppHandle,
    state: &State<'_, AppState>,
    run: QueryRun<'_>,
) -> Result<QueryResult, CoreError> {
    validate_transaction_sql(run.sql)?;
    if run.read_only_replay {
        return execute_read_only_replay(state, &run).await;
    }
    match classify_statement(run.sql) {
        StatementKind::TxBegin => {
            let mut tx = state.tx.lock().await;
            if tx.is_some() {
                return Err(CoreError::message("Transaction already active"));
            }

            let client = checkout_pool_client(run.pool).await?;
            let reflection = Arc::new(ReflectionCache::default());
            let result = execute_with_cancel(
                state,
                &client,
                &reflection,
                run.ssl,
                run.sql,
                run.query_token,
                run.offset,
                run.page_size,
            )
            .await?;
            let started_at = chrono::Utc::now().to_rfc3339();
            *tx = Some(TxSession {
                connection_id: run.active.id.clone(),
                client,
                ssl: run.ssl,
                started_at,
                reflection,
            });
            drop(tx);
            let _ = app.emit("tx-changed", tx_status_inner(state).await);
            Ok(result)
        }
        StatementKind::TxCommit => {
            let result = finish_tx_with_sql(
                app,
                state,
                run.sql,
                run.query_token,
                run.offset,
                run.page_size,
                &run.active.id,
            )
            .await?;
            Ok(result)
        }
        StatementKind::TxRollback => {
            let result = finish_tx_with_sql(
                app,
                state,
                run.sql,
                run.query_token,
                run.offset,
                run.page_size,
                &run.active.id,
            )
            .await?;
            Ok(result)
        }
        _ => {
            {
                let tx = state.tx.lock().await;
                if let Some(session) = tx.as_ref() {
                    check_transaction_connection(&session.connection_id, &run.active.id)?;
                    return execute_with_cancel(
                        state,
                        &session.client,
                        &session.reflection,
                        session.ssl,
                        run.sql,
                        run.query_token,
                        run.offset,
                        run.page_size,
                    )
                    .await;
                }
            }

            let client = checkout_pool_client(run.pool).await?;
            execute_with_cancel(
                state,
                &client,
                run.reflection,
                run.ssl,
                run.sql,
                run.query_token,
                run.offset,
                run.page_size,
            )
            .await
        }
    }
}

fn validate_replay_sql(sql: &str) -> Result<(), CoreError> {
    if split_statements(sql).len() != 1 || classify_statement(sql) != StatementKind::SelectLike {
        return Err(CoreError::message(
            "Automatic paging and export can only replay one read-only query. Run this SQL explicitly instead.",
        ));
    }
    Ok(())
}

async fn execute_read_only_replay(
    state: &State<'_, AppState>,
    run: &QueryRun<'_>,
) -> Result<QueryResult, CoreError> {
    validate_replay_sql(run.sql)?;
    let tx = state.tx.lock().await;
    if tx.is_some() {
        return Err(CoreError::message(
            "Commit or roll back the open transaction before loading more rows or re-running an export. You can still save the loaded rows.",
        ));
    }
    let client = checkout_pool_client(run.pool).await?;
    let cancelled = register_cancel(state, run.query_token, &client, run.ssl).await?;
    // Enforce read-only in PostgreSQL itself: a SELECT-like WITH statement may
    // contain INSERT/UPDATE/DELETE, which a lexical prefix check cannot detect.
    let begin = client.batch_execute("BEGIN READ ONLY").await;
    let result = match begin {
        Ok(()) if cancelled.load(Ordering::Acquire) => Err(CoreError::Cancelled(
            "Replay cancelled before execution.".into(),
        )),
        Ok(()) => {
            execute_on_client_with_reflection(
                &client,
                run.sql,
                run.offset,
                run.page_size,
                Some(&cancelled),
                run.reflection,
            )
            .await
        }
        Err(error) => Err(error.into()),
    };
    // Detach cancellation before cleanup so a late Cancel cannot interrupt the
    // ROLLBACK or hit a subsequent borrower of this pooled connection.
    state.cancels.lock().await.remove(run.query_token);
    if let Err(error) = client.batch_execute("ROLLBACK").await {
        drop(PooledPgClient::take(client));
        return Err(error.into());
    }
    drop(tx);
    result
}

#[allow(clippy::too_many_arguments)] // The cache must travel with the checked-out client.
async fn execute_with_cancel(
    state: &State<'_, AppState>,
    client: &PgClient,
    reflection: &ReflectionCache,
    ssl: bool,
    sql: &str,
    query_token: &str,
    offset: usize,
    page_size: usize,
) -> Result<QueryResult, CoreError> {
    let cancelled = register_cancel(state, query_token, client, ssl).await?;
    let result = execute_on_client_with_reflection(
        client,
        sql,
        offset,
        page_size,
        Some(&cancelled),
        reflection,
    )
    .await;
    state.cancels.lock().await.remove(query_token);
    result
}

async fn finish_tx_with_sql(
    app: &AppHandle,
    state: &State<'_, AppState>,
    sql: &str,
    query_token: &str,
    offset: usize,
    page_size: usize,
    connection_id: &str,
) -> Result<QueryResult, CoreError> {
    let mut tx = state.tx.lock().await;
    if let Some(session) = tx.as_ref() {
        check_transaction_connection(&session.connection_id, connection_id)?;
    }
    let session = tx
        .as_ref()
        .ok_or_else(|| CoreError::message("No active transaction"))?;
    let result = execute_with_cancel(
        state,
        &session.client,
        &session.reflection,
        session.ssl,
        sql,
        query_token,
        offset,
        page_size,
    )
    .await;
    if result.is_ok() {
        *tx = None;
        invalidate_pool_reflection(state, connection_id).await;
    }
    drop(tx);
    let _ = app.emit("tx-changed", tx_status_inner(state).await);
    result
}

async fn finish_tx(
    app: &AppHandle,
    state: &State<'_, AppState>,
    sql: &str,
) -> Result<(), AppError> {
    let mut tx = state.tx.lock().await;
    let session = tx
        .as_ref()
        .ok_or_else(|| CoreError::message("No active transaction"))?;
    execute_on_client(&session.client, sql, 0, 1)
        .await
        .map_err(AppError::from)?;
    invalidate_pool_reflection(state, &session.connection_id).await;
    *tx = None;
    drop(tx);
    let _ = app.emit("tx-changed", tx_status_inner(state).await);
    Ok(())
}

async fn active_connection(state: &State<'_, AppState>) -> Result<ConnectionRecord, AppError> {
    if let Some(active) = state.active.read().await.clone() {
        return Ok(active);
    }
    let active = state
        .storage
        .active_connection()
        .map_err(AppError::from)?
        .ok_or_else(|| CoreError::message("No active connection"))?;
    *state.active.write().await = Some(active.clone());
    Ok(active)
}

async fn requested_connection(
    state: &State<'_, AppState>,
    id: Option<&str>,
) -> Result<ConnectionRecord, AppError> {
    match id {
        Some(id) => state.storage.get_connection(id).map_err(AppError::from)?
            .ok_or_else(|| CoreError::message("The connection for this query no longer exists. Select a connection and run it again.").into()),
        None => active_connection(state).await,
    }
}

fn check_transaction_connection(transaction_id: &str, requested_id: &str) -> Result<(), CoreError> {
    if transaction_id != requested_id {
        return Err(CoreError::message(
            "A transaction is open on another connection. Commit or roll it back before querying this connection.",
        ));
    }
    Ok(())
}

async fn ensure_transaction_connection(
    state: &State<'_, AppState>,
    id: &str,
) -> Result<(), AppError> {
    if let Some(session) = state.tx.lock().await.as_ref() {
        check_transaction_connection(&session.connection_id, id)?;
    }
    Ok(())
}

async fn ensure_no_transaction<'a>(
    state: &'a State<'_, AppState>,
) -> Result<tokio::sync::MutexGuard<'a, Option<TxSession>>, AppError> {
    let guard = state.tx.lock().await;
    if guard.is_some() {
        return Err(CoreError::message(
            "Commit or roll back the open transaction before changing connections.",
        )
        .into());
    }
    Ok(guard)
}

fn prune_pending_cancels(cancels: &mut HashMap<String, CancelEntry>) {
    cancels.retain(|_, entry| {
        entry.token.is_some() || entry.created_at.elapsed() < Duration::from_secs(60)
    });
}

async fn begin_operation(state: &State<'_, AppState>, query_token: &str) {
    let mut cancels = state.cancels.lock().await;
    prune_pending_cancels(&mut cancels);
    cancels
        .entry(query_token.to_string())
        .or_insert_with(CancelEntry::pending);
}

async fn register_cancel(
    state: &State<'_, AppState>,
    query_token: &str,
    client: &PgClient,
    ssl: bool,
) -> Result<Arc<AtomicBool>, CoreError> {
    let mut cancels = state.cancels.lock().await;
    let entry = cancels
        .entry(query_token.to_string())
        .or_insert_with(CancelEntry::pending);
    if entry.cancelled.load(Ordering::Acquire) {
        return Err(CoreError::Cancelled(
            "Query cancelled before execution.".into(),
        ));
    }
    entry.token = Some(client.cancel_token());
    entry.ssl = ssl;
    Ok(Arc::clone(&entry.cancelled))
}

async fn active_pool(
    state: &State<'_, AppState>,
    active: &ConnectionRecord,
) -> Result<ActivePool, AppError> {
    let settings = state.storage.get_settings().map_err(AppError::from)?;

    if let Some(pool_state) = state.pool.read().await.as_ref()
        && pool_state.connection_id == active.id
        && pool_state.pool_size == settings.pool_size
    {
        return Ok(pool_state.clone());
    }

    let mut pool_state = state.pool.write().await;
    if let Some(existing) = pool_state.as_ref()
        && existing.connection_id == active.id
        && existing.pool_size == settings.pool_size
    {
        return Ok(existing.clone());
    }

    let pool =
        create_pool(&ConnectionFields::from(active), settings.pool_size).map_err(AppError::from)?;
    let client = checkout_pool_client(&pool).await.map_err(AppError::from)?;
    drop(client);

    let active_pool = ActivePool {
        connection_id: active.id.clone(),
        pool_size: settings.pool_size,
        pool: pool.clone(),
        reflection: Arc::new(ReflectionCache::default()),
    };
    *pool_state = Some(active_pool.clone());
    Ok(active_pool)
}

async fn invalidate_pool_reflection(state: &State<'_, AppState>, connection_id: &str) {
    if let Some(pool) = state
        .pool
        .read()
        .await
        .as_ref()
        .filter(|pool| pool.connection_id == connection_id)
    {
        pool.reflection.invalidate();
    }
}

async fn reset_pool(state: &State<'_, AppState>) {
    *state.pool.write().await = None;
}

async fn refresh_active(state: &State<'_, AppState>) -> Result<(), AppError> {
    let active = state.storage.active_connection().map_err(AppError::from)?;
    *state.active.write().await = active;
    Ok(())
}

async fn tx_status_inner(state: &State<'_, AppState>) -> TxStatus {
    let started_at = state
        .tx
        .lock()
        .await
        .as_ref()
        .map(|session| session.started_at.clone());
    TxStatus {
        active: started_at.is_some(),
        started_at,
    }
}

fn application_menu(app: &AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let menu = tauri::menu::Menu::default(app)?;
    #[cfg(target_os = "macos")]
    {
        use tauri::menu::{MenuItem, MenuItemKind, PredefinedMenuItem};

        // The macOS predefined Quit action calls Cocoa terminate: directly,
        // bypassing RunEvent::ExitRequested. Replace only that action so both
        // its menu click and Cmd+Q go through the draft-flushing exit handshake.
        let quit_label = PredefinedMenuItem::quit(app, None)?.text()?;
        for item in menu.items()? {
            if let MenuItemKind::Submenu(submenu) = item {
                for (index, item) in submenu.items()?.into_iter().enumerate() {
                    if let MenuItemKind::Predefined(native) = item
                        && native.text()? == quit_label
                    {
                        let quit = MenuItem::with_id(
                            app,
                            "catabase-quit",
                            &quit_label,
                            true,
                            Some("CmdOrCtrl+Q"),
                        )?;
                        submenu.remove(&native)?;
                        submenu.insert(&quit, index)?;
                    }
                }
            }
        }
    }
    Ok(menu)
}

pub fn run() {
    tauri::Builder::default()
        .menu(application_menu)
        .on_menu_event(|app, event| {
            if event.id() == "catabase-quit" {
                app.exit(0);
            }
        })
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let storage = Storage::open(data_dir.join("catabase.sqlite3"))?;
            let active = storage.active_connection()?;
            app.manage(AppState {
                storage,
                active: RwLock::new(active),
                pool: RwLock::new(None),
                tx: Mutex::new(None),
                cancels: Mutex::new(HashMap::new()),
                opencode: OpenCodeClient::default(),
                exit_guard_enabled: AtomicBool::new(false),
                exit_confirmed: AtomicBool::new(false),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            activate_connection,
            apply_row_updates,
            ai_complete,
            ai_health,
            ai_models,
            build_connection_url,
            cancel_query,
            connection_stats,
            delete_connection,
            duplicate_connection,
            execute_query,
            enable_exit_guard,
            export_write_file,
            finish_exit,
            get_ddl,
            get_settings,
            history_delete,
            history_get_result,
            history_page,
            history_purge,
            history_stats,
            introspect_schema,
            foreign_key_values,
            list_connections,
            load_draft,
            parse_connection_url,
            save_connection,
            save_draft,
            set_ai_model,
            set_pool_size,
            split_sql,
            test_connection,
            tx_commit,
            tx_rollback,
            tx_status,
        ])
        .build(tauri::generate_context!())
        .expect("failed to build Catabase")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                let state = app.state::<AppState>();
                if code != Some(tauri::RESTART_EXIT_CODE)
                    && state.exit_guard_enabled.load(Ordering::Acquire)
                    && !state.exit_confirmed.load(Ordering::Acquire)
                    && app.get_webview_window("main").is_some()
                {
                    // Native Quit bypasses window close handlers. Wait for the
                    // frontend to flush its latest editor snapshot before exit.
                    api.prevent_exit();
                    let _ = app.emit("catabase:request-exit", ());
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn editor_statement_offsets_use_monaco_utf16_units() {
        let sql = "select 'ž🐈';select '猫';select 1";
        let ranges = split_sql(sql.to_string()).await.unwrap();
        assert_eq!(ranges.len(), 3);
        let last_start = sql.rfind("select 1").unwrap();
        assert_eq!(ranges[0].end, "select 'ž🐈';".encode_utf16().count());
        assert_eq!(ranges[1].start, ranges[0].end);
        assert_eq!(ranges[2].start, sql[..last_start].encode_utf16().count());
        assert_eq!(ranges[2].end, sql.encode_utf16().count());
        assert_eq!(ranges[2].text, "select 1");
    }

    #[tokio::test]
    async fn editor_cursor_on_begin_targets_only_the_transaction_command() {
        let sql = "WITH foo AS (SELECT 'ž🐈' AS value)\n\nSELECT * FROM foo\nORDER BY value\n\n\nbegin\n\n\nSELECT 1";
        let ranges = split_sql(sql.to_string()).await.unwrap();
        assert_eq!(ranges.len(), 3);
        let begin_start = sql[..sql.find("begin").unwrap()].encode_utf16().count();
        for cursor in begin_start..begin_start + "begin".len() {
            let target = ranges
                .iter()
                .find(|range| cursor >= range.start && cursor < range.end)
                .unwrap();
            assert_eq!(target.text, "begin");
            assert_eq!(target.start, begin_start);
            assert_eq!(target.end, begin_start + "begin".len());
        }
    }

    #[test]
    fn replay_rejects_transaction_escape_and_explicit_mutations() {
        assert!(validate_replay_sql("select 1").is_ok());
        assert!(validate_replay_sql("select 1; COMMIT; DELETE FROM accounts").is_err());
        assert!(validate_replay_sql("UPDATE accounts SET name = 'changed' RETURNING *").is_err());
        assert!(validate_replay_sql("BEGIN").is_err());
        // This syntactically select-like form is deliberately delegated to the
        // server's READ ONLY transaction enforcement, not trusted by its prefix.
        assert!(
            validate_replay_sql(
                "WITH changed AS (DELETE FROM accounts RETURNING *) SELECT * FROM changed"
            )
            .is_ok()
        );
    }

    #[test]
    fn transaction_connection_must_match_exactly() {
        assert!(check_transaction_connection("source", "source").is_ok());
        assert!(check_transaction_connection("source", "other").is_err());
    }

    #[test]
    fn pool_stats_are_bound_to_the_requested_connection_without_connecting() {
        let pool = create_pool(
            &ConnectionFields {
                name: "unused".into(),
                host: "127.0.0.1".into(),
                port: 5432,
                database: "unused".into(),
                username: "unused".into(),
                password: None,
                ssl: false,
            },
            5,
        )
        .unwrap();
        let cached = ActivePool {
            connection_id: "source".into(),
            pool_size: 5,
            pool,
            reflection: Arc::new(ReflectionCache::default()),
        };

        let stats = cached_connection_stats(Some(&cached), "source").unwrap();
        assert_eq!(stats.active, 0);
        assert_eq!(stats.idle, 0);
        assert_eq!(stats.pool_size, 5);
        assert!(cached_connection_stats(Some(&cached), "other").is_none());
        assert!(cached_connection_stats(None, "source").is_none());
        assert_eq!(cached.pool.status().size, 0);
    }
}
