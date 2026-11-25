//! Background scheduler loop for autonomous self-scheduled tasks.
//!
//! Spawns a Tokio task that polls `Store::get_due_tasks()` on a fixed interval
//! and executes each due task by calling `Runtime::complete_with_needs()`.
//!
//! Successful tasks are marked delivered (or rescheduled when recurring).
//! Failed tasks are retried up to `MAX_RETRIES` times before permanent failure.

use std::sync::Arc;

use kernex_core::context::ContextNeeds;
use kernex_core::message::Request;
use kernex_core::traits::Provider;
use kernex_runtime::Runtime;
use tokio::sync::watch;
use tokio::task::JoinHandle;

const MAX_RETRIES: u32 = 3;

/// Handle returned by [`spawn`]. Lets the caller signal shutdown and wait for
/// the scheduler task to drain the in-flight task batch.
pub struct SchedulerHandle {
    shutdown_tx: watch::Sender<bool>,
    join: Option<JoinHandle<()>>,
}

impl SchedulerHandle {
    /// Signal the scheduler to stop and await its task. Idempotent; the
    /// second call is a no-op.
    pub async fn shutdown(mut self) {
        let _ = self.shutdown_tx.send(true);
        if let Some(handle) = self.join.take() {
            if let Err(e) = handle.await {
                // A panic here means the scheduler had already been dead for
                // however long the REPL stayed open. Log the payload at
                // error, not a bare warn: this used to read as an untidy
                // exit rather than a loop that stopped scheduling.
                crate::utils::log_join_error("scheduler", e);
            }
        }
    }
}

/// Spawn the scheduler loop as a background Tokio task.
///
/// Returns a [`SchedulerHandle`] the caller is expected to keep until the
/// process is exiting; calling [`SchedulerHandle::shutdown`] then signals the
/// loop to stop on its next tick boundary and awaits its `JoinHandle` so any
/// currently-running task batch has a chance to settle before the runtime
/// drops.
pub fn spawn(
    runtime: Arc<Runtime>,
    provider: Arc<dyn Provider>,
    needs: ContextNeeds,
    poll_secs: u64,
) -> SchedulerHandle {
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let join = tokio::spawn(async move {
        // The JoinHandle below is not polled until `shutdown`, so without
        // this guard a panic in the loop is silent for the life of the
        // process: tasks simply stop being scheduled and nothing says why.
        let _panic_guard = crate::utils::PanicLogGuard::new("scheduler loop");
        tracing::debug!("scheduler: started, polling every {poll_secs}s");
        let interval = tokio::time::Duration::from_secs(poll_secs);
        loop {
            tokio::select! {
                biased;
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        tracing::debug!("scheduler: shutdown signalled, stopping");
                        return;
                    }
                }
                _ = tokio::time::sleep(interval) => {
                    run_due_tasks(&runtime, provider.as_ref(), &needs).await;
                }
            }
        }
    });
    SchedulerHandle {
        shutdown_tx,
        join: Some(join),
    }
}

/// Spawn the scheduler for a `kx dev` session, or nothing when the session
/// keeps no memory (`--no-memory`): due tasks are read from the memory
/// database and their completion is written back to it.
pub fn spawn_for_session(
    runtime: &Arc<Runtime>,
    provider: &Arc<dyn Provider>,
    session: &ContextNeeds,
    poll_secs: u64,
) -> Option<SchedulerHandle> {
    session.store.then(|| {
        let needs = crate::runtime_glue::context_needs(false);
        spawn(runtime.clone(), provider.clone(), needs, poll_secs)
    })
}

/// Stop the scheduler [`spawn_for_session`] started, if it started one.
pub async fn shutdown(handle: Option<SchedulerHandle>) {
    if let Some(handle) = handle {
        handle.shutdown().await;
    }
}

async fn run_due_tasks(runtime: &Runtime, provider: &dyn Provider, needs: &ContextNeeds) {
    let tasks = match runtime.store.get_due_tasks().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("scheduler: get_due_tasks failed: {e}");
            return;
        }
    };

    if tasks.is_empty() {
        return;
    }

    tracing::info!("scheduler: {} task(s) due", tasks.len());

    for task in tasks {
        let request = Request::text(&task.sender_id, &task.description);
        match runtime.complete_with_needs(provider, &request, needs).await {
            Ok(_) => {
                tracing::info!(
                    "scheduler: task {} completed",
                    &task.id[..8.min(task.id.len())]
                );
                if let Err(e) = runtime
                    .store
                    .complete_task(&task.id, task.repeat.as_deref())
                    .await
                {
                    tracing::warn!("scheduler: complete_task {} failed: {e}", task.id);
                }
            }
            Err(e) => {
                tracing::warn!(
                    "scheduler: task {} failed: {e}",
                    &task.id[..8.min(task.id.len())]
                );
                let _ = runtime
                    .store
                    .fail_task(&task.id, &e.to_string(), MAX_RETRIES)
                    .await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernex_core::context::ContextNeeds;

    #[test]
    fn context_needs_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<ContextNeeds>();
    }

    struct NoProvider;

    #[async_trait::async_trait]
    impl Provider for NoProvider {
        fn name(&self) -> &str {
            "none"
        }
        fn requires_api_key(&self) -> bool {
            false
        }
        async fn complete(
            &self,
            _ctx: &kernex_core::context::Context,
        ) -> Result<kernex_core::message::Response, kernex_core::error::KernexError> {
            Err(kernex_core::error::KernexError::Config("not called".into()))
        }
        async fn is_available(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn a_session_without_memory_runs_no_scheduler() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = Arc::new(
            kernex_runtime::RuntimeBuilder::new()
                .data_dir(dir.path().to_str().unwrap())
                .project("demo")
                .build()
                .await
                .unwrap(),
        );
        let provider: Arc<dyn Provider> = Arc::new(NoProvider);
        let no_memory = crate::runtime_glue::context_needs(true);
        assert!(spawn_for_session(&runtime, &provider, &no_memory, 60).is_none());

        let with_memory = crate::runtime_glue::context_needs(false);
        let handle = spawn_for_session(&runtime, &provider, &with_memory, 60);
        assert!(handle.is_some());
        shutdown(handle).await;
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn max_retries_is_nonzero() {
        assert!(MAX_RETRIES > 0);
    }
}
