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

use super::mcp_registrar::{McpRegistrar, RegisterOutcome};
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
    /// The component registered an MCP server with the host CLI (no file
    /// written). Rollback unregisters it via the host CLI.
    Registered,
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

/// MCP registry name and scope kernex registers under. Single registration
/// today; apply and rollback share these so they stay in lockstep.
const MCP_REGISTER_NAME: &str = "kernex";
const MCP_REGISTER_SCOPE: &str = "user";

/// True for components that register with a host CLI instead of writing a file.
fn is_mcp_registration(agent: &str, component: &str) -> bool {
    matches!((agent, component), ("claude-code", "mcp-json"))
}

pub async fn run(
    opts: &InstallOptions,
    plan: &InstallPlan,
    _backup: &BackupReceipt,
    audit: &AuditWriter,
    registrar: &dyn McpRegistrar,
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
        let receipt = if is_mcp_registration(&plan.agent, component) {
            // Registration is a host-CLI command, not a file write, so the
            // sandbox write-probe does not apply.
            match register_mcp(component, path, registrar) {
                Ok(r) => r,
                Err(error) => return Err(ApplyFailure::new(receipts, error)),
            }
        } else {
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
            match render_and_write(&plan.agent, component, path, &vars) {
                Ok(r) => r,
                Err(error) => return Err(ApplyFailure::new(receipts, error)),
            }
        };
        // The component is applied now: record the receipt BEFORE the write-event
        // emit so that even a logging failure leaves it rollback-able.
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
/// files are restored from the backup tarball; `Registered` MCP servers are
/// unregistered via the host CLI. Each step emits `stage.apply.rollback`
/// (or `stage.apply.rollback.error`) and the walk continues after individual
/// failures so a single broken restore does not strand the rest of the
/// rollback.
pub async fn rollback(
    backup: &BackupReceipt,
    receipts: &[Receipt],
    _err: &InstallError,
    audit: &AuditWriter,
    registrar: &dyn McpRegistrar,
) -> Result<(), InstallError> {
    for receipt in receipts.iter().rev() {
        let action_outcome = match receipt.action {
            ReceiptAction::Created => undo_created(&receipt.path),
            ReceiptAction::Overwrote => restore_from_backup(backup, &receipt.path),
            ReceiptAction::Skipped => Ok(()),
            ReceiptAction::Registered => registrar
                .remove(MCP_REGISTER_NAME, MCP_REGISTER_SCOPE)
                .map_err(|e| e.to_string()),
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

/// Register kernex's MCP server with the host CLI (Claude Code) via the
/// injected registrar. Produces a `Registered` receipt, or `Skipped` when no
/// host CLI is present (nothing to register into). Rollback unregisters via
/// the same registrar.
fn register_mcp(
    component: &str,
    path: &Path,
    registrar: &dyn McpRegistrar,
) -> Result<Receipt, InstallError> {
    let server_json = json!({ "command": "kx", "args": ["mcp"] }).to_string();
    let action = match registrar.add(MCP_REGISTER_NAME, &server_json, MCP_REGISTER_SCOPE)? {
        RegisterOutcome::Registered => ReceiptAction::Registered,
        RegisterOutcome::SkippedNoClaude => ReceiptAction::Skipped,
    };
    Ok(Receipt {
        component: component.to_string(),
        path: path.to_path_buf(),
        action,
        bytes_written: 0,
        sha256: [0u8; 32],
    })
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
    //   (Claude `mcp-json` is NOT a file component: it registers via the host
    //   CLI in `register_mcp`, so it never reaches `render_and_write`.)
    // - Codex `config-toml`: upsert `[mcp_servers.*]` sub-tables via
    //   `toml_edit`, preserving unrelated keys and comment formatting.
    // - Codex `agents-md`: replace just the `<!-- kernex:begin --> ... <!-- kernex:end -->`
    //   block in place, leaving the rest of the user-owned AGENTS.md
    //   untouched.
    // - Everything else: write the rendered bytes verbatim.
    let prior_exists = path.exists();
    let final_bytes: Vec<u8> = match (agent, component) {
        ("claude-code", "claude-md") => merge_claude_md(path, &rendered)?.into_bytes(),
        #[cfg(feature = "agent-codex")]
        ("codex", "config-toml") => merge_codex_config(path, &rendered)?.into_bytes(),
        #[cfg(feature = "agent-codex")]
        ("codex", "agents-md") => merge_codex_agents_md(path, &rendered)?.into_bytes(),
        _ => rendered.into_bytes(),
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    // Refuse to write through a symlink: a link planted at the target path
    // (e.g. ~/.claude/CLAUDE.md -> ~/.ssh/authorized_keys) would otherwise
    // redirect this write to wherever it points. symlink_metadata does not
    // follow, so the check sees the link itself.
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(InstallError::Permanent(format!(
                "refusing to write through symlink at {} (replace the link                  with a regular file to proceed)",
                path.display()
            )));
        }
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

fn template_for(agent: &str, component: &str) -> Option<&'static str> {
    use crate::adapters::claude::CLAUDE_MD_TMPL;
    #[cfg(feature = "agent-codex")]
    use crate::adapters::codex::{
        AGENTS_MD_TMPL as CODEX_AGENTS_MD_TMPL, CONFIG_TOML_TMPL as CODEX_CONFIG_TOML_TMPL,
        OUTPUT_STYLE_TMPL as CODEX_OUTPUT_STYLE_TMPL,
    };
    match (agent, component) {
        ("claude-code", "claude-md") => Some(CLAUDE_MD_TMPL),
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
