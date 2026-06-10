//! Stage 5 APPLY — render templates and write files; auto-rollback on failure.
//!
//! Behavior per E-apply-1..8.

use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use chrono::Utc;
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::adapters::claude::render;
use crate::install::audit::{AuditEvent, AuditWriter, EventError, EventStatus, Stage};

use super::stage_backup::BackupReceipt;
use super::stage_resolve::InstallPlan;
use super::{InstallError, InstallOptions};

/// Per-component write result. APPLY accumulates a `Vec<Receipt>` and emits
/// each as a `stage.apply.write` audit event. Rollback walks the vec in
/// reverse order: `Created` files are removed, `Overwrote` files are
/// extracted from the backup tarball.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    pub component: String,
    pub path: PathBuf,
    pub action: ReceiptAction,
    pub bytes_written: u64,
    pub sha256: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptAction {
    Created,
    Overwrote,
    Skipped,
}

/// APPLY failure that carries the receipts written *before* the failure so the
/// orchestrator can roll them back. Threading these (instead of an empty slice)
/// is what makes auto-rollback actually undo a partial install: a failure on
/// component N must still remove components 1..N-1 that were already written.
#[derive(Debug)]
pub struct ApplyFailure {
    pub partial: Vec<Receipt>,
    pub error: InstallError,
}

impl ApplyFailure {
    fn new(partial: Vec<Receipt>, error: InstallError) -> Self {
        Self { partial, error }
    }
}

pub async fn run(
    opts: &InstallOptions,
    plan: &InstallPlan,
    _backup: &BackupReceipt,
    audit: &AuditWriter,
) -> Result<Vec<Receipt>, ApplyFailure> {
    let started = Utc::now();
    // No receipts exist yet: a start-audit failure rolls back nothing.
    audit
        .emit(AuditEvent {
            event: "stage.apply.start".to_string(),
            stage: Stage::Apply,
            status: EventStatus::Success,
            started_at: started,
            ended_at: None,
            duration_ms: None,
            payload: json!({"agent": &plan.agent, "components": &plan.components}),
            errors: vec![],
        })
        .map_err(|e| {
            ApplyFailure::new(
                Vec::new(),
                InstallError::Permanent(format!("audit emit failed: {e}")),
            )
        })?;

    let vars = build_vars(opts, plan);
    let data_dir = opts.home.join(".kx");
    let mut receipts: Vec<Receipt> = Vec::with_capacity(plan.target_paths.len());

    for (component, path) in &plan.target_paths {
        if !plan_contains(plan, path) {
            return Err(ApplyFailure::new(
                receipts,
                InstallError::PathNotInPlan(path.clone()),
            ));
        }
        if kernex_sandbox::is_write_blocked(path, &data_dir, None) {
            if let Err(e) = audit.emit(AuditEvent {
                event: "stage.apply.error".to_string(),
                stage: Stage::Apply,
                status: EventStatus::Failure,
                started_at: started,
                ended_at: Some(Utc::now()),
                duration_ms: None,
                payload: json!({"component": component, "path": path}),
                errors: vec![EventError {
                    code: "sandbox_refused".to_string(),
                    message: format!("sandbox blocked write to {}", path.display()),
                    transient: true,
                }],
            }) {
                return Err(ApplyFailure::new(
                    receipts,
                    InstallError::Permanent(format!("audit emit failed: {e}")),
                ));
            }
            return Err(ApplyFailure::new(
                receipts,
                InstallError::SandboxRefused { path: path.clone() },
            ));
        }
        let receipt = match render_and_write(&plan.agent, component, path, &vars) {
            Ok(r) => r,
            Err(error) => return Err(ApplyFailure::new(receipts, error)),
        };
        // The file is on disk now: record the receipt BEFORE the write-event
        // emit so that even a logging failure leaves the write rollback-able.
        receipts.push(receipt);
        if let Err(e) = audit.emit(AuditEvent {
            event: "stage.apply.write".to_string(),
            stage: Stage::Apply,
            status: EventStatus::Success,
            started_at: Utc::now(),
            ended_at: None,
            duration_ms: None,
            payload: serde_json::to_value(receipts.last()).unwrap_or(serde_json::Value::Null),
            errors: vec![],
        }) {
            return Err(ApplyFailure::new(
                receipts,
                InstallError::Permanent(format!("audit emit failed: {e}")),
            ));
        }
    }

    let ended = Utc::now();
    if let Err(e) = audit.emit(AuditEvent {
        event: "stage.apply.end".to_string(),
        stage: Stage::Apply,
        status: EventStatus::Success,
        started_at: started,
        ended_at: Some(ended),
        duration_ms: Some((ended - started).num_milliseconds().max(0) as u64),
        payload: json!({"receipts": &receipts}),
        errors: vec![],
    }) {
        return Err(ApplyFailure::new(
            receipts,
            InstallError::Permanent(format!("audit emit failed: {e}")),
        ));
    }

    Ok(receipts)
}

/// Best-effort rollback per E-apply-4..5.
///
/// Walks receipts in reverse. `Created` files are removed; `Overwrote`
/// files are restored from the backup tarball. Each restoration emits
/// `stage.apply.rollback` (or `stage.apply.rollback.error`) and the walk
/// continues after individual failures so a single broken restore does
/// not strand the rest of the rollback.
pub async fn rollback(
    backup: &BackupReceipt,
    receipts: &[Receipt],
    _err: &InstallError,
    audit: &AuditWriter,
) -> Result<(), InstallError> {
    for receipt in receipts.iter().rev() {
        let action_outcome = match receipt.action {
            ReceiptAction::Created => undo_created(&receipt.path),
            ReceiptAction::Overwrote => restore_from_backup(backup, &receipt.path),
            ReceiptAction::Skipped => Ok(()),
        };

        let now = Utc::now();
        let (event, status, errors) = match action_outcome {
            Ok(()) => (
                "stage.apply.rollback".to_string(),
                EventStatus::Success,
                vec![],
            ),
            Err(message) => (
                "stage.apply.rollback.error".to_string(),
                EventStatus::Failure,
                vec![EventError {
                    code: "rollback_step_failed".to_string(),
                    message,
                    transient: false,
                }],
            ),
        };

        audit
            .emit(AuditEvent {
                event,
                stage: Stage::Rollback,
                status,
                started_at: now,
                ended_at: Some(now),
                duration_ms: Some(0),
                payload: serde_json::to_value(receipt).unwrap_or(serde_json::Value::Null),
                errors,
            })
            .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;
    }
    Ok(())
}

fn render_and_write(
    agent: &str,
    component: &str,
    path: &Path,
    vars: &HashMap<&str, String>,
) -> Result<Receipt, InstallError> {
    let tmpl = template_for(agent, component).ok_or_else(|| {
        InstallError::Permanent(format!(
            "unknown component '{component}' for agent '{agent}'"
        ))
    })?;
    let rendered = render(tmpl, vars);

    // Per-component merge semantics:
    // - Claude Code `claude-md`: replace just the `<!-- kernex:begin --> ...
    //   <!-- kernex:end -->` block in place, leaving the rest of the user-owned
    //   `~/.claude/CLAUDE.md` prose untouched (parity with Codex `agents-md`).
    //   Previously this was written verbatim, clobbering the user's global file.
    // - Claude Code `mcp-json`: merge into the existing `mcpServers` block
    //   so other servers (figma, affine, codegraph, ...) are preserved.
    // - Codex `config-toml`: upsert `[mcp_servers.*]` sub-tables via
    //   `toml_edit`, preserving unrelated keys and comment formatting.
    // - Codex `agents-md`: replace just the `<!-- kernex:begin --> ... <!-- kernex:end -->`
    //   block in place, leaving the rest of the user-owned AGENTS.md
    //   untouched.
    // - Everything else: write the rendered bytes verbatim.
    let prior_exists = path.exists();
    let final_bytes: Vec<u8> = match (agent, component) {
        ("claude-code", "claude-md") => merge_claude_md(path, &rendered)?.into_bytes(),
        ("claude-code", "mcp-json") => merge_mcp_servers(path, &rendered)?.into_bytes(),
        #[cfg(feature = "agent-codex")]
        ("codex", "config-toml") => merge_codex_config(path, &rendered)?.into_bytes(),
        #[cfg(feature = "agent-codex")]
        ("codex", "agents-md") => merge_codex_agents_md(path, &rendered)?.into_bytes(),
        _ => rendered.into_bytes(),
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, &final_bytes)?;

    let mut hasher = Sha256::new();
    hasher.update(&final_bytes);
    let sha256: [u8; 32] = hasher.finalize().into();

    Ok(Receipt {
        component: component.to_string(),
        path: path.to_path_buf(),
        action: if prior_exists {
            ReceiptAction::Overwrote
        } else {
            ReceiptAction::Created
        },
        bytes_written: final_bytes.len() as u64,
        sha256,
    })
}

/// Merge the rendered `mcp-json` template into an existing MCP registry
/// file, preserving every other `mcpServers` entry already present.
///
/// Behavior:
/// - If the target file does not exist, returns the rendered template
///   verbatim (so the first install creates a valid one-entry file).
/// - If the target file is empty or non-existent JSON, returns the
///   rendered template verbatim.
/// - If the target file is valid JSON with an `mcpServers` object, each
///   key from the rendered template's `mcpServers` is merged in. Keys
///   that already exist (e.g., a prior `kernex` entry from a re-run) are
///   overwritten with the new value, matching install-idempotency: the
///   second `kx install` produces the same registry as the first.
/// - If the target file exists but is invalid JSON, returns an error so
///   the install fails clean rather than corrupting the user's config.
///
/// Returns the serialized JSON to write back to disk, formatted with
/// two-space indentation and a trailing newline so diffs against the
/// original stay readable.
fn merge_mcp_servers(path: &Path, rendered: &str) -> Result<String, InstallError> {
    let rendered_value: serde_json::Value = serde_json::from_str(rendered).map_err(|e| {
        InstallError::Permanent(format!(
            "mcp-json template did not render to valid JSON: {e}"
        ))
    })?;

    if !path.exists() {
        return Ok(format_with_trailing_newline(&rendered_value));
    }

    let existing_text = fs::read_to_string(path)?;
    if existing_text.trim().is_empty() {
        return Ok(format_with_trailing_newline(&rendered_value));
    }

    let mut existing: serde_json::Value = serde_json::from_str(&existing_text).map_err(|e| {
        InstallError::Permanent(format!(
            "existing MCP registry at {} is not valid JSON: {e}; refusing to overwrite",
            path.display()
        ))
    })?;

    let rendered_servers = rendered_value
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .ok_or_else(|| {
            InstallError::Permanent(
                "mcp-json template is missing a top-level mcpServers object".to_string(),
            )
        })?;

    let existing_obj = existing.as_object_mut().ok_or_else(|| {
        InstallError::Permanent(format!(
            "existing MCP registry at {} is not a JSON object",
            path.display()
        ))
    })?;

    let target_servers = existing_obj
        .entry("mcpServers")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

    let target_map = target_servers.as_object_mut().ok_or_else(|| {
        InstallError::Permanent(format!(
            "existing mcpServers at {} is not a JSON object",
            path.display()
        ))
    })?;

    for (name, server_value) in rendered_servers {
        target_map.insert(name.clone(), server_value.clone());
    }

    Ok(format_with_trailing_newline(&existing))
}

fn format_with_trailing_newline(value: &serde_json::Value) -> String {
    let mut out = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    out.push('\n');
    out
}

fn template_for(agent: &str, component: &str) -> Option<&'static str> {
    use crate::adapters::claude::{CLAUDE_MD_TMPL, MCP_JSON_TMPL, OUTPUT_STYLE_TMPL};
    #[cfg(feature = "agent-codex")]
    use crate::adapters::codex::{
        AGENTS_MD_TMPL as CODEX_AGENTS_MD_TMPL, CONFIG_TOML_TMPL as CODEX_CONFIG_TOML_TMPL,
        OUTPUT_STYLE_TMPL as CODEX_OUTPUT_STYLE_TMPL,
    };
    match (agent, component) {
        ("claude-code", "claude-md") => Some(CLAUDE_MD_TMPL),
        ("claude-code", "mcp-json") => Some(MCP_JSON_TMPL),
        ("claude-code", "output-style") => Some(OUTPUT_STYLE_TMPL),
        #[cfg(feature = "agent-codex")]
        ("codex", "config-toml") => Some(CODEX_CONFIG_TOML_TMPL),
        #[cfg(feature = "agent-codex")]
        ("codex", "agents-md") => Some(CODEX_AGENTS_MD_TMPL),
        #[cfg(feature = "agent-codex")]
        ("codex", "output-style") => Some(CODEX_OUTPUT_STYLE_TMPL),
        _ => None,
    }
}

/// Merge a freshly-rendered Claude `CLAUDE.md` template into the user's
/// `~/.claude/CLAUDE.md`. Wraps the rendered body in
/// `<!-- kernex:begin --> ... <!-- kernex:end -->` via `merge_marker_block`
/// so the user's existing CLAUDE.md prose is preserved across installs
/// (parity with Codex `agents-md`). This replaces the prior verbatim write,
/// which clobbered the user's global CLAUDE.md.
fn merge_claude_md(path: &Path, rendered: &str) -> Result<String, InstallError> {
    let existing = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    Ok(crate::adapters::shared::merge_marker_block(
        &existing,
        rendered,
        "<!-- kernex:begin -->",
        "<!-- kernex:end -->",
    ))
}

/// Merge a freshly-rendered Codex `config.toml` template into an existing
/// `~/.codex/config.toml`. Delegates to `crate::adapters::codex::merge_codex_config_toml`
/// which uses `toml_edit` to preserve formatting and unrelated entries.
#[cfg(feature = "agent-codex")]
fn merge_codex_config(path: &Path, rendered: &str) -> Result<String, InstallError> {
    let existing = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    crate::adapters::codex::merge_codex_config_toml(&existing, rendered).map_err(|e| {
        InstallError::Permanent(format!(
            "codex config.toml merge failed at {}: {e}",
            path.display()
        ))
    })
}

/// Merge a freshly-rendered Codex `AGENTS.md` template into the project-
/// local `<cwd>/AGENTS.md`. Wraps the rendered body in
/// `<!-- kernex:begin --> ... <!-- kernex:end -->` via `merge_marker_block`
/// so the user's pre-existing AGENTS.md prose stays untouched.
#[cfg(feature = "agent-codex")]
fn merge_codex_agents_md(path: &Path, rendered: &str) -> Result<String, InstallError> {
    let existing = if path.exists() {
        fs::read_to_string(path)?
    } else {
        String::new()
    };
    Ok(crate::adapters::shared::merge_marker_block(
        &existing,
        rendered,
        "<!-- kernex:begin -->",
        "<!-- kernex:end -->",
    ))
}

fn build_vars(opts: &InstallOptions, plan: &InstallPlan) -> HashMap<&'static str, String> {
    let mut vars = HashMap::new();
    vars.insert(
        "project_name",
        opts.home
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("project")
            .to_string(),
    );
    vars.insert(
        "user_name",
        std::env::var("USER").unwrap_or_else(|_| "developer".to_string()),
    );
    vars.insert("kernex_version", env!("CARGO_PKG_VERSION").to_string());
    vars.insert(
        "install_timestamp",
        Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    );
    vars.insert("components", plan.components.join(", "));
    vars
}

fn plan_contains(plan: &InstallPlan, path: &Path) -> bool {
    plan.target_paths.iter().any(|(_, p)| p == path)
}

fn undo_created(path: &Path) -> Result<(), String> {
    fs::remove_file(path).map_err(|e| format!("remove {}: {e}", path.display()))
}

fn restore_from_backup(backup: &BackupReceipt, target: &Path) -> Result<(), String> {
    let file = File::open(&backup.tarball_path)
        .map_err(|e| format!("open backup {}: {e}", backup.tarball_path.display()))?;
    let decoder = GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    let mut found = false;
    let archive_target = target.strip_prefix("/").unwrap_or(target);
    for entry in archive
        .entries()
        .map_err(|e| format!("read archive entries: {e}"))?
    {
        let mut entry = entry.map_err(|e| format!("iter entry: {e}"))?;
        let entry_path = entry
            .path()
            .map_err(|e| format!("entry path: {e}"))?
            .into_owned();
        if entry_path == archive_target {
            entry
                .unpack(target)
                .map_err(|e| format!("unpack {}: {e}", target.display()))?;
            found = true;
            break;
        }
    }
    if !found {
        return Err(format!(
            "backup tarball does not contain {}",
            target.display()
        ));
    }
    Ok(())
}
