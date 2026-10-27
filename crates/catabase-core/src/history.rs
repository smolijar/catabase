//! Statement completion hooks shared by editor queries and reviewed row edits.
//! Recording happens before returning to the UI, not at transaction commit.
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use crate::{CoreError, NewHistoryEntry, QueryResult, Storage};

pub trait StatementObserver: Send + Sync {
    fn started(&self, sql: &str);
    fn finished(&self, sql: &str, result: Result<&QueryResult, &CoreError>, duration_ms: u128);
}

pub struct StatementHistory<'a> {
    storage: &'a Storage,
    connection_id: &'a str,
    notify: &'a (dyn Fn(Result<(), CoreError>) + Send + Sync),
    state: Mutex<RecordingState>,
}

#[derive(Default)]
struct RecordingState {
    transaction_id: Option<String>,
    pending: Option<(String, Instant)>,
    recorded: usize,
    save_time: Duration,
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
        }
    }

    pub fn set_transaction(&self, id: Option<String>) {
        self.state.lock().unwrap().transaction_id = id;
    }

    pub fn save_time_ms(&self) -> f64 {
        self.state.lock().unwrap().save_time.as_secs_f64() * 1000.0
    }

    /// A bounded cancellation can drop the execution future before its normal
    /// completion hook. Save the in-flight statement, but never invent entries
    /// for the remaining, unexecuted statements in a batch.
    pub fn interrupted(&self, error: &CoreError) {
        let pending = self.state.lock().unwrap().pending.take();
        if let Some((sql, start)) = pending {
            self.finished(&sql, Err(error), start.elapsed().as_millis());
        }
    }

    pub fn rejected(&self, sql: &str, error: &CoreError) {
        self.interrupted(error);
        if self.state.lock().unwrap().recorded == 0 {
            self.finished(sql, Err(error), 0);
        }
    }
}

impl StatementObserver for StatementHistory<'_> {
    fn started(&self, sql: &str) {
        self.state.lock().unwrap().pending = Some((sql.to_owned(), Instant::now()));
    }

    fn finished(&self, sql: &str, result: Result<&QueryResult, &CoreError>, duration_ms: u128) {
        let start = Instant::now();
        let mut state = self.state.lock().unwrap();
        let saved = match result {
            Ok(result) => self.storage.insert_transaction_query_history(
                Some(self.connection_id.to_owned()),
                state.transaction_id.clone(),
                sql.to_owned(),
                result,
            ),
            Err(error) => self.storage.insert_history(NewHistoryEntry {
                connection_id: Some(self.connection_id.to_owned()),
                transaction_id: state.transaction_id.clone(),
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
        state.pending = None;
        state.recorded += 1;
        state.save_time += start.elapsed();
        drop(state);
        (self.notify)(saved.map(|_| ()));
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
