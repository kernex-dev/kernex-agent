//! Stage 2 RESOLVE — combine user options + DETECT into a typed plan.
//!
//! Behavior per E-resolve-1..5.

use std::path::PathBuf;

use chrono::Utc;
use kernex_adapter_core::Detection;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::install::audit::{AuditEvent, AuditWriter, EventStatus, Stage};
use crate::install::preset::resolve_preset;

use super::{InstallError, InstallOptions};

/// Output of RESOLVE consumed by BACKUP and APPLY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallPlan {
    /// Logical agent identifier (e.g. "claude-code"). Mapped to an
    /// `AdapterId` at registry lookup time.
    pub agent: String,
    /// Components from the resolved preset (e.g. `["claude-md", "mcp-json"]`).
    pub components: Vec<String>,
    /// Per-component absolute target paths under `$HOME`.
    pub target_paths: Vec<(String, PathBuf)>,
}

pub async fn run(
    opts: &InstallOptions,
    _detection: &Detection,
    audit: &AuditWriter,
) -> Result<InstallPlan, InstallError> {
    let started = Utc::now();
    audit
        .emit(AuditEvent {
            event: "stage.resolve.start".to_string(),
            stage: Stage::Resolve,
            status: EventStatus::Success,
            started_at: started,
            ended_at: None,
            duration_ms: None,
            payload: json!({"agent": &opts.agent, "preset": &opts.preset}),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    let preset = resolve_preset(&opts.preset)?;
    let plan = build_plan(opts, preset.components)?;

    let ended = Utc::now();
    audit
        .emit(AuditEvent {
            event: "stage.resolve.end".to_string(),
            stage: Stage::Resolve,
            status: EventStatus::Success,
            started_at: started,
            ended_at: Some(ended),
            duration_ms: Some((ended - started).num_milliseconds().max(0) as u64),
            payload: serde_json::to_value(&plan).unwrap_or(serde_json::Value::Null),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    Ok(plan)
}

fn build_plan(opts: &InstallOptions, components: Vec<String>) -> Result<InstallPlan, InstallError> {
    let target_paths = components
        .iter()
        .map(|component| {
            let path = component_path(&opts.home, component)?;
            Ok((component.clone(), path))
        })
        .collect::<Result<Vec<_>, InstallError>>()?;
    Ok(InstallPlan {
        agent: opts.agent.clone(),
        components,
        target_paths,
    })
}

fn component_path(home: &std::path::Path, component: &str) -> Result<PathBuf, InstallError> {
    let claude_dir = home.join(".claude");
    match component {
        "claude-md" => Ok(claude_dir.join("CLAUDE.md")),
        "mcp-json" => Ok(claude_dir.join("mcp.json")),
        "output-style" => Ok(claude_dir.join("output-style.md")),
        other => Err(InstallError::Permanent(format!(
            "unknown component '{other}' in preset"
        ))),
    }
}
