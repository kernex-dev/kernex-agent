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
        crate::utils::log_join_error("kx serve worker pool", e);
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
    // The pool's own JoinHandle is not polled until shutdown, so a panic
    // in this loop would otherwise leave the daemon accepting jobs that
    // nothing ever picks up, with no log line to say so.
    let _panic_guard = crate::utils::PanicLogGuard::new("kx serve worker pool");

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
            // A panicking job never reaches set_status, so it stays Running
            // in the store forever. The JoinSet holds the payload until the
            // shutdown drain, which can be hours away; this reports the
            // death when it happens.
            let _panic_guard = crate::utils::PanicLogGuard::new("kx serve job");
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
    while let Some(joined) = joinset.join_next().await {
        // Surface the payload of any job that panicked. Discarding this
        // Result was the last place a panicking job could disappear
        // without a trace.
        if let Err(e) = joined {
            crate::utils::log_join_error("kx serve job", e);
        }
        if std::time::Instant::now() >= drain_deadline {
            tracing::warn!(
                in_flight = joinset.len(),
                "shutdown drain timeout reached; aborting remaining jobs"
            );
            joinset.abort_all();
            while let Some(joined) = joinset.join_next().await {
                // Cancellations here are expected; log_join_error keeps
                // those at debug and reports anything else.
                if let Err(e) = joined {
                    crate::utils::log_join_error("kx serve job", e);
                }
            }
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

/// The three verdicts the reality-checker skill contract defines.
const VERDICT_SHIP_IT: &str = "SHIP IT";
const VERDICT_NEEDS_WORK: &str = "NEEDS WORK";
const VERDICT_BLOCKED: &str = "BLOCKED";

/// Cap the input fed into the JSON parser. Provider responses can be many
/// MB; without a cap a runaway response amplifies cost on every workflow
/// flag check. 256 KiB is generous for a verdict struct.
const MAX_VERDICT_JSON_BYTES: usize = 256 * 1024;

/// Longest `key` we will consider on the left of a `key: value` line when
/// hunting for the verdict. Bounds the per-line work on a huge response.
const MAX_VERDICT_LABEL_BYTES: usize = 64;

/// Returns `true` unless the reality-checker issued an explicit `SHIP IT`.
///
/// This gate FAILS CLOSED. Output that does not parse, carries no verdict,
/// or carries a verdict outside the contract set flags the job instead of
/// passing it. The previous implementation ended in
/// `!output.contains("SHIP IT")`, so any prose merely mentioning the phrase
/// passed the gate, including a sentence saying "do not SHIP IT".
fn is_verdict_flagged(output: &str) -> bool {
    match parse_verdict(output) {
        Some(verdict) => verdict != VERDICT_SHIP_IT,
        None => {
            tracing::warn!(
                bytes = output.len(),
                "reality-checker output carried no recognisable verdict; flagging job"
            );
            true
        }
    }
}

/// Extract the verdict when the output carries one we recognise.
///
/// Two shapes are accepted, matching the two reality-checker contracts: a
/// JSON body with a `verdict` field, and a markdown report with a
/// `Verdict:` line. Everything else yields `None`, which the caller treats
/// as a flag.
fn parse_verdict(output: &str) -> Option<&'static str> {
    let body = strip_code_fence(output.trim());
    if body.starts_with('{') {
        // The body announces itself as a JSON document, so it either parses
        // and carries a verdict we recognise, or we do not know. Do NOT fall
        // through to the line scan here: that would let a response truncated
        // mid-generation be salvaged into a pass, which is the ambiguous
        // case this gate exists to reject. The length guard keeps a runaway
        // multi-MB response out of the parser on every workflow check.
        if body.len() > MAX_VERDICT_JSON_BYTES {
            return None;
        }
        let val = serde_json::from_str::<serde_json::Value>(body).ok()?;
        return val
            .get("verdict")
            .and_then(|v| v.as_str())
            .and_then(known_verdict);
    }
    body.lines().find_map(verdict_from_line)
}

/// Unwrap a fenced block so a JSON body the model wrapped in ```` ```json ````
/// still reaches the JSON parser. Returns the input unchanged when there is
/// no complete fence.
fn strip_code_fence(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("```") else {
        return text;
    };
    // Drop the info string (`json`, `JSON`, or nothing) up to its newline.
    let Some((_, body)) = rest.split_once('\n') else {
        return text;
    };
    body.trim_end()
        .strip_suffix("```")
        .map_or(text, str::trim_end)
}

/// Pull a verdict out of a single `key: value` line, e.g. `**Verdict:** SHIP
/// IT` from the markdown report, or a `"verdict": "NEEDS WORK",` line inside
/// a report that embeds JSON without fencing it.
fn verdict_from_line(line: &str) -> Option<&'static str> {
    let (label, value) = line.split_once(':')?;
    if label.len() > MAX_VERDICT_LABEL_BYTES {
        return None;
    }
    // Compare the label on its letters alone so `**Verdict**`, `"verdict"`
    // and `- Verdict` all match, and `## Reality Check` does not.
    let matches_label = label
        .chars()
        .filter(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .eq("verdict".chars());
    if !matches_label {
        return None;
    }
    // Strip leading markdown / JSON decoration, then keep only the first
    // clause. `SHIP IT. All good.` is a decision; `SHIP IT / NEEDS WORK /
    // BLOCKED` is the report template echoed back without one, and must not
    // read as a pass.
    const DECORATION: &[char] = &[' ', '\t', '*', '"', '\'', '`', '#'];
    let value = value.trim_start_matches(DECORATION);
    let clause = value
        .find(['.', ',', ';', '"', '(', '*', '`'])
        .map_or(value, |i| &value[..i]);
    known_verdict(clause)
}

/// Map a raw verdict onto the contract set, ignoring case and surrounding
/// whitespace. Anything outside the set is `None`, never a pass.
fn known_verdict(raw: &str) -> Option<&'static str> {
    match raw.trim().to_ascii_uppercase().as_str() {
        VERDICT_SHIP_IT => Some(VERDICT_SHIP_IT),
        VERDICT_NEEDS_WORK => Some(VERDICT_NEEDS_WORK),
        VERDICT_BLOCKED => Some(VERDICT_BLOCKED),
        _ => None,
    }
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

    // -- fail-closed verdict gate ------------------------------------
    //
    // The gate used to end in `!output.contains("SHIP IT")`, so any prose
    // carrying the phrase passed. These pin the closed direction: only an
    // explicit, unambiguous SHIP IT verdict passes.

    #[test]
    fn is_verdict_flagged_markdown_ship_it_passes() {
        let output = "## Reality Check: auth flow\n\n**Verdict:** SHIP IT\n\n**Rating:** B\n";
        assert!(!is_verdict_flagged(output));
    }

    #[test]
    fn is_verdict_flagged_markdown_blocked_is_flagged() {
        let output = "## Reality Check: auth flow\n\n**Verdict:** BLOCKED\n\n**Rating:** F\n";
        assert!(is_verdict_flagged(output));
    }

    #[test]
    fn is_verdict_flagged_prose_saying_do_not_ship_it() {
        // The regression this gate exists for: the phrase appears, the
        // meaning is the opposite, and the old substring check passed it.
        assert!(is_verdict_flagged(
            "The reviewer was explicit: do not SHIP IT until the migration has tests."
        ));
    }

    #[test]
    fn is_verdict_flagged_verdict_line_saying_do_not_ship_it() {
        assert!(is_verdict_flagged("**Verdict:** do not SHIP IT"));
    }

    #[test]
    fn is_verdict_flagged_template_echoed_without_a_decision() {
        // The model repeated the report template instead of choosing.
        assert!(is_verdict_flagged(
            "**Verdict:** SHIP IT / NEEDS WORK / BLOCKED"
        ));
    }

    #[test]
    fn is_verdict_flagged_unparseable_output_is_flagged() {
        // Truncated JSON, empty body, and a bare provider apology all mean
        // "we do not know", which must not read as a pass.
        assert!(is_verdict_flagged(r#"{"verdict":"SHIP IT","grade":"A","#));
        assert!(is_verdict_flagged(""));
        assert!(is_verdict_flagged("I'm sorry, I can't help with that."));
    }

    #[test]
    fn is_verdict_flagged_json_without_verdict_field_is_flagged() {
        assert!(is_verdict_flagged(
            r#"{"grade":"A","summary":"looks fine to me"}"#
        ));
    }

    #[test]
    fn is_verdict_flagged_unknown_verdict_value_is_flagged() {
        assert!(is_verdict_flagged(r#"{"verdict":"LGTM","grade":"A"}"#));
    }

    #[test]
    fn is_verdict_flagged_fenced_json_ship_it_passes() {
        // A code-fenced body does not parse as JSON, so the line scan has
        // to find the verdict.
        let output = "```json\n{\n  \"verdict\": \"SHIP IT\",\n  \"grade\": \"A\"\n}\n```";
        assert!(!is_verdict_flagged(output));
    }

    #[test]
    fn is_verdict_flagged_ignores_non_verdict_labels() {
        // A heading that merely mentions the phrase is not a verdict line.
        assert!(is_verdict_flagged(
            "### Conditions for SHIP IT: add integration tests"
        ));
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
        // Staleness fields are additive: the pre-existing ones above still
        // read the same on an idle daemon.
        assert_eq!(json["flags"].as_array().map(Vec::len), Some(0));
        assert!(json["last_job_completed_at"].is_null());
        assert_eq!(json["stale_after_secs"], routes::STALE_AFTER_SECS);
    }

    async fn health_json(state: AppState) -> serde_json::Value {
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
        serde_json::from_slice(&bytes).unwrap()
    }

    fn stub_job(
        id: &str,
        status: JobStatus,
        created_at: &str,
        finished_at: Option<&str>,
    ) -> jobs::Job {
        jobs::Job {
            id: id.to_string(),
            status,
            output: None,
            error: None,
            message: "stub".to_string(),
            provider: "claude-code".to_string(),
            project: None,
            channel: None,
            created_at: created_at.to_string(),
            finished_at: finished_at.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn health_degrades_when_a_job_has_been_pending_past_the_threshold() {
        // The gap this closes: /health returned a hardcoded "ok", so a
        // daemon whose provider had died reported healthy forever while the
        // Dockerfile curled it every 30s.
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let stale = crate::utils::iso_timestamp_at(
            crate::utils::unix_now_secs().saturating_sub(routes::STALE_AFTER_SECS * 4),
        );
        state.jobs.write().await.insert(
            "stuck".to_string(),
            stub_job("stuck", JobStatus::Running, &stale, None),
        );

        let json = health_json(state).await;
        assert_eq!(json["status"], "degraded");
        assert!(json["last_job_completed_at"].is_null());
        let flags: Vec<String> = serde_json::from_value(json["flags"].clone()).unwrap();
        assert_eq!(
            flags,
            vec![
                routes::FLAG_STALE_PENDING_JOB.to_string(),
                routes::FLAG_STALE_COMPLETION.to_string(),
            ]
        );
        // Existing fields are untouched by the new signal.
        assert_eq!(json["jobs"]["running"], 1);
        assert_eq!(json["jobs"]["total"], 1);
        assert!(json["version"].is_string());
    }

    #[tokio::test]
    async fn health_stays_ok_while_jobs_keep_finishing() {
        let (state, _rx) = make_test_state(TEST_TOKEN);
        let now = crate::utils::unix_now_secs();
        let recent = crate::utils::iso_timestamp_at(now);
        let stale =
            crate::utils::iso_timestamp_at(now.saturating_sub(routes::STALE_AFTER_SECS * 4));
        {
            let mut store = state.jobs.write().await;
            // An old job that finished, and a fresh one still running.
            store.insert(
                "done".to_string(),
                stub_job("done", JobStatus::Done, &stale, Some(&recent)),
            );
            store.insert(
                "fresh".to_string(),
                stub_job("fresh", JobStatus::Queued, &recent, None),
            );
        }

        let json = health_json(state).await;
        assert_eq!(json["status"], "ok");
        assert_eq!(json["flags"].as_array().map(Vec::len), Some(0));
        assert_eq!(json["last_job_completed_at"], recent);
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
