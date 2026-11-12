//! In-flight sends by request id, so a send can be cancelled (plan 03 §6).

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, MutexGuard, PoisonError};

use tokio::task::AbortHandle;

use crate::error::{AppError, ErrorCode};

#[derive(Debug, Default)]
pub struct Inflight {
    tasks: Mutex<HashMap<String, AbortHandle>>,
}

/// Removes the id when the waiting caller finishes or is dropped.
/// Also aborts the task, so a dropped caller never leaves a send running unseen.
struct Registered<'a> {
    inflight: &'a Inflight,
    id: String,
    handle: AbortHandle,
}

impl Drop for Registered<'_> {
    fn drop(&mut self) {
        self.handle.abort();
        self.inflight.lock().remove(&self.id);
    }
}

pub fn cancelled() -> AppError {
    AppError::new(ErrorCode::Cancelled, "The request was cancelled.")
}

impl Inflight {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, AbortHandle>> {
        self.tasks.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs `future` as a task under `id`. A second run with a live id is refused; an
    /// aborted task surfaces as `cancelled`.
    pub async fn run<T, F>(&self, id: &str, future: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: Future<Output = Result<T, AppError>> + Send + 'static,
    {
        let handle = {
            let mut tasks = self.lock();
            if tasks.contains_key(id) {
                return Err(AppError::invalid_input(
                    "This request is already being sent.",
                ));
            }
            let handle = tokio::spawn(future);
            tasks.insert(id.to_string(), handle.abort_handle());
            handle
        };
        let _registered = Registered {
            inflight: self,
            id: id.to_string(),
            handle: handle.abort_handle(),
        };
        match handle.await {
            Ok(result) => result,
            Err(error) if error.is_cancelled() => Err(cancelled()),
            Err(error) => Err(AppError::internal("The request failed unexpectedly.")
                .with_detail(error.to_string())),
        }
    }

    /// Aborts the send with `id`; an unknown id is a no-op.
    pub fn cancel(&self, id: &str) {
        if let Some(handle) = self.lock().get(id) {
            handle.abort();
        }
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn a_finished_task_returns_its_result_and_leaves_the_registry() {
        let inflight = Inflight::default();

        let result = inflight.run("a", async { Ok(7) }).await;

        assert_eq!(result.unwrap(), 7);
        assert!(inflight.is_empty());
    }

    #[tokio::test]
    async fn dropping_the_caller_aborts_the_task() {
        let inflight = Inflight::default();
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&finished);

        let waiting = inflight.run("a", async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
        let timed_out = tokio::time::timeout(Duration::from_millis(5), waiting).await;
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert!(timed_out.is_err());
        assert!(!finished.load(std::sync::atomic::Ordering::SeqCst));
        assert!(inflight.is_empty());
    }

    #[tokio::test]
    async fn cancel_aborts_and_a_live_id_cannot_run_twice() {
        let inflight = Arc::new(Inflight::default());
        let running = Arc::clone(&inflight);
        let task = tokio::spawn(async move {
            running
                .run("a", async {
                    tokio::time::sleep(Duration::from_secs(30)).await;
                    Ok(())
                })
                .await
        });
        while inflight.is_empty() {
            tokio::task::yield_now().await;
        }

        let second = inflight.run("a", async { Ok(()) }).await;
        inflight.cancel("unknown");
        inflight.cancel("a");
        let first = task.await.unwrap();

        assert_eq!(second.unwrap_err().code, ErrorCode::InvalidInput);
        assert_eq!(first.unwrap_err().code, ErrorCode::Cancelled);
        assert!(inflight.is_empty());
    }
}
