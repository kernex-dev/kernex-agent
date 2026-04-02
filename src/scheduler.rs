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

const MAX_RETRIES: u32 = 3;

/// Spawn the scheduler loop as a background Tokio task.
///
/// Returns immediately; the loop runs until the process exits.
pub fn spawn(
    runtime: Arc<Runtime>,
    provider: Arc<dyn Provider>,
    needs: ContextNeeds,
    poll_secs: u64,
) {
    tokio::spawn(async move {
        tracing::debug!("scheduler: started, polling every {poll_secs}s");
        let interval = tokio::time::Duration::from_secs(poll_secs);
        loop {
            tokio::time::sleep(interval).await;
            run_due_tasks(&runtime, provider.as_ref(), &needs).await;
        }
    });
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

    #[test]
    fn max_retries_is_nonzero() {
        assert!(MAX_RETRIES > 0);
    }
}
