use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Done,
    /// Job completed but the reality-checker flagged validation warnings.
    Flagged,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: String,
    pub status: JobStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub message: String,
    pub provider: String,
    pub project: Option<String>,
    pub channel: Option<String>,
    pub created_at: String,
    pub finished_at: Option<String>,
}

/// Sent through the mpsc channel from HTTP handlers to the worker pool.
#[derive(Debug)]
pub struct JobRequest {
    pub job_id: String,
    pub message: String,
    pub provider: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub project: Option<String>,
    pub channel: Option<String>,
    pub max_turns: Option<usize>,
    pub verbose: bool,
    /// Named skills to activate for this job (Level 1 metadata injected into prompt).
    pub skills: Option<Vec<String>>,
    /// Execution mode: "task" (default) or "evaluate"/"review" for persona/assessment jobs.
    pub mode: Option<String>,
    /// Named workflow to execute. If set, `message` is used as the workflow input and
    /// each step is dispatched as a separate agent call.
    pub workflow: Option<String>,
}

pub type JobStore = Arc<RwLock<HashMap<String, Job>>>;

/// Maximum number of jobs kept in the in-memory store at once.
/// When the cap is exceeded, the oldest terminal jobs (done/flagged/failed)
/// are evicted. Active jobs (queued/running) are never removed.
pub const MAX_STORE_JOBS: usize = 1_000;

pub fn new_store() -> JobStore {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Evict the oldest finished jobs to keep the store at or below [`MAX_STORE_JOBS`].
/// Called each time a new job is inserted.
pub fn evict_oldest_finished(store: &mut HashMap<String, Job>) {
    if store.len() <= MAX_STORE_JOBS {
        return;
    }
    let mut finished: Vec<(String, String)> = store
        .iter()
        .filter(|(_, j)| {
            matches!(
                j.status,
                JobStatus::Done | JobStatus::Flagged | JobStatus::Failed
            )
        })
        .map(|(id, j)| (id.clone(), j.created_at.clone()))
        .collect();
    // Oldest first — ISO-8601 timestamps sort lexicographically
    finished.sort_by(|a, b| a.1.cmp(&b.1));
    let to_remove = store.len().saturating_sub(MAX_STORE_JOBS);
    for (id, _) in finished.into_iter().take(to_remove) {
        store.remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_job(id: &str, status: JobStatus, created_at: &str) -> Job {
        Job {
            id: id.to_string(),
            status,
            output: None,
            error: None,
            message: "test".to_string(),
            provider: "claude-code".to_string(),
            project: None,
            channel: None,
            created_at: created_at.to_string(),
            finished_at: None,
        }
    }

    #[test]
    fn evict_below_cap_is_noop() {
        let mut store = HashMap::new();
        store.insert("a".to_string(), make_job("a", JobStatus::Done, "2026-01-01T00:00:00Z"));
        evict_oldest_finished(&mut store);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn evict_removes_oldest_finished_when_over_cap() {
        let mut store = HashMap::new();
        // Fill to cap + 1 with Done jobs
        for i in 0..=MAX_STORE_JOBS {
            let id = format!("job-{i:04}");
            let ts = format!("2026-01-{:02}T00:00:00Z", (i % 28) + 1);
            store.insert(id.clone(), make_job(&id, JobStatus::Done, &ts));
        }
        assert_eq!(store.len(), MAX_STORE_JOBS + 1);
        evict_oldest_finished(&mut store);
        assert_eq!(store.len(), MAX_STORE_JOBS);
    }

    #[test]
    fn evict_does_not_remove_active_jobs() {
        let mut store = HashMap::new();
        // Fill to cap with Done jobs, then add 1 Running job
        for i in 0..MAX_STORE_JOBS {
            let id = format!("done-{i:04}");
            store.insert(id.clone(), make_job(&id, JobStatus::Done, "2026-01-01T00:00:00Z"));
        }
        store.insert("active".to_string(), make_job("active", JobStatus::Running, "2026-01-01T00:00:00Z"));
        // Now at cap + 1
        evict_oldest_finished(&mut store);
        assert_eq!(store.len(), MAX_STORE_JOBS);
        assert!(store.contains_key("active"), "running job must not be evicted");
    }

    #[test]
    fn new_store_is_empty() {
        let store = new_store();
        let guard = store.try_read().unwrap();
        assert!(guard.is_empty());
    }

    #[test]
    fn job_status_serializes_lowercase() {
        let s = serde_json::to_string(&JobStatus::Queued).unwrap();
        assert_eq!(s, "\"queued\"");
        let s = serde_json::to_string(&JobStatus::Running).unwrap();
        assert_eq!(s, "\"running\"");
        let s = serde_json::to_string(&JobStatus::Done).unwrap();
        assert_eq!(s, "\"done\"");
        let s = serde_json::to_string(&JobStatus::Flagged).unwrap();
        assert_eq!(s, "\"flagged\"");
        let s = serde_json::to_string(&JobStatus::Failed).unwrap();
        assert_eq!(s, "\"failed\"");
    }

    #[test]
    fn job_serializes_fields() {
        let job = Job {
            id: "abc-123".to_string(),
            status: JobStatus::Queued,
            output: None,
            error: None,
            message: "hello".to_string(),
            provider: "claude-code".to_string(),
            project: Some("my-app".to_string()),
            channel: None,
            created_at: "2026-04-03T09:00:00Z".to_string(),
            finished_at: None,
        };
        let json = serde_json::to_string(&job).unwrap();
        assert!(json.contains("\"id\":\"abc-123\""));
        assert!(json.contains("\"status\":\"queued\""));
        assert!(json.contains("\"message\":\"hello\""));
    }
}
