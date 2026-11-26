//! Statement completion hooks shared by editor queries and reviewed row edits.
//! Recording happens before returning to the UI, not at transaction commit.
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{CoreError, NewHistoryEntry, QueryResult, StatementRange, Storage};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchStatement {
    pub statement: StatementRange,
    pub result: Option<QueryResult>,
    pub duration_ms: u128,
    pub history_entry_id: Option<i64>,
}

pub trait StatementObserver: Send + Sync {
    fn started(&self, sql: &str);
    fn finished(&self, sql: &str, result: Result<&QueryResult, &CoreError>, duration_ms: u128);

    fn batch_result(&self, statement: &StatementRange, result: &mut QueryResult) {
        self.finished(&statement.text, Ok(result), result.duration_ms);
    }

    fn batch_error(&self, statement: &StatementRange, error: &CoreError, duration_ms: u128) {
        self.finished(&statement.text, Err(error), duration_ms);
    }
}

pub struct StatementHistory<'a> {
    storage: &'a Storage,
    connection_id: &'a str,
    notify: &'a (dyn Fn(Result<(), CoreError>) + Send + Sync),
    state: Mutex<RecordingState>,
    observer: Option<&'a dyn StatementObserver>,
}

#[derive(Default)]
struct RecordingState {
    transaction_id: Option<String>,
    pending: Option<PendingStatement>,
    recorded: usize,
    saved_ids: Vec<i64>,
    batch: Option<Vec<BatchStatement>>,
    save_time: Duration,
}

struct PendingStatement {
    sql: String,
    start: Instant,
    started_at: DateTime<Utc>,
}

impl<'a> StatementHistory<'a> {
    pub fn new(
        storage: &'a Storage,
        connection_id: &'a str,
        notify: &'a (dyn Fn(Result<(), CoreError>) + Send + Sync),
    ) -> Self {
        Self {
            storage,
            connection_id,
            notify,
            state: Mutex::new(RecordingState::default()),
            observer: None,
        }
    }

    pub fn with_observer(mut self, observer: &'a dyn StatementObserver) -> Self {
        self.observer = Some(observer);
        self
    }

    pub fn collect_batch_results(mut self) -> Self {
        self.state.get_mut().unwrap().batch = Some(Vec::new());
        self
    }

    pub fn take_batch_results(&self) -> Vec<BatchStatement> {
        self.state.lock().unwrap().batch.take().unwrap_or_default()
    }

    pub fn set_transaction(&self, id: Option<String>) {
        self.state.lock().unwrap().transaction_id = id;
    }

    pub fn finish_transaction(&self, status: &str) {
        let id = self.state.lock().unwrap().transaction_id.clone();
        if let Some(id) = id {
            (self.notify)(self.storage.finish_history_transaction(&id, status));
        }
    }

    pub fn save_time_ms(&self) -> f64 {
        self.state.lock().unwrap().save_time.as_secs_f64() * 1000.0
    }

    pub fn take_saved_ids(&self) -> Vec<i64> {
        std::mem::take(&mut self.state.lock().unwrap().saved_ids)
    }

    /// A bounded cancellation can drop the execution future before its normal
    /// completion hook. Save the in-flight statement, but never invent entries
    /// for the remaining, unexecuted statements in a batch.
    pub fn interrupted(&self, error: &CoreError) {
        let pending = self.state.lock().unwrap().pending.take();
        if let Some(pending) = pending {
            self.record(
                &pending.sql,
                Err(error),
                pending.start.elapsed().as_millis(),
                Some(pending.started_at),
            );
        }
    }

    pub fn rejected(&self, sql: &str, error: &CoreError) {
        self.interrupted(error);
        if self.state.lock().unwrap().recorded == 0 {
            self.finished(sql, Err(error), 0);
        }
    }

    fn finish_statement(
        &self,
        sql: &str,
        result: Result<&QueryResult, &CoreError>,
        duration_ms: u128,
    ) -> Option<i64> {
        let started_at = self
            .state
            .lock()
            .unwrap()
            .pending
            .take()
            .map(|pending| pending.started_at);
        self.record(sql, result, duration_ms, started_at)
    }

    fn record(
        &self,
        sql: &str,
        result: Result<&QueryResult, &CoreError>,
        duration_ms: u128,
        started_at: Option<DateTime<Utc>>,
    ) -> Option<i64> {
        if let Some(observer) = self.observer {
            observer.finished(sql, result, duration_ms);
        }
        let start = Instant::now();
        let mut state = self.state.lock().unwrap();
        let saved = match result {
            Ok(result) => self.storage.insert_transaction_query_history(
                Some(self.connection_id.to_owned()),
                state.transaction_id.clone(),
                started_at,
                sql.to_owned(),
                result,
            ),
            Err(error) => self.storage.insert_history(NewHistoryEntry {
                connection_id: Some(self.connection_id.to_owned()),
                transaction_id: state.transaction_id.clone(),
                started_at,
                query: sql.to_owned(),
                duration_ms,
                row_count: None,
                is_error: true,
                error_message: Some(
                    error
                        .pg_info()
                        .map(|info| info.message)
                        .unwrap_or_else(|| error.to_string()),
                ),
                result_json: None,
            }),
        };
        state.recorded += 1;
        if let Ok(id) = &saved {
            state.saved_ids.push(*id);
        }
        state.save_time += start.elapsed();
        drop(state);
        let id = saved.as_ref().ok().copied();
        (self.notify)(saved.map(|_| ()));
        id
    }
}

impl StatementObserver for StatementHistory<'_> {
    fn started(&self, sql: &str) {
        let pending = PendingStatement {
            sql: sql.to_owned(),
            start: Instant::now(),
            started_at: Utc::now(),
        };
        if let Some(observer) = self.observer {
            observer.started(sql);
        }
        self.state.lock().unwrap().pending = Some(pending);
    }

    fn finished(&self, sql: &str, result: Result<&QueryResult, &CoreError>, duration_ms: u128) {
        self.finish_statement(sql, result, duration_ms);
    }

    fn batch_result(&self, statement: &StatementRange, result: &mut QueryResult) {
        let duration_ms = result.duration_ms;
        let history_entry_id = self.finish_statement(&statement.text, Ok(result), duration_ms);
        if let Some(batch) = self.state.lock().unwrap().batch.as_mut() {
            let summary = QueryResult::command(result.row_count, duration_ms);
            batch.push(BatchStatement {
                statement: statement.clone(),
                result: Some(std::mem::replace(result, summary)),
                duration_ms,
                history_entry_id,
            });
        }
    }

    fn batch_error(&self, statement: &StatementRange, error: &CoreError, duration_ms: u128) {
        let history_entry_id = self.finish_statement(&statement.text, Err(error), duration_ms);
        if let Some(batch) = self.state.lock().unwrap().batch.as_mut() {
            batch.push(BatchStatement {
                statement: statement.clone(),
                result: None,
                duration_ms,
                history_entry_id,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn records_each_statement_before_commit_and_deduplicates_results() {
        let storage = Storage::in_memory().unwrap();
        let notifications = AtomicUsize::new(0);
        let notify = |saved: Result<(), CoreError>| {
            saved.unwrap();
            notifications.fetch_add(1, Ordering::Relaxed);
        };
        let id = storage.start_history_transaction("manual").unwrap();
        let history = StatementHistory::new(&storage, "docker", &notify);
        history.set_transaction(Some(id.clone()));
        for (index, sql) in ["BEGIN", "SELECT 1", "UPDATE t SET n = 1", "SELECT 1", "END"]
            .iter()
            .enumerate()
        {
            history.started(sql);
            history.finished(sql, Ok(&QueryResult::command(0, 1)), 1);
            let page = storage.history_transaction_page(&id, 0, 50).unwrap();
            assert_eq!(page.total, index as i64 + 1);
            assert_eq!(page.entries.last().unwrap().query, *sql);
            assert_eq!(
                page.entries[0].transaction.as_ref().unwrap().status,
                "active"
            );
        }
        storage.finish_history_transaction(&id, "finished").unwrap();
        assert_eq!(notifications.load(Ordering::Relaxed), 5);
        assert_eq!(storage.history_storage_stats().unwrap().unique_results, 1);
        let ids = history.take_saved_ids();
        let run = storage.history_entries(&ids).unwrap();
        assert_eq!(run.iter().map(|entry| entry.id).collect::<Vec<_>>(), ids);
        assert_eq!(
            run.iter()
                .map(|entry| entry.query.as_str())
                .collect::<Vec<_>>(),
            ["BEGIN", "SELECT 1", "UPDATE t SET n = 1", "SELECT 1", "END"]
        );
        for entry in &run {
            let start = DateTime::parse_from_rfc3339(entry.started_at.as_deref().unwrap()).unwrap();
            let end = DateTime::parse_from_rfc3339(&entry.executed_at).unwrap();
            assert!(start <= end);
        }
        let page = storage.history_transaction_page(&id, 1, 2).unwrap();
        assert_eq!(page.total, 5);
        assert_eq!(page.entries.len(), 2);
        assert_eq!(page.entries[0].query, "SELECT 1");
        assert_eq!(
            page.entries[0]
                .transaction
                .as_ref()
                .unwrap()
                .statement_count,
            5
        );
        assert_eq!(
            page.entries[0].transaction.as_ref().unwrap().status,
            "finished"
        );
        let matches = storage.history_page(Some("UPDATE".into()), 0, 50).unwrap();
        assert_eq!(matches.entries[0].transaction.as_ref().unwrap().id, id);
        storage.history_delete(page.entries[0].id).unwrap();
        assert_eq!(
            storage.history_transaction_page(&id, 0, 50).unwrap().total,
            4
        );
    }

    #[test]
    fn batch_capture_preserves_saved_results_and_omits_implicit_begin() {
        let storage = Storage::in_memory().unwrap();
        let notify = |saved: Result<(), CoreError>| saved.unwrap();
        let history = StatementHistory::new(&storage, "docker", &notify).collect_batch_results();
        let ranges = crate::split_statements("SELECT 1; SELECT missing;");
        history.finished("BEGIN", Ok(&QueryResult::command(0, 0)), 0);
        let mut result = QueryResult::command(1, 2);
        result.rows = vec![serde_json::json!({ "value": "1" })];
        history.started(&ranges[0].text);
        history.batch_result(&ranges[0], &mut result);
        history.started(&ranges[1].text);
        history.batch_error(&ranges[1], &CoreError::message("Column missing"), 3);

        let ids = history.take_saved_ids();
        let batch = history.take_batch_results();
        assert_eq!(ids.len(), 3);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].statement, ranges[0]);
        assert_eq!(batch[0].history_entry_id, Some(ids[1]));
        assert_eq!(batch[1].statement, ranges[1]);
        assert_eq!(batch[1].history_entry_id, Some(ids[2]));
        assert_eq!(batch[1].duration_ms, 3);
        assert!(batch[1].result.is_none());
        assert!(result.rows.is_empty());
        let saved = storage.history_result(ids[1]).unwrap().unwrap();
        assert_eq!(saved["rows"][0], batch[0].result.as_ref().unwrap().rows[0]);
    }

    #[test]
    fn cancellation_logs_only_in_flight_statement_once_and_keeps_completed_entries() {
        let storage = Storage::in_memory().unwrap();
        let notify = |saved: Result<(), CoreError>| saved.unwrap();
        let history = StatementHistory::new(&storage, "docker", &notify);
        history.started("SELECT 1;");
        history.finished("SELECT 1;", Ok(&QueryResult::command(1, 1)), 1);
        history.started("SELECT pg_sleep(10);");
        let error = CoreError::ConnectionDiscarded("cancel timed out".into());
        history.interrupted(&error);
        history.rejected("SELECT 1; SELECT pg_sleep(10); SELECT 3", &error);
        let entries = storage.history_page(None, 0, 50).unwrap().entries;
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].query, "SELECT pg_sleep(10);");
        assert!(entries[0].is_error);
        assert!(!entries[1].is_error);
    }

    #[test]
    fn reopening_preserves_membership_and_marks_only_unfinished_transactions_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        let storage = Storage::open(&path).unwrap();
        let notify = |saved: Result<(), CoreError>| saved.unwrap();
        let history = StatementHistory::new(&storage, "docker", &notify);
        let active = storage.start_history_transaction("manual").unwrap();
        let ended = storage.start_history_transaction("review").unwrap();
        for id in [&active, &ended] {
            history.set_transaction(Some(id.clone()));
            history.finished("BEGIN", Ok(&QueryResult::command(0, 1)), 1);
        }
        storage
            .finish_history_transaction(&ended, "finished")
            .unwrap();
        drop(history);
        drop(storage);
        let reopened = Storage::open(path).unwrap();
        reopened.interrupt_history_transactions().unwrap();
        for (id, status) in [(&active, "interrupted"), (&ended, "finished")] {
            let entries = reopened
                .history_transaction_page(id, 0, 50)
                .unwrap()
                .entries;
            assert_eq!(entries[0].transaction.as_ref().unwrap().status, status);
        }
        reopened.history_purge().unwrap();
        assert_eq!(reopened.history_storage_stats().unwrap().stored_bytes, 0);
    }
}
