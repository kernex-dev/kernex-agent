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
    pub max_tokens: Option<u32>,
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

/// A queued or running job older than this, or a stretch this long with
/// pending work and no completion at all, degrades `/health`.
///
/// Sized so a slow agent run does not trip it while a wedged worker or a
/// provider that never answers shows up within a few container healthcheck
/// intervals.
pub const STALE_AFTER_SECS: u64 = 900;

/// Set when a queued or running job has been pending longer than
/// [`STALE_AFTER_SECS`].
pub const FLAG_STALE_PENDING_JOB: &str = "stale_pending_job";

/// Set alongside [`FLAG_STALE_PENDING_JOB`] when nothing at all reached a
/// terminal status within [`STALE_AFTER_SECS`]. One stuck job raises only
/// the first flag; both together mean the daemon, not one job, is wedged.
pub const FLAG_STALE_COMPLETION: &str = "stale_completion";

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// `"ok"` while `flags` is empty, `"degraded"` otherwise. Never absent,
    /// so existing consumers keep the field they already read.
    pub status: &'static str,
    pub version: &'static str,
    pub jobs: JobStats,
    /// Most recent terminal transition (done, flagged or failed) still held
    /// in the in-memory store. `None` when nothing has finished since this
    /// process started, which is also the state of a freshly booted daemon.
    pub last_job_completed_at: Option<String>,
    /// The staleness threshold in effect, so a caller can reason about the
    /// flags without hardcoding the constant.
    pub stale_after_secs: u64,
    /// Explicit staleness reasons. Empty means healthy. Named flags rather
    /// than a binary up/down so an operator can tell WHY the daemon
    /// degraded without correlating logs.
    pub flags: Vec<&'static str>,
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

    // `iso_timestamp` is fixed-width UTC, so these compare chronologically
    // as plain strings. `evict_oldest_finished` already relies on the same
    // property.
    let last_job_completed_at = store
        .values()
        .filter(|j| {
            matches!(
                j.status,
                JobStatus::Done | JobStatus::Flagged | JobStatus::Failed
            )
        })
        .filter_map(|j| j.finished_at.as_deref())
        .max();
    let oldest_pending = store
        .values()
        .filter(|j| matches!(j.status, JobStatus::Queued | JobStatus::Running))
        .map(|j| j.created_at.as_str())
        .min();

    let flags = health_flags(
        last_job_completed_at,
        oldest_pending,
        &utils::iso_timestamp_at(utils::unix_now_secs().saturating_sub(STALE_AFTER_SECS)),
    );

    Json(HealthResponse {
        status: if flags.is_empty() { "ok" } else { "degraded" },
        version: env!("CARGO_PKG_VERSION"),
        jobs: stats,
        last_job_completed_at: last_job_completed_at.map(str::to_string),
        stale_after_secs: STALE_AFTER_SECS,
        flags,
    })
}

/// Decide which staleness flags apply, given the newest terminal timestamp,
/// the oldest still-pending job's creation time, and the ISO cutoff below
/// which a timestamp counts as stale.
///
/// Degradation is anchored on one fact: work that went in and has not come
/// out. That is the shape a dead or hung provider takes from the outside,
/// and it keeps the two false positives that matter off the endpoint. An
/// idle daemon stays healthy, because with nothing pending there is nothing
/// to be late. A freshly booted daemon working its first job stays healthy
/// too, because that job has not yet aged past the threshold.
fn health_flags(
    last_completed: Option<&str>,
    oldest_pending: Option<&str>,
    cutoff: &str,
) -> Vec<&'static str> {
    let mut flags = Vec::new();
    let Some(oldest_pending) = oldest_pending else {
        return flags;
    };
    if oldest_pending >= cutoff {
        return flags;
    }
    flags.push(FLAG_STALE_PENDING_JOB);

    // A single stuck job while others still finish is one bad job. Nothing
    // finishing at all is the daemon.
    let no_recent_completion = match last_completed {
        Some(ts) => ts < cutoff,
        None => true,
    };
    if no_recent_completion {
        flags.push(FLAG_STALE_COMPLETION);
    }
    flags
}

const MAX_MESSAGE_BYTES: usize = 65_536; // 64 KiB

/// A `project` or `workflow` value must be a single safe path component.
/// Both feed directly into filesystem path construction (`data_dir_for`
/// joins `project`; the workflow loader joins `<workflow>.toml`), so an
/// unvalidated value is a path-escape primitive even behind bearer auth:
/// `"../../etc"` would redirect the data dir or read an arbitrary `.toml`.
/// Accept exactly one normal path component and nothing else (no separators,
/// no `.`/`..`, no absolute paths, no control bytes).
fn is_safe_path_segment(s: &str) -> bool {
    if s.is_empty() || s.len() > 128 {
        return false;
    }
    if s.bytes().any(|b| b == 0 || b.is_ascii_control()) {
        return false;
    }
    // Reject both separators explicitly: on Unix `\` is a legal filename byte,
    // so Path::components would accept `a\b` as one component. Rejecting it
    // here keeps the guard platform-independent and blocks Windows-style
    // separators regardless of the host.
    if s.contains('/') || s.contains('\\') {
        return false;
    }
    let mut comps = std::path::Path::new(s).components();
    matches!(
        (comps.next(), comps.next()),
        (Some(std::path::Component::Normal(c)), None) if c == std::ffi::OsStr::new(s)
    )
}

/// `channel` is a free-form label persisted to the job store and used as the
/// runtime channel; it is not a filesystem path (webhook channels carry a
/// `/`), so it gets a lighter guard: bounded length, no control bytes, no
/// `..` traversal token.
fn is_safe_channel(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && !s.contains("..")
        && s.bytes().all(|b| b != 0 && !b.is_ascii_control())
}

fn bad_request(msg: &str) -> (StatusCode, Json<ErrorResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ErrorResponse {
            error: msg.to_string(),
        }),
    )
}

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

    // Validate caller-supplied names before they reach path construction or
    // the job store. project and workflow are path components; channel is a
    // stored label.
    if let Some(ref project) = body.project {
        if !is_safe_path_segment(project) {
            return Err(bad_request(
                "invalid project name (expected a single path component, no separators or traversal)",
            ));
        }
    }
    if let Some(ref workflow) = body.workflow {
        if !is_safe_path_segment(workflow) {
            return Err(bad_request(
                "invalid workflow name (expected a single path component, no separators or traversal)",
            ));
        }
    }
    if let Some(ref channel) = body.channel {
        if !is_safe_channel(channel) {
            return Err(bad_request("invalid channel name"));
        }
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
        // SQLite writes are sync and hold a sync Mutex across fsync; running
        // them inline blocks the tokio worker for the entire I/O. Punt to
        // spawn_blocking so other concurrent /run requests aren't serialized
        // on this lock.
        let db = db.clone();
        let job_for_insert = job.clone();
        tokio::task::spawn_blocking(move || db.insert(&job_for_insert))
            .await
            .ok();
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
        max_tokens: body.max_tokens.or(state.default_flags.max_tokens),
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
    // Reject malformed event names up front (no `..`, no uppercase, no path
    // separators) so attacker-controlled segments cannot leak into the
    // env-var lookup or the channel string written to the DB.
    if !is_valid_webhook_event(&event) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "invalid event name (expected /^[a-z0-9-]{1,64}$/)".to_string(),
            }),
        ));
    }

    // Fail-closed: every webhook event must have an HMAC secret configured.
    // Without this guard, a bearer-authenticated caller could forge any event
    // by picking a segment that has no KERNEX_WEBHOOK_SECRET_<EVENT> set, and
    // the request would be accepted with no per-event verification.
    let secret_key = format!(
        "KERNEX_WEBHOOK_SECRET_{}",
        event.to_uppercase().replace('-', "_")
    );
    let secret = std::env::var(&secret_key).map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: format!("webhook event '{event}' is not configured ({secret_key} unset)"),
            }),
        )
    })?;
    verify_webhook_hmac(&headers, &body, &secret)?;

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
            // Namespace webhook channels under a fixed prefix so that
            // attacker-influenced `event` values cannot collide with
            // operator-chosen channels used by /run or kx dev. The runtime
            // uses `channel` as a memory/recall key; sharing a channel
            // means messages cross over.
            channel: Some(format!("webhook/{event}")),
            max_tokens: None,
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

    let sig_bytes = utils::hex_decode(hex).ok_or_else(|| {
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

/// True when `event` is a well-formed webhook segment: lowercase alphanumeric
/// plus hyphen, 1-64 bytes. Used to reject path inputs that could escape into
/// the env-var lookup, the SQLite channel column, or just unbounded length.
fn is_valid_webhook_event(event: &str) -> bool {
    if event.is_empty() || event.len() > 64 {
        return false;
    }
    event
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- /health staleness -------------------------------------------
    //
    // `/health` used to hardcode `status: "ok"`, so a daemon whose provider
    // had died reported healthy for as long as it stayed up. These pin the
    // conditions under which it now degrades, and the two idle cases that
    // must NOT degrade.

    const CUTOFF: &str = "2026-09-03T12:00:00Z";
    const BEFORE_CUTOFF: &str = "2026-09-03T11:00:00Z";
    const AFTER_CUTOFF: &str = "2026-09-03T13:00:00Z";

    #[test]
    fn health_flags_empty_when_nothing_is_pending() {
        // Idle daemon, last job finished hours ago. Nothing is late,
        // because nothing is owed.
        assert!(health_flags(Some(BEFORE_CUTOFF), None, CUTOFF).is_empty());
        assert!(health_flags(None, None, CUTOFF).is_empty());
    }

    #[test]
    fn health_flags_empty_for_a_fresh_pending_job() {
        // First job on a just-booted daemon: pending, nothing has ever
        // completed, and that is normal.
        assert!(health_flags(None, Some(AFTER_CUTOFF), CUTOFF).is_empty());
    }

    #[test]
    fn health_flags_stale_pending_job_alone_when_others_still_finish() {
        // One job wedged, but the daemon is still completing work.
        let flags = health_flags(Some(AFTER_CUTOFF), Some(BEFORE_CUTOFF), CUTOFF);
        assert_eq!(flags, vec![FLAG_STALE_PENDING_JOB]);
    }

    #[test]
    fn health_flags_both_when_nothing_completes_at_all() {
        // Work went in, nothing came out, nothing else finished either.
        let flags = health_flags(Some(BEFORE_CUTOFF), Some(BEFORE_CUTOFF), CUTOFF);
        assert_eq!(flags, vec![FLAG_STALE_PENDING_JOB, FLAG_STALE_COMPLETION]);

        // Same, on a daemon that has never completed anything.
        let flags = health_flags(None, Some(BEFORE_CUTOFF), CUTOFF);
        assert_eq!(flags, vec![FLAG_STALE_PENDING_JOB, FLAG_STALE_COMPLETION]);
    }

    #[test]
    fn health_cutoff_is_derived_from_the_threshold() {
        // The cutoff `handle_health` builds must be STALE_AFTER_SECS in the
        // past, and must compare against ISO timestamps as a plain string.
        let now = utils::unix_now_secs();
        let cutoff = utils::iso_timestamp_at(now.saturating_sub(STALE_AFTER_SECS));
        let just_now = utils::iso_timestamp_at(now);
        let long_ago = utils::iso_timestamp_at(now.saturating_sub(STALE_AFTER_SECS * 2));
        assert!(just_now.as_str() > cutoff.as_str());
        assert!(long_ago.as_str() < cutoff.as_str());
    }

    #[test]
    fn webhook_event_validator_accepts_well_formed() {
        assert!(is_valid_webhook_event("github"));
        assert!(is_valid_webhook_event("pr-review"));
        assert!(is_valid_webhook_event("issue-comment-1"));
        assert!(is_valid_webhook_event("a"));
    }

    #[test]
    fn webhook_event_validator_rejects_malformed() {
        assert!(!is_valid_webhook_event(""));
        assert!(!is_valid_webhook_event(".."));
        assert!(!is_valid_webhook_event("Github")); // uppercase
        assert!(!is_valid_webhook_event("path/traversal"));
        assert!(!is_valid_webhook_event("with space"));
        assert!(!is_valid_webhook_event("ctrl\x00char"));
        assert!(!is_valid_webhook_event(&"a".repeat(65))); // > 64 chars
    }

    #[test]
    fn path_segment_validator_accepts_real_project_names() {
        // Real Hurtado project dir names: mixed case, hyphens, digits.
        assert!(is_safe_path_segment("visualaudit-ops"));
        assert!(is_safe_path_segment("GEOAutopilot"));
        assert!(is_safe_path_segment("Doli-Producer"));
        assert!(is_safe_path_segment("repo_123"));
        assert!(is_safe_path_segment("a"));
    }

    #[test]
    fn path_segment_validator_rejects_traversal_and_separators() {
        assert!(!is_safe_path_segment(""));
        assert!(!is_safe_path_segment("."));
        assert!(!is_safe_path_segment(".."));
        assert!(!is_safe_path_segment("../../etc"));
        assert!(!is_safe_path_segment("../../../../tmp/x")); // ASEC-01 payload
        assert!(!is_safe_path_segment("a/b"));
        assert!(!is_safe_path_segment("a\\b"));
        assert!(!is_safe_path_segment("/abs"));
        assert!(!is_safe_path_segment("trailing/"));
        assert!(!is_safe_path_segment("./rel"));
        assert!(!is_safe_path_segment("nul\0byte"));
        assert!(!is_safe_path_segment(&"a".repeat(129))); // > 128
    }

    #[test]
    fn channel_validator_allows_labels_rejects_control_and_traversal() {
        assert!(is_safe_channel("cli"));
        assert!(is_safe_channel("webhook/github")); // slashes allowed for labels
        assert!(!is_safe_channel(""));
        assert!(!is_safe_channel("a/../b"));
        assert!(!is_safe_channel("ctrl\x00"));
        assert!(!is_safe_channel(&"a".repeat(129)));
    }

    #[test]
    fn health_response_serializes_with_jobs() {
        let r = HealthResponse {
            status: "ok",
            version: env!("CARGO_PKG_VERSION"),
            jobs: JobStats {
                queued: 1,
                running: 2,
                done: 3,
                flagged: 0,
                failed: 1,
                total: 7,
            },
            last_job_completed_at: Some("2026-09-03T12:00:00Z".to_string()),
            stale_after_secs: STALE_AFTER_SECS,
            flags: Vec::new(),
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains(&format!("\"version\":\"{}\"", env!("CARGO_PKG_VERSION"))));
        assert!(json.contains("\"jobs\":{"));
        assert!(json.contains("\"total\":7"));
        assert!(json.contains("\"last_job_completed_at\":\"2026-09-03T12:00:00Z\""));
        assert!(json.contains(&format!("\"stale_after_secs\":{STALE_AFTER_SECS}")));
        assert!(json.contains("\"flags\":[]"));
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
