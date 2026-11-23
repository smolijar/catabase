use std::{collections::HashMap, sync::atomic::Ordering, time::Duration};

use serde::{Deserialize, Serialize};
use tokio_postgres::SimpleQueryMessage;
use uuid::Uuid;

use crate::{CoreError, PgClient, QueryCancellation};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionActivity {
    pub inserted: u64,
    pub updated: u64,
    pub deleted: u64,
    pub tables: usize,
}

type RowCounters = HashMap<u32, [u64; 3]>;

pub struct TransactionActivityTracker {
    previous: Option<RowCounters>,
    observed: RowCounters,
    available: bool,
}

impl Default for TransactionActivityTracker {
    fn default() -> Self {
        Self {
            previous: None,
            observed: HashMap::new(),
            available: true,
        }
    }
}

impl TransactionActivityTracker {
    pub fn totals(&self) -> Option<TransactionActivity> {
        self.available.then(|| {
            let mut totals = TransactionActivity {
                tables: self.observed.len(),
                ..TransactionActivity::default()
            };
            for [inserted, updated, deleted] in self.observed.values() {
                totals.inserted = totals.inserted.saturating_add(*inserted);
                totals.updated = totals.updated.saturating_add(*updated);
                totals.deleted = totals.deleted.saturating_add(*deleted);
            }
            totals
        })
    }

    pub async fn before_write(
        &mut self,
        client: &PgClient,
        ssl: bool,
        cancellation: &QueryCancellation,
        settings_in_batch: bool,
    ) -> Result<(), CoreError> {
        if self.previous.is_none() && self.available && !cancellation.flag().load(Ordering::Acquire)
        {
            // A snapshot before SET TRANSACTION in the same batch would prevent
            // PostgreSQL from changing transaction isolation or taking a snapshot.
            if !settings_in_batch {
                self.previous = read_counters(client, ssl, cancellation).await?;
            }
            self.available = self.previous.is_some();
        }
        Ok(())
    }

    pub async fn after_write(
        &mut self,
        client: &PgClient,
        ssl: bool,
        cancellation: &QueryCancellation,
    ) -> Result<(), CoreError> {
        if self.previous.is_none() {
            return Ok(());
        }
        if let Some(latest) = read_counters(client, ssl, cancellation).await? {
            self.update(latest);
            self.available = true;
        } else {
            self.available = false;
        }
        Ok(())
    }

    fn update(&mut self, latest: RowCounters) {
        let Some(previous) = &self.previous else {
            return;
        };
        for (table, counters) in &latest {
            let start = previous.get(table).copied().unwrap_or_default();
            let delta = [
                counters[0].saturating_sub(start[0]),
                counters[1].saturating_sub(start[1]),
                counters[2].saturating_sub(start[2]),
            ];
            if delta != [0, 0, 0] {
                let observed = self.observed.entry(*table).or_default();
                for (total, count) in observed.iter_mut().zip(delta) {
                    *total = total.saturating_add(count);
                }
            }
        }
        self.previous = Some(latest);
    }
}

async fn read_counters(
    client: &PgClient,
    ssl: bool,
    cancellation: &QueryCancellation,
) -> Result<Option<RowCounters>, CoreError> {
    // The savepoint isolates a counters failure or cancellation from the user's
    // transaction. Roll back the temporary timeout setting even on success.
    let savepoint = format!("catabase_counts_{}", Uuid::new_v4().simple());
    let restore = format!("ROLLBACK TO SAVEPOINT {savepoint}; RELEASE SAVEPOINT {savepoint}");
    let sql = format!(
        "SAVEPOINT {savepoint}; SET LOCAL statement_timeout = '1000ms';
         SELECT pg_catalog.current_setting('track_counts');
         SELECT relid, n_tup_ins, n_tup_upd, n_tup_del
           FROM pg_catalog.pg_stat_xact_user_tables
          WHERE n_tup_ins <> 0 OR n_tup_upd <> 0 OR n_tup_del <> 0;
         {restore}"
    );
    let messages = match cancellation
        .run(client, ssl, async { Ok(client.simple_query(&sql).await?) })
        .await
    {
        Ok(messages) => messages,
        Err(CoreError::Cancelled(_)) => return Ok(None),
        Err(error @ CoreError::ConnectionDiscarded(_)) => return Err(error),
        Err(error) if error.pg_info().and_then(|info| info.code).as_deref() == Some("25P02") => {
            // A transaction already in the failed state rejected the initial
            // SAVEPOINT. Leave any user savepoints available for recovery.
            return Ok(None);
        }
        Err(_) => {
            if !matches!(
                tokio::time::timeout(Duration::from_secs(2), client.batch_execute(&restore)).await,
                Ok(Ok(()))
            ) {
                return Err(CoreError::ConnectionDiscarded(
                    "The transaction session could not be recovered after a row count check."
                        .into(),
                ));
            }
            return Ok(None);
        }
    };
    let mut rows = messages.iter().filter_map(|message| match message {
        SimpleQueryMessage::Row(row) => Some(row),
        _ => None,
    });
    if rows.next().and_then(|row| row.get(0)) != Some("on") {
        return Ok(None);
    }
    let counters = rows
        .map(|row| {
            Some((
                row.get(0)?.parse().ok()?,
                [
                    row.get(1)?.parse().ok()?,
                    row.get(2)?.parse().ok()?,
                    row.get(3)?.parse().ok()?,
                ],
            ))
        })
        .collect();
    Ok(counters)
}
