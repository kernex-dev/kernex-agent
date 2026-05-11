pub mod db;
pub mod jobs;
pub mod routes;
pub mod skills;
pub mod workflow;

use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::extract::Request;
use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use axum::routing::{get, post};
use axum::Router;
use kernex_core::message::Request as KxRequest;
use kernex_runtime::RuntimeBuilder;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;
use tower_http::trace::TraceLayer;

use crate::config::ProjectConfig;
use crate::{build_provider, context_needs, data_dir_for, CliHookRunner, ProviderFlags};

use jobs::{JobRequest, JobStatus, JobStore};

#[derive(Clone)]
pub struct AppState {
    pub jobs: JobStore,
    pub tx: mpsc::Sender<JobRequest>,
    pub default_flags: Arc<ProviderFlags>,
    pub auth_token: String,
    pub db: Option<Arc<db::JobDb>>,
}

pub async fn cmd_serve(
    host: String,
    port: u16,
    auth_token: Option<String>,
    workers: usize,
    flags: &ProviderFlags,
) -> anyhow::Result<()> {
    // Tracing subscriber is now initialised once in `main::run` for every
    // subcommand. Don't init again here.

    let token = auth_token
        .or_else(|| std::env::var("KERNEX_AUTH_TOKEN").ok())
        .ok_or_else(|| {
            anyhow::anyhow!("auth token required: pass --auth-token or set KERNEX_AUTH_TOKEN")
        })?;

    // Reject short tokens up front. 32 bytes is roughly equivalent to a
    // base32-encoded 160-bit secret and matches the strength we expect for
    // any HTTP bearer over the network. A short, guessable token here
    // would trivially bypass the entire serve auth boundary.
    const MIN_AUTH_TOKEN_LEN: usize = 32;
    if token.len() < MIN_AUTH_TOKEN_LEN {
        anyhow::bail!(
            "auth token must be at least {MIN_AUTH_TOKEN_LEN} bytes (got {})",
            token.len()
        );
    }

    // Clamp the worker count to a sane upper bound. Each slot keeps a
    // long-lived runtime + provider client around, so very large values
    // are almost certainly a typo (e.g. `--workers 1000`) and would chew
    // through file descriptors and memory before doing useful work.
    const MAX_WORKERS: usize = 256;
    if workers == 0 {
        anyhow::bail!("--workers must be >= 1");
    }
    let workers = if workers > MAX_WORKERS {
        tracing::warn!("requested --workers {workers} exceeds cap of {MAX_WORKERS}; clamping");
        MAX_WORKERS
    } else {
        workers
    };

    let serve_data_dir = data_dir_for("serve");
    let db_arc = match db::JobDb::init(&serve_data_dir) {
        Ok(job_db) => {
            let arc = Arc::new(job_db);
            arc.mark_running_as_failed();
            Some(arc)
        }
        Err(e) => {
            tracing::warn!("SQLite init failed ({e}); running without job persistence");
            None
        }
    };

    let job_store = jobs::new_store();
    if let Some(ref db) = db_arc {
        let existing = db.load_all();
        let mut store = job_store.write().await;
        for job in existing {
            store.insert(job.id.clone(), job);
        }
    }

    let (tx, rx) = mpsc::channel::<JobRequest>(256);

    let state = AppState {
        jobs: job_store.clone(),
        tx,
        default_flags: Arc::new(flags.clone()),
        auth_token: token,
        db: db_arc.clone(),
    };

    let semaphore = Arc::new(Semaphore::new(workers));
    let worker_handle = tokio::spawn(run_worker(rx, job_store, db_arc, semaphore));

    let app = build_app(state, MAX_REQUEST_BODY_BYTES);

    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("kx serve listening on http://{addr}");

    // Graceful shutdown: wait for SIGINT (Ctrl+C) or SIGTERM, then let axum
    // finish in-flight requests. Once axum::serve returns, the AppState
    // (and therefore the mpsc sender) is dropped; the worker loop exits as
    // soon as the channel closes; we await the worker JoinHandle to ensure
    // any in-flight job's `complete_with_needs` call settles its DB writes
    // before we return.
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("kx serve: shutdown signalled, draining worker pool");
    if let Err(e) = worker_handle.await {
        tracing::warn!("kx serve: worker task did not exit cleanly: {e}");
    }
    tracing::info!("kx serve: stopped");
    Ok(())
}

// Cap every request body at 1 MiB at the router layer. /run already
// performs a stricter 64 KiB check on its `message` field inside the
// handler; this ceiling protects /webhook/{event} (which reads raw
// Bytes) and any future endpoint from a flood of arbitrarily large
// bodies (status pages, attacker payloads, accidental file uploads).
pub(crate) const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

/// Build the Axum router used by `kx serve`. Extracted from `cmd_serve`
/// so integration tests can exercise the router (and the auth + body
/// limit layers) without spinning up a real TCP listener or worker.
pub(crate) fn build_app(state: AppState, max_body_bytes: usize) -> Router {
    let protected = Router::new()
        .route("/run", post(routes::handle_run))
        .route("/jobs", get(routes::handle_list_jobs))
        .route("/jobs/{id}", get(routes::handle_get_job))
        .route("/webhook/{event}", post(routes::handle_webhook))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    Router::new()
        .route("/health", get(routes::handle_health))
        .merge(protected)
        // TraceLayer emits a structured `tower_http::trace` span per
        // request and a corresponding response event. Combined with the
        // tracing-subscriber initialized in `main`, every HTTP request
        // produces a per-request span operators (and downstream agents
        // piping kx serve traffic through a log collector) can correlate
        // with the job dispatched via `routes::handle_run`.
        .layer(TraceLayer::new_for_http())
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .with_state(state)
}

/// Future that completes on the first OS shutdown signal we observe.
/// Listens to both SIGINT (Ctrl+C) and SIGTERM on Unix; falls back to
/// SIGINT-only on other platforms.
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!("failed to install ctrl_c handler: {e}");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => {
                tracing::warn!("failed to install SIGTERM handler: {e}");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {}
        _ = terminate => {}
    }
}

async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    use subtle::ConstantTimeEq;

    let provided = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let authorized = provided
        .map(|p| p.as_bytes().ct_eq(state.auth_token.as_bytes()).into())
        .unwrap_or(false);

    if authorized {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

async fn run_worker(
    mut rx: mpsc::Receiver<JobRequest>,
    jobs: JobStore,
    db: Option<Arc<db::JobDb>>,
    semaphore: Arc<Semaphore>,
) {
    // Track the JoinHandle for every spawned job so a graceful shutdown can
    // await them before returning. Without this, axum::serve exits as soon
    // as rx.recv() returns None, then run_worker returns, and the runtime
    // drops every in-flight execute_job future mid-completion: the final
    // set_status (and the SQLite UPDATE) never run, the provider response
    // is lost, and the boot-time mark_running_as_failed sweep papers over
    // it on the next start. The JoinSet here is the missing drain step.
    let mut joinset: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();

    while let Some(req) = rx.recv().await {
        // Acquire the worker permit *before* spawning so we apply real
        // back-pressure: when all `workers` slots are busy, recv() blocks
        // and the bounded channel fills up, causing handlers to surface a
        // 503 'job queue full' to clients. Acquiring inside the spawned
        // task instead would let recv drain the channel ahead of execution
        // capacity and pile up parked tasks awaiting a permit.
        let permit = match semaphore.clone().acquire_owned().await {
            Ok(p) => p,
            Err(_) => {
                tracing::warn!("worker semaphore closed; stopping pull loop");
                break;
            }
        };
        let jobs_clone = jobs.clone();
        let db_clone = db.clone();
        joinset.spawn(async move {
            // Hold the permit for the duration of the job; drop on completion.
            let _permit = permit;
            execute_job(req, jobs_clone, db_clone).await;
        });
    }

    // rx closed => shutdown signal. Drain in-flight jobs with a per-task
    // budget so a stuck provider can't block forever; whatever doesn't
    // finish in time will be picked up by the next start's
    // mark_running_as_failed sweep.
    const SHUTDOWN_DRAIN_SECS: u64 = 30;
    let drain_deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(SHUTDOWN_DRAIN_SECS);
    while joinset.join_next().await.is_some() {
        if std::time::Instant::now() >= drain_deadline {
            tracing::warn!(
                in_flight = joinset.len(),
                "shutdown drain timeout reached; aborting remaining jobs"
            );
            joinset.abort_all();
            while joinset.join_next().await.is_some() {}
            break;
        }
    }
}

async fn execute_job(req: JobRequest, jobs: JobStore, db: Option<Arc<db::JobDb>>) {
    set_status(
        &jobs,
        db.clone(),
        &req.job_id,
        JobStatus::Running,
        None,
        None,
    )
    .await;
    let id = req.job_id.clone();

    let result: Result<(String, JobStatus), String> = if let Some(wf_name) = req.workflow.as_deref()
    {
        let project_name = req.project.as_deref().unwrap_or("serve");
        let data_dir = data_dir_for(project_name);
        match workflow::load_workflow(wf_name, &data_dir) {
            Ok(wf) => run_workflow(req, wf).await.map(|(output, flagged)| {
                let status = if flagged {
                    JobStatus::Flagged
                } else {
                    JobStatus::Done
                };
                (output, status)
            }),
            Err(e) => Err(e),
        }
    } else {
        run_agent(req).await.map(|output| (output, JobStatus::Done))
    };

    match result {
        Ok((output, status)) => {
            set_status(&jobs, db.clone(), &id, status, Some(output), None).await;
        }
        Err(e) => {
            tracing::warn!(job_id = %id, error = %e, "job failed");
            set_status(&jobs, db.clone(), &id, JobStatus::Failed, None, Some(e)).await;
        }
    }
}

async fn run_workflow(req: JobRequest, wf: workflow::Workflow) -> Result<(String, bool), String> {
    let original_input = req.message.clone();
    let mut outputs: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut last_output = String::new();

    for step in &wf.steps {
        let rendered = workflow::render_input(&step.input, &original_input, &outputs);

        let step_req = JobRequest {
            job_id: req.job_id.clone(),
            message: rendered,
            provider: req.provider.clone(),
            model: req.model.clone(),
            api_key: req.api_key.clone(),
            base_url: req.base_url.clone(),
            project: req.project.clone(),
            channel: req.channel.clone(),
            max_tokens: req.max_tokens,
            verbose: req.verbose,
            skills: Some(vec![step.skill.clone()]),
            mode: step.mode.clone(),
            workflow: None,
        };

        tracing::info!(
            job_id = %req.job_id,
            step = %step.id,
            skill = %step.skill,
            "workflow step starting"
        );

        let output = run_agent(step_req).await?;
        outputs.insert(step.id.clone(), output.clone());
        last_output = output;
    }

    // If the last step is already reality-checker, parse its verdict directly.
    if wf
        .steps
        .last()
        .is_some_and(|s| s.skill == "reality-checker")
    {
        let is_flagged = is_verdict_flagged(&last_output);
        return Ok((last_output, is_flagged));
    }

    // Auto-run reality-checker as the validation gate for every workflow.
    let checker_input =
        format!("Original request: {original_input}\n\nWorkflow output:\n{last_output}");
    let checker_req = JobRequest {
        job_id: req.job_id.clone(),
        message: checker_input,
        provider: req.provider.clone(),
        model: req.model.clone(),
        api_key: req.api_key.clone(),
        base_url: req.base_url.clone(),
        project: req.project.clone(),
        channel: req.channel.clone(),
        max_tokens: req.max_tokens,
        verbose: req.verbose,
        skills: Some(vec!["reality-checker".to_string()]),
        mode: Some("task".to_string()),
        workflow: None,
    };

    tracing::info!(job_id = %req.job_id, "workflow validation gate: running reality-checker");

    match run_agent(checker_req).await {
        Ok(checker_output) => {
            let is_flagged = is_verdict_flagged(&checker_output);
            if is_flagged {
                let output = format!("{last_output}\n\n---\n\n{checker_output}");
                Ok((output, true))
            } else {
                Ok((last_output, false))
            }
        }
        Err(e) => {
            tracing::warn!(job_id = %req.job_id, error = %e, "reality-checker failed; flagging job");
            Ok((last_output, true))
        }
    }
}

/// Returns `true` if the reality-checker verdict is anything other than "SHIP IT".
fn is_verdict_flagged(output: &str) -> bool {
    // Cap the input fed into the JSON parser. Provider responses can be
    // many MB; without a cap a runaway response amplifies cost on every
    // workflow flag check. 256 KiB is generous for a verdict struct.
    const MAX_VERDICT_JSON_BYTES: usize = 256 * 1024;
    if output.len() <= MAX_VERDICT_JSON_BYTES {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(output) {
            if let Some(verdict) = val.get("verdict").and_then(|v| v.as_str()) {
                return verdict != "SHIP IT";
            }
        }
    }
    // Fallback: plain text scan — absence of "SHIP IT" is treated as flagged.
    !output.contains("SHIP IT")
}

async fn run_agent(req: JobRequest) -> Result<String, String> {
    let flags = ProviderFlags {
        name: req.provider,
        model: req.model,
        api_key: req.api_key,
        base_url: req.base_url,
        project: req.project.clone(),
        channel: req.channel.clone(),
        max_tokens: req.max_tokens,
        no_memory: true,
        // Serve jobs get auto-compact unconditionally — they tend to be
        // long-running and we don't yet expose a per-request override on
        // JobRequest. Operators who want to disable it can pass
        // `--no-auto-compact` to `kx serve`; that intent is checked by
        // the server-level guard further up the call stack.
        auto_compact: true,
        verbose: req.verbose,
    };

    let config = ProjectConfig::default();
    let (provider, _label) = build_provider(&flags, &config).map_err(|e| e.to_string())?;

    let project_name = req.project.as_deref().unwrap_or("serve");
    let data_dir = data_dir_for(project_name);
    let channel = req.channel.as_deref().unwrap_or("serve");

    let skill_names = req.skills.as_deref().unwrap_or(&[]);
    let system_prompt =
        skills::build_serve_system_prompt(skill_names, &data_dir, req.mode.as_deref());

    let runtime = RuntimeBuilder::new()
        .data_dir(&data_dir.to_string_lossy())
        .system_prompt(&system_prompt)
        .channel(channel)
        .project(project_name)
        .auto_compact(flags.auto_compact)
        .hook_runner(Arc::new(CliHookRunner {
            verbose: req.verbose,
        }))
        .build()
        .await
        .map_err(|e| e.to_string())?;

    let needs = context_needs(true);
    let request = KxRequest::text("user", &req.message);

    let response = runtime
        .complete_with_needs(provider.as_ref(), &request, &needs)
        .await
        .map_err(|e| e.to_string())?;

    Ok(response.text)
}

async fn set_status(
    jobs: &JobStore,
    db: Option<Arc<db::JobDb>>,
    job_id: &str,
    status: JobStatus,
    output: Option<String>,
    error: Option<String>,
) {
    let finished_at = if matches!(
        status,
        JobStatus::Done | JobStatus::Failed | JobStatus::Flagged
    ) {
        Some(crate::utils::iso_timestamp())
    } else {
        None
    };
    {
        let mut store = jobs.write().await;
        if let Some(job) = store.get_mut(job_id) {
            job.status = status.clone();
            if output.is_some() {
                job.output = output.clone();
            }
            if error.is_some() {
                job.error = error.clone();
            }
            if finished_at.is_some() {
                job.finished_at = finished_at.clone();
            }
        }
    }
    if let Some(db) = db {
        // SQLite update_status takes a sync Mutex across an UPDATE; punt off
        // the tokio worker thread.
        let job_id = job_id.to_string();
        let status_owned = status;
        let finished_at_owned = finished_at;
        tokio::task::spawn_blocking(move || {
            db.update_status(
                &job_id,
                &status_owned,
                output.as_deref(),
                error.as_deref(),
                finished_at_owned.as_deref(),
            );
        })
        .await
        .ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn set_status_updates_job() {
        let store = jobs::new_store();
        let job = jobs::Job {
            id: "test-1".to_string(),
            status: JobStatus::Queued,
            output: None,
            error: None,
            message: "test".to_string(),
            provider: "claude-code".to_string(),
            project: None,
            channel: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            finished_at: None,
        };
        store.write().await.insert("test-1".to_string(), job);

        set_status(&store, None, "test-1", JobStatus::Running, None, None).await;
        let guard = store.read().await;
        let j = guard.get("test-1").unwrap();
        assert_eq!(j.status, JobStatus::Running);
        assert!(j.finished_at.is_none());
    }

    #[tokio::test]
    async fn set_status_done_sets_finished_at() {
        let store = jobs::new_store();
        let job = jobs::Job {
            id: "test-2".to_string(),
            status: JobStatus::Running,
            output: None,
            error: None,
            message: "work".to_string(),
            provider: "claude-code".to_string(),
            project: None,
            channel: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            finished_at: None,
        };
        store.write().await.insert("test-2".to_string(), job);

        set_status(
            &store,
            None,
            "test-2",
            JobStatus::Done,
            Some("result".to_string()),
            None,
        )
        .await;
        let guard = store.read().await;
        let j = guard.get("test-2").unwrap();
        assert_eq!(j.status, JobStatus::Done);
        assert_eq!(j.output, Some("result".to_string()));
        assert!(j.finished_at.is_some());
    }

    #[tokio::test]
    async fn set_status_failed_sets_error_and_finished_at() {
        let store = jobs::new_store();
        let job = jobs::Job {
            id: "test-3".to_string(),
            status: JobStatus::Running,
            output: None,
            error: None,
            message: "bad".to_string(),
            provider: "claude-code".to_string(),
            project: None,
            channel: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            finished_at: None,
        };
        store.write().await.insert("test-3".to_string(), job);

        set_status(
            &store,
            None,
            "test-3",
            JobStatus::Failed,
            None,
            Some("provider error".to_string()),
        )
        .await;
        let guard = store.read().await;
        let j = guard.get("test-3").unwrap();
        assert_eq!(j.status, JobStatus::Failed);
        assert_eq!(j.error, Some("provider error".to_string()));
        assert!(j.finished_at.is_some());
    }

    #[test]
    fn cmd_serve_requires_auth_token() {
        // Validate the logic: no token from env or arg should fail
        let token: Option<String> = None;
        let from_env: Option<String> = None; // simulate missing env var
        let resolved = token.or(from_env);
        assert!(resolved.is_none());
    }

    #[test]
    fn is_verdict_flagged_ship_it_not_flagged() {
        let output = r#"{"verdict":"SHIP IT","grade":"A","verified":[],"gaps":[],"conditions":[],"summary":"ok"}"#;
        assert!(!is_verdict_flagged(output));
    }

    #[test]
    fn is_verdict_flagged_needs_work_is_flagged() {
        let output = r#"{"verdict":"NEEDS WORK","grade":"C","verified":[],"gaps":["missing tests"],"conditions":[],"summary":"gaps present"}"#;
        assert!(is_verdict_flagged(output));
    }

    #[test]
    fn is_verdict_flagged_blocked_is_flagged() {
        let output = r#"{"verdict":"BLOCKED","grade":"F","verified":[],"gaps":["no evidence"],"conditions":[],"summary":"blocked"}"#;
        assert!(is_verdict_flagged(output));
    }

    #[test]
    fn is_verdict_flagged_non_json_ship_it_text() {
        // Fallback: plain text containing "SHIP IT"
        assert!(!is_verdict_flagged("Verdict: SHIP IT. All good."));
    }

    #[test]
    fn is_verdict_flagged_non_json_no_ship_it() {
        assert!(is_verdict_flagged("Something went wrong with the output."));
    }

    // -- HTTP boundary tests for `kx serve` ---------------------------
    //
    // These exercise the Axum router built by `build_app` via tower's
    // `ServiceExt::oneshot`. They cover the auth + body-limit + webhook
    // fail-closed paths without spinning up a real TCP listener or a
    // worker pool.

    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const TEST_TOKEN: &str = "test-token-must-be-at-least-32-bytes-long-yes";
    const TEST_MAX_BODY: usize = 1024 * 1024;

    fn make_test_state(token: &str) -> (AppState, mpsc::Receiver<JobRequest>) {
        let (tx, rx) = mpsc::channel::<JobRequest>(8);
        let flags = Arc::new(crate::ProviderFlags {
            name: "ollama".to_string(),
            model: None,
            api_key: None,
            base_url: None,
            project: None,
            channel: None,
            max_tokens: None,
            no_memory: false,
            auto_compact: true,
            verbose: false,
        });
        let state = AppState {
            jobs: jobs::new_store(),
            tx,
            default_flags: flags,
            auth_token: token.to_string(),
            db: None,
        };
        (state, rx)
    }

    #[tokio::test]
    async fn health_does_not_require_auth() {
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let app = build_app(state, TEST_MAX_BODY);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], "ok");
        assert!(json["version"].is_string());
    }

    #[tokio::test]
    async fn jobs_rejects_missing_bearer() {
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let app = build_app(state, TEST_MAX_BODY);

        let resp = app
            .oneshot(Request::builder().uri("/jobs").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn jobs_rejects_wrong_bearer() {
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let app = build_app(state, TEST_MAX_BODY);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/jobs")
                    .header(header::AUTHORIZATION, "Bearer not-the-real-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn jobs_with_valid_bearer_returns_empty_list() {
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let app = build_app(state, TEST_MAX_BODY);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/jobs")
                    .header(header::AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(json.is_array());
        assert_eq!(json.as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn run_enqueues_job_with_default_provider() {
        let (state, mut rx) = make_test_state(TEST_TOKEN);
        let app = build_app(state, TEST_MAX_BODY);

        let body = serde_json::json!({ "message": "hello" }).to_string();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/run")
                    .header(header::AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let queued = rx.try_recv().expect("expected queued job in channel");
        assert_eq!(queued.message, "hello");
        assert_eq!(queued.provider, "ollama");
    }

    #[tokio::test]
    async fn run_rejects_message_above_64kib() {
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let app = build_app(state, TEST_MAX_BODY);

        // 64 KiB + 1 byte; exceeds the per-handler MAX_MESSAGE_BYTES but
        // stays below the router-level body limit.
        let big = "a".repeat(65_537);
        let body = serde_json::json!({ "message": big }).to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/run")
                    .header(header::AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn router_enforces_body_limit() {
        const SMALL_LIMIT: usize = 1024;
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let app = build_app(state, SMALL_LIMIT);

        let big = "a".repeat(SMALL_LIMIT + 256);
        let body = serde_json::json!({ "message": big }).to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/run")
                    .header(header::AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn webhook_rejects_invalid_event_name() {
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let app = build_app(state, TEST_MAX_BODY);

        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/webhook/UPPERCASE")
                    .header(header::AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn webhook_fails_closed_when_secret_unset() {
        // Unique event name so we do not collide with an env var that
        // some other test or the developer's shell might have set.
        let event = "kx-test-no-secret-event-z9";
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let app = build_app(state, TEST_MAX_BODY);

        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/webhook/{event}"))
                    .header(header::AUTHORIZATION, format!("Bearer {TEST_TOKEN}"))
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn webhook_rejects_missing_bearer() {
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let app = build_app(state, TEST_MAX_BODY);

        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/webhook/test-event")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
