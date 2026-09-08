use std::collections::HashMap;

use catabase_core::{
    AiCompletionRequest, AiHealth, ConnectionFields, ConnectionInput, ConnectionRecord,
    ConnectionStats, CoreError, HistoryPage, HistoryStats, NewHistoryEntry, OpenCodeClient,
    PgCancelToken, PgClient, PgErrorInfo, PgPool, PooledPgClient, QueryResult, SchemaTree,
    Settings, StatementKind, StatementRange, Storage, build_connection_url as build_url,
    cancel_query_token, checkout_pool_client, classify_statement,
    connection_stats as load_connection_stats, create_pool, execute_on_client,
    get_ddl_on_client as load_ddl, introspect_schema_on_client as load_schema,
    parse_connection_url as parse_url, split_statements, test_connection as test_pg_connection,
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
}

struct ActivePool {
    connection_id: String,
    pool_size: u32,
    pool: PgPool,
}

struct TxSession {
    connection_id: String,
    client: PooledPgClient,
    ssl: bool,
    started_at: String,
}

struct CancelEntry {
    token: PgCancelToken,
    ssl: bool,
}

struct QueryRun<'a> {
    active: &'a ConnectionRecord,
    pool: &'a PgPool,
    ssl: bool,
    sql: &'a str,
    query_token: &'a str,
    offset: usize,
    page_size: usize,
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
    let record = state
        .storage
        .save_connection(input)
        .map_err(AppError::from)?;
    refresh_active(&state).await?;
    reset_tx(&app, &state).await;
    reset_pool(&state).await;
    emit_pool_reset(&app);
    Ok(record)
}

#[tauri::command]
async fn delete_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    state
        .storage
        .delete_connection(&id)
        .map_err(AppError::from)?;
    refresh_active(&state).await?;
    reset_tx(&app, &state).await;
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
    let record = state
        .storage
        .activate_connection(&id)
        .map_err(AppError::from)?;
    *state.active.write().await = Some(record.clone());
    reset_tx(&app, &state).await;
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
    let normalized = state
        .storage
        .set_pool_size(pool_size)
        .map_err(AppError::from)?;
    reset_tx(&app, &state).await;
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
    Ok(split_statements(&sql))
}

#[tauri::command]
async fn execute_query(
    app: AppHandle,
    state: State<'_, AppState>,
    sql: String,
    query_token: String,
    offset: u32,
    page_size: u32,
) -> Result<QueryResult, AppError> {
    let active = active_connection(&state).await?;
    let pool = active_pool(&state, &active).await?;
    let result = run_query_with_state(
        &app,
        &state,
        QueryRun {
            active: &active,
            pool: &pool,
            ssl: active.ssl,
            sql: &sql,
            query_token: &query_token,
            offset: offset as usize,
            page_size: page_size as usize,
        },
    )
    .await;

    match result {
        Ok(result) => {
            let _ = state.storage.insert_history(NewHistoryEntry {
                connection_id: Some(active.id),
                query: sql,
                duration_ms: result.duration_ms,
                row_count: Some(result.row_count),
                is_error: false,
                error_message: None,
                result_json: serde_json::to_value(&result).ok(),
            });
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

#[tauri::command]
async fn cancel_query(
    state: State<'_, AppState>,
    query_token: String,
) -> Result<CancelResult, AppError> {
    let entry = state.cancels.lock().await.remove(&query_token);
    if let Some(entry) = entry {
        cancel_query_token(entry.token, entry.ssl)
            .await
            .map_err(AppError::from)?;
        return Ok(CancelResult {
            cancelled: true,
            message: "Server-side cancel sent.".to_string(),
        });
    }

    Ok(CancelResult {
        cancelled: false,
        message: "Query already finished or was not registered.".to_string(),
    })
}

#[tauri::command]
async fn connection_stats(state: State<'_, AppState>) -> Result<ConnectionStats, AppError> {
    let active = active_connection(&state).await?;
    let pool = active_pool(&state, &active).await?;
    Ok(load_connection_stats(&pool))
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
async fn introspect_schema(state: State<'_, AppState>) -> Result<SchemaTree, AppError> {
    let active = active_connection(&state).await?;
    let pool = active_pool(&state, &active).await?;
    let client = checkout_pool_client(&pool).await.map_err(AppError::from)?;
    load_schema(&client).await.map_err(Into::into)
}

#[tauri::command]
async fn get_ddl(
    state: State<'_, AppState>,
    schema: String,
    name: String,
) -> Result<String, AppError> {
    let active = active_connection(&state).await?;
    let pool = active_pool(&state, &active).await?;
    let client = checkout_pool_client(&pool).await.map_err(AppError::from)?;
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
    match classify_statement(run.sql) {
        StatementKind::TxBegin => {
            if state.tx.lock().await.is_some() {
                return Err(CoreError::message("Transaction already active"));
            }

            let client = checkout_pool_client(run.pool).await?;
            let result = execute_with_cancel(
                state,
                &client,
                run.ssl,
                run.sql,
                run.query_token,
                run.offset,
                run.page_size,
            )
            .await?;
            let started_at = chrono::Utc::now().to_rfc3339();
            *state.tx.lock().await = Some(TxSession {
                connection_id: run.active.id.clone(),
                client,
                ssl: run.ssl,
                started_at,
            });
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
            )
            .await?;
            Ok(result)
        }
        _ => {
            let needs_reset = {
                let mut tx = state.tx.lock().await;
                if let Some(session) = tx.as_mut() {
                    if session.connection_id != run.active.id {
                        true
                    } else {
                        return execute_with_cancel(
                            state,
                            &session.client,
                            session.ssl,
                            run.sql,
                            run.query_token,
                            run.offset,
                            run.page_size,
                        )
                        .await;
                    }
                } else {
                    false
                }
            };

            if needs_reset {
                reset_tx(app, state).await;
            }

            let client = checkout_pool_client(run.pool).await?;
            execute_with_cancel(
                state,
                &client,
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

async fn execute_with_cancel(
    state: &State<'_, AppState>,
    client: &PgClient,
    ssl: bool,
    sql: &str,
    query_token: &str,
    offset: usize,
    page_size: usize,
) -> Result<QueryResult, CoreError> {
    state.cancels.lock().await.insert(
        query_token.to_string(),
        CancelEntry {
            token: client.cancel_token(),
            ssl,
        },
    );
    let result = execute_on_client(client, sql, offset, page_size).await;
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
) -> Result<QueryResult, CoreError> {
    let session = state
        .tx
        .lock()
        .await
        .take()
        .ok_or_else(|| CoreError::message("No active transaction"))?;
    let result = execute_with_cancel(
        state,
        &session.client,
        session.ssl,
        sql,
        query_token,
        offset,
        page_size,
    )
    .await;
    let _ = app.emit("tx-changed", tx_status_inner(state).await);
    result
}

async fn finish_tx(
    app: &AppHandle,
    state: &State<'_, AppState>,
    sql: &str,
) -> Result<(), AppError> {
    let session = state
        .tx
        .lock()
        .await
        .take()
        .ok_or_else(|| CoreError::message("No active transaction"))?;
    execute_on_client(&session.client, sql, 0, 1)
        .await
        .map_err(AppError::from)?;
    let _ = app.emit("tx-changed", tx_status_inner(state).await);
    Ok(())
}

async fn reset_tx(app: &AppHandle, state: &State<'_, AppState>) {
    let session = state.tx.lock().await.take();
    if let Some(session) = session {
        let _ = execute_on_client(&session.client, "ROLLBACK", 0, 1).await;
    }
    let _ = app.emit("tx-changed", tx_status_inner(state).await);
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

async fn active_pool(
    state: &State<'_, AppState>,
    active: &ConnectionRecord,
) -> Result<PgPool, AppError> {
    let settings = state.storage.get_settings().map_err(AppError::from)?;

    if let Some(pool_state) = state.pool.read().await.as_ref()
        && pool_state.connection_id == active.id
        && pool_state.pool_size == settings.pool_size
    {
        return Ok(pool_state.pool.clone());
    }

    let mut pool_state = state.pool.write().await;
    if let Some(existing) = pool_state.as_ref()
        && existing.connection_id == active.id
        && existing.pool_size == settings.pool_size
    {
        return Ok(existing.pool.clone());
    }

    let pool =
        create_pool(&ConnectionFields::from(active), settings.pool_size).map_err(AppError::from)?;
    let client = checkout_pool_client(&pool).await.map_err(AppError::from)?;
    drop(client);

    *pool_state = Some(ActivePool {
        connection_id: active.id.clone(),
        pool_size: settings.pool_size,
        pool: pool.clone(),
    });
    Ok(pool)
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

pub fn run() {
    tauri::Builder::default()
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
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            activate_connection,
            ai_complete,
            ai_health,
            ai_models,
            build_connection_url,
            cancel_query,
            connection_stats,
            delete_connection,
            execute_query,
            export_write_file,
            get_ddl,
            get_settings,
            history_delete,
            history_get_result,
            history_page,
            history_purge,
            history_stats,
            introspect_schema,
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
        .run(tauri::generate_context!())
        .expect("failed to run Catabase");
}
