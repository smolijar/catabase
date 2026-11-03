use super::*;
use catabase_core::{build_explain_sql, extract_explain_plan};

#[cfg(test)]
#[path = "explain_tests.rs"]
mod tests;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExplainRequest {
    query: String,
    preamble: String,
    use_second_result: bool,
    #[serde(default)]
    title: String,
    #[serde(default)]
    comments: String,
    share: bool,
    connection_id: String,
    query_token: String,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
enum ExplainStage {
    FirstRun,
    SecondRun,
    Sharing,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExplainProgress<'a> {
    query_token: &'a str,
    stage: ExplainStage,
    run: usize,
    total_runs: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExplainTiming {
    duration_ms: u128,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExplainResult {
    plan: String,
    query: String,
    explain_sql: String,
    runs: Vec<ExplainTiming>,
    used_run: usize,
    url: Option<String>,
    share_error: Option<String>,
}

struct LocalPlan {
    plan: String,
    runs: Vec<ExplainTiming>,
}

/// Both passes use the same checked-out client and cancellation signal. No
/// automatic BEGIN, COMMIT, or ROLLBACK is issued by this explicit workflow.
struct ExplainSession<'a> {
    client: &'a PgClient,
    reflection: &'a ReflectionCache,
    history: &'a StatementHistory<'a>,
    cancellation: &'a QueryCancellation,
    ssl: bool,
}

impl ExplainSession<'_> {
    async fn run(
        &self,
        sql: &str,
        use_second_result: bool,
        progress: impl Fn(ExplainStage, usize),
    ) -> Result<LocalPlan, CoreError> {
        let total_runs = if use_second_result { 2 } else { 1 };
        let mut runs = Vec::with_capacity(total_runs);
        let mut plan = String::new();
        for run in 1..=total_runs {
            if self.cancellation.flag().load(Ordering::Acquire) {
                return Err(CoreError::Cancelled(
                    "Explain cancelled before the next run.".into(),
                ));
            }
            progress(
                if run == 1 {
                    ExplainStage::FirstRun
                } else {
                    ExplainStage::SecondRun
                },
                run,
            );
            let result = self
                .cancellation
                .run(
                    self.client,
                    self.ssl,
                    execute_on_client_observed(
                        self.client,
                        sql,
                        0,
                        10_000,
                        Some(self.cancellation.flag()),
                        self.reflection,
                        Some(self.history),
                    ),
                )
                .await;
            let result = match result {
                Ok(result) => result,
                Err(error) => {
                    self.history.interrupted(&error);
                    self.history.rejected(sql, &error);
                    return Err(error);
                }
            };
            // Validate every result before starting another pass. Never execute
            // a second statement just because the first produced unexpected data.
            plan = extract_explain_plan(&result)?;
            runs.push(ExplainTiming {
                duration_ms: result.duration_ms,
            });
        }
        Ok(LocalPlan { plan, runs })
    }
}

#[tauri::command]
pub(super) async fn explain_query(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ExplainRequest,
) -> Result<ExplainResult, AppError> {
    let _workspace = state.updates.workspace_operation()?;
    // The dedicated Explain command bypasses automatic transaction creation
    // only after validating one EXPLAIN statement. Generic query IPC cannot
    // request this exception or execute arbitrary preamble statements.
    let sql = build_explain_sql(&request.query, &request.preamble).map_err(|error| {
        let mut error = AppError::from(error);
        // No database request has happened. The UI must not turn a healthy
        // connection red (or refresh its check time) for invalid modal input.
        error.code = Some("CATABASE_EXPLAIN_INPUT".into());
        error
    })?;
    let cancellation = begin_operation(&state, &request.query_token).await;
    let total_runs = if request.use_second_result { 2 } else { 1 };
    let progress = |stage, run| {
        let _ = app.emit(
            "explain-progress",
            ExplainProgress {
                query_token: &request.query_token,
                stage,
                run,
                total_runs,
            },
        );
    };
    let outcome = async {
        let (active, pool) = cancellation
            .setup(async {
                let active = requested_connection(&state, Some(&request.connection_id)).await?;
                ensure_transaction_connection(&state, &active.id).await?;
                let pool = active_pool(&state, &active).await?;
                Ok::<_, AppError>((active, pool))
            })
            .await?;
        let notify = |saved| notify_history(&app, saved);
        let history = StatementHistory::new(&state.storage, &active.id, &notify);
        let mut tx = cancellation
            .setup(async { Ok::<_, CoreError>(state.tx.lock().await) })
            .await?;
        let local = if let Some(session) = tx.as_ref() {
            check_transaction_connection(&session.connection_id, &active.id)?;
            history.set_transaction(Some(session.history_id.clone()));
            let result = ExplainSession {
                client: &session.client,
                reflection: &session.reflection,
                history: &history,
                cancellation: &cancellation,
                ssl: session.ssl,
            }
            .run(&sql, request.use_second_result, &progress)
            .await;
            if must_discard(&result) || session.client.is_closed() {
                discard_transaction(&app, &state, &mut tx);
                invalidate_pool_reflection(&state, &active.id).await;
            }
            result
        } else {
            let client = cancellation.setup(checkout_pool_client(&pool.pool)).await?;
            let result = ExplainSession {
                client: &client,
                reflection: &pool.reflection,
                history: &history,
                cancellation: &cancellation,
                ssl: active.ssl,
            }
            .run(&sql, request.use_second_result, &progress)
            .await;
            release_client(client, &result);
            result
        };
        // Database routing remains locked across both passes, but uploading a
        // completed plan must not hold a database client or transaction lock.
        drop(tx);
        let _ = app.emit("tx-changed", tx_status_inner(&state).await);
        let local = local?;
        let mut result = ExplainResult {
            plan: local.plan,
            query: request.query.clone(),
            explain_sql: sql,
            runs: local.runs,
            used_run: total_runs,
            url: None,
            share_error: None,
        };
        if request.share {
            if cancellation.flag().load(Ordering::Acquire) {
                result.share_error = Some(
                    "The plan is ready. Sharing was skipped because cancellation was requested."
                        .into(),
                );
            } else {
                progress(ExplainStage::Sharing, total_runs);
                match cancellation
                    .setup(super::dalibo::share_plan(
                        &result.plan,
                        &request.query,
                        &request.title,
                        &request.comments,
                    ))
                    .await
                {
                    Ok(url) => result.url = Some(url),
                    Err(CoreError::Cancelled(_)) => {
                        result.share_error = Some(
                            "Sharing cancelled. Dalibo may already have received the plan.".into(),
                        )
                    }
                    Err(error) => result.share_error = Some(error.to_string()),
                }
            }
        }
        Ok::<_, AppError>(result)
    }
    .await;
    state.cancels.lock().await.remove(&request.query_token);
    outcome
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ShareRequest {
    plan: String,
    query: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    comments: String,
    query_token: String,
}

#[derive(Serialize)]
pub(super) struct ShareResult {
    url: String,
}

#[tauri::command]
pub(super) async fn share_explain(
    state: State<'_, AppState>,
    request: ShareRequest,
) -> Result<ShareResult, AppError> {
    let _workspace = state.updates.workspace_operation()?;
    let cancellation = begin_operation(&state, &request.query_token).await;
    let result = cancellation
        .setup(super::dalibo::share_plan(
            &request.plan,
            &request.query,
            &request.title,
            &request.comments,
        ))
        .await;
    state.cancels.lock().await.remove(&request.query_token);
    match result {
        Ok(url) => Ok(ShareResult { url }),
        Err(CoreError::Cancelled(_)) => Err(CoreError::Cancelled(
            "Sharing cancelled. Dalibo may already have received the plan.".into(),
        )
        .into()),
        Err(error) => Err(error.into()),
    }
}

#[tauri::command]
pub(super) async fn open_explain_link(url: String) -> Result<(), AppError> {
    super::dalibo::validate_plan_url(&url)?;
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = tokio::process::Command::new("open");
        command.arg(&url);
        command
    };
    #[cfg(target_os = "linux")]
    let mut command = {
        let mut command = tokio::process::Command::new("xdg-open");
        command.arg(&url);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = tokio::process::Command::new("rundll32.exe");
        command.arg("url.dll,FileProtocolHandler").arg(&url);
        command
    };
    let status = tokio::time::timeout(Duration::from_secs(10), command.status())
        .await
        .map_err(|_| CoreError::message("Opening the plan timed out."))??;
    if !status.success() {
        return Err(CoreError::message("Could not open the plan in your browser.").into());
    }
    Ok(())
}
