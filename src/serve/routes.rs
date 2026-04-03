use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::jobs::{evict_oldest_finished, Job, JobRequest, JobStatus};
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

#[derive(Debug, Default, Serialize)]
pub struct JobStats {
    pub queued: usize,
    pub running: usize,
    pub done: usize,
    pub flagged: usize,
    pub failed: usize,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub jobs: JobStats,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<usize>,
}

pub async fn handle_health(State(state): State<AppState>) -> Json<HealthResponse> {
    let store = state.jobs.read().await;
    let mut stats = JobStats::default();
    for job in store.values() {
        stats.total += 1;
        match job.status {
            JobStatus::Queued => stats.queued += 1,
            JobStatus::Running => stats.running += 1,
            JobStatus::Done => stats.done += 1,
            JobStatus::Flagged => stats.flagged += 1,
            JobStatus::Failed => stats.failed += 1,
        }
    }
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        jobs: stats,
    })
}

const MAX_MESSAGE_BYTES: usize = 65_536; // 64 KiB

pub async fn handle_run(
    State(state): State<AppState>,
    Json(body): Json<RunBody>,
) -> Result<Json<JobIdResponse>, (StatusCode, Json<ErrorResponse>)> {
    if body.message.len() > MAX_MESSAGE_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(ErrorResponse {
                error: format!("message exceeds {MAX_MESSAGE_BYTES} byte limit"),
            }),
        ));
    }

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

    if let Some(ref db) = state.db {
        db.insert(&job);
    }
    {
        let mut store = state.jobs.write().await;
        store.insert(job_id.clone(), job);
        evict_oldest_finished(&mut store);
    }

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
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<JobIdResponse>, (StatusCode, Json<ErrorResponse>)> {
    let secret_key = format!(
        "KERNEX_WEBHOOK_SECRET_{}",
        event.to_uppercase().replace('-', "_")
    );
    if let Ok(secret) = std::env::var(&secret_key) {
        verify_webhook_hmac(&headers, &body, &secret)?;
    }

    let webhook_body: WebhookBody = serde_json::from_slice(&body).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: format!("invalid JSON: {e}"),
            }),
        )
    })?;

    let message = webhook_body
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

fn verify_webhook_hmac(
    headers: &HeaderMap,
    body: &[u8],
    secret: &str,
) -> Result<(), (StatusCode, Json<ErrorResponse>)> {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    type HmacSha256 = Hmac<Sha256>;

    let sig_header = headers
        .get("x-hub-signature-256")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "missing X-Hub-Signature-256 header".to_string(),
                }),
            )
        })?;

    let hex = sig_header.strip_prefix("sha256=").ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid signature format".to_string(),
            }),
        )
    })?;

    let sig_bytes = hex_decode(hex).ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "invalid signature encoding".to_string(),
            }),
        )
    })?;

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "HMAC key error".to_string(),
            }),
        )
    })?;
    mac.update(body);
    mac.verify_slice(&sig_bytes).map_err(|_| {
        (
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "signature mismatch".to_string(),
            }),
        )
    })
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_response_serializes_with_jobs() {
        let r = HealthResponse {
            status: "ok",
            version: "0.4.0",
            jobs: JobStats {
                queued: 1,
                running: 2,
                done: 3,
                flagged: 0,
                failed: 1,
                total: 7,
            },
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"version\":\"0.4.0\""));
        assert!(json.contains("\"jobs\":{"));
        assert!(json.contains("\"total\":7"));
    }

    #[test]
    fn job_stats_default_is_zero() {
        let s = JobStats::default();
        assert_eq!(s.total, 0);
        assert_eq!(s.queued, 0);
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
    fn message_size_limit_constant() {
        assert_eq!(MAX_MESSAGE_BYTES, 65_536);
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

    #[test]
    fn hex_decode_valid() {
        assert_eq!(hex_decode("deadbeef"), Some(vec![0xde, 0xad, 0xbe, 0xef]));
        assert_eq!(hex_decode(""), Some(vec![]));
        assert_eq!(hex_decode("00ff"), Some(vec![0x00, 0xff]));
    }

    #[test]
    fn hex_decode_invalid() {
        assert_eq!(hex_decode("xyz"), None); // odd length
        assert_eq!(hex_decode("zz"), None); // non-hex chars
        assert_eq!(hex_decode("abc"), None); // odd length
    }

    #[test]
    fn verify_webhook_hmac_missing_header() {
        let headers = HeaderMap::new();
        let result = verify_webhook_hmac(&headers, b"body", "secret");
        assert!(result.is_err());
        let (code, _) = result.unwrap_err();
        assert_eq!(code, StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn verify_webhook_hmac_valid_signature() {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        type HmacSha256 = Hmac<Sha256>;

        let secret = "test-secret";
        let body = b"hello world";

        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(body);
        let sig = mac.finalize().into_bytes();
        let hex_sig = sig.iter().map(|b| format!("{b:02x}")).collect::<String>();

        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature-256",
            format!("sha256={hex_sig}").parse().unwrap(),
        );

        let result = verify_webhook_hmac(&headers, body, secret);
        assert!(result.is_ok());
    }

    #[test]
    fn verify_webhook_hmac_wrong_signature() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-hub-signature-256",
            "sha256=0000000000000000000000000000000000000000000000000000000000000000"
                .parse()
                .unwrap(),
        );
        let result = verify_webhook_hmac(&headers, b"body", "secret");
        assert!(result.is_err());
        let (code, _) = result.unwrap_err();
        assert_eq!(code, StatusCode::UNAUTHORIZED);
    }
}
