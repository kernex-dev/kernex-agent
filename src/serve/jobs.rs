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

pub fn new_store() -> JobStore {
    Arc::new(RwLock::new(HashMap::new()))
}

#[cfg(test)]
mod tests {
    use super::*;

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
