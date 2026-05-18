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

use crate::adapters::claude::{render, CLAUDE_MD_TMPL, MCP_JSON_TMPL, OUTPUT_STYLE_TMPL};
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

pub async fn run(
    opts: &InstallOptions,
    plan: &InstallPlan,
    _backup: &BackupReceipt,
    audit: &AuditWriter,
) -> Result<Vec<Receipt>, InstallError> {
    let started = Utc::now();
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
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    let vars = build_vars(opts, plan);
    let data_dir = opts.home.join(".kx");
    let mut receipts: Vec<Receipt> = Vec::with_capacity(plan.target_paths.len());

    for (component, path) in &plan.target_paths {
        if !plan_contains(plan, path) {
            return Err(InstallError::PathNotInPlan(path.clone()));
        }
        if kernex_sandbox::is_write_blocked(path, &data_dir, None) {
            audit
                .emit(AuditEvent {
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
                })
                .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;
            return Err(InstallError::SandboxRefused { path: path.clone() });
        }
        let receipt = render_and_write(component, path, &vars)?;
        audit
            .emit(AuditEvent {
                event: "stage.apply.write".to_string(),
                stage: Stage::Apply,
                status: EventStatus::Success,
                started_at: Utc::now(),
                ended_at: None,
                duration_ms: None,
                payload: serde_json::to_value(&receipt).unwrap_or(serde_json::Value::Null),
                errors: vec![],
            })
            .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;
        receipts.push(receipt);
    }

    let ended = Utc::now();
    audit
        .emit(AuditEvent {
            event: "stage.apply.end".to_string(),
            stage: Stage::Apply,
            status: EventStatus::Success,
            started_at: started,
            ended_at: Some(ended),
            duration_ms: Some((ended - started).num_milliseconds().max(0) as u64),
            payload: json!({"receipts": &receipts}),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

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
    component: &str,
    path: &Path,
    vars: &HashMap<&str, String>,
) -> Result<Receipt, InstallError> {
    let tmpl = template_for(component)
        .ok_or_else(|| InstallError::Permanent(format!("unknown component '{component}'")))?;
    let rendered = render(tmpl, vars);
    let bytes = rendered.as_bytes();

    let prior_exists = path.exists();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes)?;

    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let sha256: [u8; 32] = hasher.finalize().into();

    Ok(Receipt {
        component: component.to_string(),
        path: path.to_path_buf(),
        action: if prior_exists {
            ReceiptAction::Overwrote
        } else {
            ReceiptAction::Created
        },
        bytes_written: bytes.len() as u64,
        sha256,
    })
}

fn template_for(component: &str) -> Option<&'static str> {
    match component {
        "claude-md" => Some(CLAUDE_MD_TMPL),
        "mcp-json" => Some(MCP_JSON_TMPL),
        "output-style" => Some(OUTPUT_STYLE_TMPL),
        _ => None,
    }
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
