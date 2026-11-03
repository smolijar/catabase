//! Opt-in checks use only the guarded zenbase docker connection and temporary
//! tables from the shared fixture. They never upload a plan or change app data.
use super::*;
use crate::auto_transaction_tests::fixture_pool;
use catabase_core::DEFAULT_EXPLAIN_PREAMBLE;
use serde_json::{Value, json};
use std::error::Error;

#[tokio::test]
#[ignore = "requires explicit CATABASE_TEST_CONNECTION_STORE for saved zenbase docker"]
async fn two_runs_return_second_plan_and_skip_automatic_transactions() -> Result<(), Box<dyn Error>>
{
    let (active, pool) = fixture_pool().await?;
    let client = checkout_pool_client(&pool).await?;
    let storage = Storage::in_memory()?;
    assert!(storage.get_settings()?.auto_transaction);
    let notify = |saved: Result<(), CoreError>| saved.unwrap();
    let history = StatementHistory::new(&storage, &active.id, &notify);
    let reflection = ReflectionCache::default();
    let cancellation = QueryCancellation::default();
    let sql = build_explain_sql(
        "UPDATE catabase_auto_transaction_probe SET value = 11 WHERE value = 10 RETURNING value",
        DEFAULT_EXPLAIN_PREAMBLE,
    )?;
    let stages = std::sync::Mutex::new(Vec::new());
    let result = ExplainSession {
        client: &client,
        reflection: &reflection,
        history: &history,
        cancellation: &cancellation,
        ssl: false,
    }
    .run(&sql, true, |stage, run| {
        stages.lock().unwrap().push((stage, run))
    })
    .await?;
    assert_eq!(result.runs.len(), 2);
    assert_eq!(
        *stages.lock().unwrap(),
        vec![(ExplainStage::FirstRun, 1), (ExplainStage::SecondRun, 2)]
    );
    let plan: Value = serde_json::from_str(&result.plan)?;
    assert_eq!(
        plan[0]["Plan"]["Actual Rows"],
        json!(0),
        "The second pass updated no rows and must be the displayed plan."
    );
    let entries = storage.history_page(None, 0, 50)?;
    assert_eq!(entries.total, 2);
    assert!(
        entries
            .entries
            .iter()
            .all(|entry| entry.query == sql && entry.transaction.is_none() && !entry.is_error)
    );
    client.batch_execute("ROLLBACK").await?;
    let value: i32 = client
        .query_one("SELECT value FROM catabase_auto_transaction_probe", &[])
        .await?
        .get(0);
    assert_eq!(
        value, 11,
        "Explain ANALYZE writes committed normally; automatic transaction mode must not wrap this explicit workflow."
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicit CATABASE_TEST_CONNECTION_STORE for saved zenbase docker"]
async fn explain_reuses_existing_transaction_and_can_return_text_plan() -> Result<(), Box<dyn Error>>
{
    let (active, pool) = fixture_pool().await?;
    let client = checkout_pool_client(&pool).await?;
    let storage = Storage::in_memory()?;
    let notify = |saved: Result<(), CoreError>| saved.unwrap();
    let history = StatementHistory::new(&storage, &active.id, &notify);
    let transaction = storage.start_history_transaction("manual")?;
    history.set_transaction(Some(transaction.clone()));
    let reflection = ReflectionCache::default();
    let cancellation = QueryCancellation::default();
    client.batch_execute("BEGIN").await?;
    let sql = build_explain_sql(
        "UPDATE catabase_auto_transaction_probe SET value = value + 1 RETURNING value",
        "EXPLAIN (ANALYZE, COSTS OFF, FORMAT TEXT)",
    )?;
    let result = ExplainSession {
        client: &client,
        reflection: &reflection,
        history: &history,
        cancellation: &cancellation,
        ssl: false,
    }
    .run(&sql, false, |_, _| {})
    .await?;
    assert_eq!(result.runs.len(), 1);
    assert!(result.plan.contains("Update on"));
    assert!(result.plan.contains("Execution Time:"));
    let changed: i32 = client
        .query_one("SELECT value FROM catabase_auto_transaction_probe", &[])
        .await?
        .get(0);
    assert_eq!(changed, 11);
    client.batch_execute("ROLLBACK").await?;
    let restored: i32 = client
        .query_one("SELECT value FROM catabase_auto_transaction_probe", &[])
        .await?
        .get(0);
    assert_eq!(
        restored, 10,
        "Explain must not commit or roll back the existing transaction."
    );
    assert_eq!(
        storage.history_transaction_page(&transaction, 0, 50)?.total,
        1
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires explicit CATABASE_TEST_CONNECTION_STORE for saved zenbase docker"]
async fn explain_can_cancel_first_or_second_run_without_starting_more_work()
-> Result<(), Box<dyn Error>> {
    for cancel_run in [1, 2] {
        let (active, pool) = fixture_pool().await?;
        let client = checkout_pool_client(&pool).await?;
        let storage = Storage::in_memory()?;
        let notify = |saved: Result<(), CoreError>| saved.unwrap();
        let history = StatementHistory::new(&storage, &active.id, &notify);
        let reflection = ReflectionCache::default();
        let cancellation = Arc::new(QueryCancellation::default());
        let sql = build_explain_sql("SELECT pg_sleep(0.15)", DEFAULT_EXPLAIN_PREAMBLE)?;
        let stages = std::sync::Mutex::new(Vec::new());
        let result = ExplainSession {
            client: &client,
            reflection: &reflection,
            history: &history,
            cancellation: &cancellation,
            ssl: false,
        }
        .run(&sql, true, |stage, run| {
            stages.lock().unwrap().push(stage);
            if run == cancel_run {
                let cancellation = Arc::clone(&cancellation);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    cancellation.request();
                });
            }
        })
        .await;
        let error = result.err().expect("The selected pass must be cancelled");
        assert_eq!(
            error.pg_info().and_then(|error| error.code).as_deref(),
            Some("57014")
        );
        assert_eq!(stages.lock().unwrap().len(), cancel_run);
        let entries = storage.history_page(None, 0, 50)?;
        assert_eq!(entries.total, cancel_run as i64);
        assert!(entries.entries[0].is_error);
        assert_eq!(
            entries
                .entries
                .iter()
                .filter(|entry| !entry.is_error)
                .count(),
            cancel_run - 1
        );
        let recovered: i32 = client.query_one("SELECT 1", &[]).await?.get(0);
        assert_eq!(
            recovered, 1,
            "The cancel sender must finish before the client can be reused."
        );
    }
    Ok(())
}
