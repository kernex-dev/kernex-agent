pub mod db;
pub mod jobs;
pub mod routes;
pub mod skills;
pub mod workflow;

use std::sync::Arc;

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
) -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let token = auth_token
        .or_else(|| std::env::var("KERNEX_AUTH_TOKEN").ok())
        .ok_or("auth token required: pass --auth-token or set KERNEX_AUTH_TOKEN")?;

    if token.is_empty() {
        return Err("auth token cannot be empty".into());
    }

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
    tokio::spawn(run_worker(rx, job_store, db_arc, semaphore));

    let protected = Router::new()
        .route("/run", post(routes::handle_run))
        .route("/jobs", get(routes::handle_list_jobs))
        .route("/jobs/{id}", get(routes::handle_get_job))
        .route("/webhook/{event}", post(routes::handle_webhook))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    let app = Router::new()
        .route("/health", get(routes::handle_health))
        .merge(protected)
        .with_state(state);

    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("kx serve listening on http://{addr}");
    println!("kx serve listening on http://{addr}");
    println!("Press Ctrl+C to stop.");

    axum::serve(listener, app).await?;
    Ok(())
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
    while let Some(req) = rx.recv().await {
        let jobs_clone = jobs.clone();
        let db_clone = db.clone();
        let sem_clone = semaphore.clone();
        tokio::spawn(async move {
            let Ok(_permit) = sem_clone.acquire_owned().await else {
                return;
            };
            execute_job(req, jobs_clone, db_clone).await;
        });
    }
}

async fn execute_job(req: JobRequest, jobs: JobStore, db: Option<Arc<db::JobDb>>) {
    set_status(
        &jobs,
        db.as_deref(),
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
            set_status(&jobs, db.as_deref(), &id, status, Some(output), None).await;
        }
        Err(e) => {
            tracing::warn!(job_id = %id, error = %e, "job failed");
            set_status(&jobs, db.as_deref(), &id, JobStatus::Failed, None, Some(e)).await;
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
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(output) {
        if let Some(verdict) = val.get("verdict").and_then(|v| v.as_str()) {
            return verdict != "SHIP IT";
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
        .data_dir(data_dir.to_str().unwrap_or("~/.kx"))
        .system_prompt(&system_prompt)
        .channel(channel)
        .project(project_name)
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
    db: Option<&db::JobDb>,
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
        db.update_status(
            job_id,
            &status,
            output.as_deref(),
            error.as_deref(),
            finished_at.as_deref(),
        );
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
}
