use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::jobs::{Job, JobRequest, JobStatus};
use super::AppState;
use crate::utils;

#[derive(Debug, Deserialize)]
pub struct RunBody {
    pub message: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub channel: Option<String>,
    pub max_turns: Option<usize>,
    /// Named skills to activate. Each name must match an installed skill in the project's data dir.
    pub skills: Option<Vec<String>>,
    /// Execution mode: "task" (default) or "evaluate"/"review".
    pub mode: Option<String>,
    /// Named workflow to execute from the workflows directory.
    pub workflow: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WebhookBody {
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobIdResponse {
    pub job_id: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<usize>,
}

pub async fn handle_health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub async fn handle_run(
    State(state): State<AppState>,
    Json(body): Json<RunBody>,
) -> Result<Json<JobIdResponse>, (StatusCode, Json<ErrorResponse>)> {
    let job_id = Uuid::new_v4().to_string();
    let provider = body
        .provider
        .unwrap_or_else(|| state.default_flags.name.clone());

    let job = Job {
        id: job_id.clone(),
        status: JobStatus::Queued,
        output: None,
        error: None,
        message: body.message.clone(),
        provider: provider.clone(),
        project: body.project.clone(),
        channel: body.channel.clone(),
        created_at: utils::iso_timestamp(),
        finished_at: None,
    };

    state.jobs.write().await.insert(job_id.clone(), job);

    let req = JobRequest {
        job_id: job_id.clone(),
        message: body.message,
        provider,
        model: body.model.or_else(|| state.default_flags.model.clone()),
        api_key: state.default_flags.api_key.clone(),
        base_url: state.default_flags.base_url.clone(),
        project: body.project,
        channel: body.channel,
        max_turns: body.max_turns.or(state.default_flags.max_turns),
        verbose: state.default_flags.verbose,
        skills: body.skills,
        mode: body.mode,
        workflow: body.workflow,
    };

    state.tx.send(req).await.map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "job queue full".to_string(),
            }),
        )
    })?;

    Ok(Json(JobIdResponse { job_id }))
}

pub async fn handle_get_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Job>, (StatusCode, Json<ErrorResponse>)> {
    let store = state.jobs.read().await;
    store.get(&id).cloned().map(Json).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("job not found: {id}"),
            }),
        )
    })
}

pub async fn handle_list_jobs(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Json<Vec<Job>> {
    let limit = query.limit.unwrap_or(50);
    let store = state.jobs.read().await;
    let mut jobs: Vec<Job> = store.values().cloned().collect();
    jobs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    jobs.truncate(limit);
    Json(jobs)
}

pub async fn handle_webhook(
    State(state): State<AppState>,
    Path(event): Path<String>,
    Json(body): Json<WebhookBody>,
) -> Result<Json<JobIdResponse>, (StatusCode, Json<ErrorResponse>)> {
    let message = body
        .message
        .unwrap_or_else(|| format!("Webhook event triggered: {event}"));

    handle_run(
        State(state),
        Json(RunBody {
            message,
            provider: None,
            model: None,
            project: None,
            channel: Some(format!("webhook-{event}")),
            max_turns: None,
            skills: None,
            mode: None,
            workflow: None,
        }),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_response_fields() {
        let r = HealthResponse {
            status: "ok",
            version: "0.4.0",
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"version\":\"0.4.0\""));
    }

    #[test]
    fn error_response_serializes() {
        let r = ErrorResponse {
            error: "not found".to_string(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "{\"error\":\"not found\"}");
    }

    #[test]
    fn job_id_response_serializes() {
        let r = JobIdResponse {
            job_id: "abc-123".to_string(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, "{\"job_id\":\"abc-123\"}");
    }

    #[test]
    fn run_body_deserializes() {
        let raw = r#"{"message":"hello","provider":"ollama"}"#;
        let body: RunBody = serde_json::from_str(raw).unwrap();
        assert_eq!(body.message, "hello");
        assert_eq!(body.provider, Some("ollama".to_string()));
        assert!(body.model.is_none());
    }

    #[test]
    fn run_body_minimal() {
        let raw = r#"{"message":"test"}"#;
        let body: RunBody = serde_json::from_str(raw).unwrap();
        assert_eq!(body.message, "test");
        assert!(body.provider.is_none());
    }

    #[test]
    fn webhook_body_optional_message() {
        let raw = r#"{}"#;
        let body: WebhookBody = serde_json::from_str(raw).unwrap();
        assert!(body.message.is_none());

        let raw = r#"{"message":"deploy"}"#;
        let body: WebhookBody = serde_json::from_str(raw).unwrap();
        assert_eq!(body.message, Some("deploy".to_string()));
    }
}
