use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use tokio::sync::watch;

use crate::{CoreError, PgClient, cancel_query_token};

/// Cancellation is signalled locally. Network I/O is driven by the query owner,
/// which keeps the client checked out until the cancel packet has finished.
pub struct QueryCancellation {
    flag: AtomicBool,
    requested: watch::Sender<bool>,
}

impl Default for QueryCancellation {
    fn default() -> Self {
        Self {
            flag: AtomicBool::new(false),
            requested: watch::channel(false).0,
        }
    }
}

impl QueryCancellation {
    pub fn request(&self) {
        self.flag.store(true, Ordering::Release);
        self.requested.send_replace(true);
    }

    pub fn flag(&self) -> &AtomicBool {
        &self.flag
    }

    async fn requested(&self) {
        let mut receiver = self.requested.subscribe();
        let _ = receiver.wait_for(|requested| *requested).await;
    }

    /// Only use around setup that has not submitted SQL on a checked-out client.
    pub async fn setup<T, E: From<CoreError>>(
        &self,
        future: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        tokio::select! {
            biased;
            () = self.requested() => Err(CoreError::Cancelled("Query cancelled before execution.".into()).into()),
            result = future => result,
        }
    }

    /// On ConnectionDiscarded the owner MUST detach and drop the pooled client,
    /// never recycle it or retain it as a manual transaction session.
    pub async fn run<T>(
        &self,
        client: &PgClient,
        ssl: bool,
        future: impl Future<Output = Result<T, CoreError>>,
    ) -> Result<T, CoreError> {
        self.run_with_cancel(
            future,
            || cancel_query_token(client.cancel_token(), ssl),
            Duration::from_secs(5),
        )
        .await
    }

    async fn run_with_cancel<T, C: Future<Output = Result<(), CoreError>>>(
        &self,
        future: impl Future<Output = Result<T, CoreError>>,
        send: impl FnOnce() -> C,
        grace: Duration,
    ) -> Result<T, CoreError> {
        if self.flag.load(Ordering::Acquire) {
            return Err(CoreError::Cancelled(
                "Query cancelled before execution.".into(),
            ));
        }
        tokio::pin!(future);
        tokio::select! {
            biased;
            result = &mut future => return result,
            () = self.requested() => {},
        }
        // Wait for BOTH the original request and its cancel sender. Returning
        // the client sooner could let a late cancel hit its next borrower.
        match tokio::time::timeout(grace, async { tokio::join!(future, send()) }).await {
            Ok((result, _cancel_delivery)) => result,
            Err(_) => Err(CoreError::ConnectionDiscarded(
                "Cancellation was not confirmed within 5 seconds. The local connection to the unresponsive session was closed and will not be reused. If this query wrote data or committed a transaction, its outcome is unknown; verify it before retrying.".into()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::pending, sync::Arc};

    #[tokio::test]
    async fn cancellation_interrupts_setup_and_never_starts_a_cancelled_query() {
        let cancel = QueryCancellation::default();
        cancel.request();
        assert!(matches!(
            cancel.setup::<(), CoreError>(pending()).await,
            Err(CoreError::Cancelled(_))
        ));
        assert!(matches!(
            cancel
                .run_with_cancel(
                    pending::<Result<(), CoreError>>(),
                    || async { panic!("must not send") },
                    Duration::ZERO
                )
                .await,
            Err(CoreError::Cancelled(_))
        ));
    }

    #[tokio::test]
    async fn stalled_execution_and_cancel_transport_have_a_bounded_exit() {
        for stalled_sender in [false, true] {
            let cancel = Arc::new(QueryCancellation::default());
            let requester = Arc::clone(&cancel);
            let task = tokio::spawn(async move {
                requester
                    .run_with_cancel(
                        pending::<Result<(), CoreError>>(),
                        || async move {
                            if stalled_sender {
                                pending().await
                            } else {
                                Ok(())
                            }
                        },
                        Duration::from_millis(20),
                    )
                    .await
            });
            tokio::task::yield_now().await;
            cancel.request();
            let result = tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(result, Err(CoreError::ConnectionDiscarded(_))));
        }
    }

    #[tokio::test]
    async fn finished_query_does_not_release_client_while_cancel_is_in_flight() {
        let cancel = Arc::new(QueryCancellation::default());
        let requester = Arc::clone(&cancel);
        let (complete, completed) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            requester
                .run_with_cancel(
                    async {
                        completed.await.unwrap();
                        Ok(42)
                    },
                    pending,
                    Duration::from_millis(20),
                )
                .await
        });
        tokio::task::yield_now().await;
        cancel.request();
        tokio::task::yield_now().await;
        complete.send(()).unwrap();
        assert!(matches!(
            task.await.unwrap(),
            Err(CoreError::ConnectionDiscarded(_))
        ));
    }
}
