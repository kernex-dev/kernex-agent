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
    detection: &Detection,
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

    let preset = resolve_preset(&opts.preset, &opts.agent)?;
    let plan = build_plan(opts, detection, preset.components)?;

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

fn build_plan(
    opts: &InstallOptions,
    detection: &Detection,
    components: Vec<String>,
) -> Result<InstallPlan, InstallError> {
    let target_paths = components
        .iter()
        .map(|component| {
            let path = component_path(opts, detection, component)?;
            Ok((component.clone(), path))
        })
        .collect::<Result<Vec<_>, InstallError>>()?;
    Ok(InstallPlan {
        agent: opts.agent.clone(),
        components,
        target_paths,
    })
}

fn component_path(
    opts: &InstallOptions,
    detection: &Detection,
    component: &str,
) -> Result<PathBuf, InstallError> {
    match (opts.agent.as_str(), component) {
        // Claude Code reads global MCP server registrations from
        // <home>/.claude/mcp-servers.json (the dedicated MCP registry)
        // and from <home>/.claude.json (the User MCPs block). We target
        // the dedicated registry because it has a smaller blast radius
        // on errors and matches where the user's existing personal MCPs
        // (figma, affine, freepik) already live. stage_apply MERGES the
        // rendered kernex entry into the existing mcpServers block; it
        // does NOT overwrite the file.
        ("claude-code", "claude-md") => Ok(opts.home.join(".claude").join("CLAUDE.md")),
        ("claude-code", "mcp-json") => Ok(opts.home.join(".claude").join("mcp-servers.json")),
        ("claude-code", "output-style") => Ok(opts.home.join(".claude").join("output-style.md")),
        // Codex writes its instruction surface to `<cwd>/AGENTS.md`
        // (project-rooted per ADR-001) and its MCP server registry to
        // `~/.codex/config.toml` (home-rooted). When detection didn't
        // populate the roots, fall back to opts.home and the current
        // working directory so the resolver stays pure in tests that
        // pass a minimal Detection stub.
        ("codex", "agents-md") => Ok(detection
            .project_root
            .clone()
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| opts.home.clone())
            .join("AGENTS.md")),
        ("codex", "config-toml") => Ok(detection
            .config_root
            .clone()
            .unwrap_or_else(|| opts.home.join(".codex"))
            .join("config.toml")),
        ("codex", "output-style") => Ok(detection
            .config_root
            .clone()
            .unwrap_or_else(|| opts.home.join(".codex"))
            .join("output-style.md")),
        (agent, other) => Err(InstallError::Permanent(format!(
            "unknown component '{other}' for agent '{agent}'"
        ))),
    }
}
