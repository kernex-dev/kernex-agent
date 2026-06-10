//! Stage 6 VERIFY — health checks after APPLY commits.
//!
//! Behavior per E-verify-1..6. VERIFY is observational; failed checks are
//! recorded in the VerifyReport but do NOT trigger rollback (E-verify-5).

use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::install::audit::{AuditEvent, AuditWriter, EventStatus, Stage};

use super::stage_apply::{Receipt, ReceiptAction};
use super::stage_resolve::InstallPlan;
use super::{InstallError, InstallOptions};

/// Per-check pass/fail record assembled by VERIFY.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyReport {
    pub checks: Vec<VerifyCheck>,
}

impl VerifyReport {
    pub fn all_passed(&self) -> bool {
        self.checks.iter().all(|c| c.passed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyCheck {
    pub name: String,
    pub passed: bool,
    pub detail: Option<String>,
}

pub async fn run(
    opts: &InstallOptions,
    _plan: &InstallPlan,
    receipts: &[Receipt],
    audit: &AuditWriter,
) -> Result<VerifyReport, InstallError> {
    let started = Utc::now();
    audit
        .emit(AuditEvent {
            event: "stage.verify.start".to_string(),
            stage: Stage::Verify,
            status: EventStatus::Success,
            started_at: started,
            ended_at: None,
            duration_ms: None,
            payload: json!({"deep": opts.verify_deep}),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    let mut checks = Vec::new();
    for receipt in receipts {
        // Registration components (Registered, or Skipped when no host CLI)
        // have no file on disk; lenient verify skips them.
        if matches!(
            receipt.action,
            ReceiptAction::Registered | ReceiptAction::Skipped
        ) {
            continue;
        }
        checks.push(check_path_exists_and_sha256(receipt));
        if receipt.component == "claude-md" {
            checks.push(check_utf8(&receipt.path, "claude-md UTF-8"));
        }
    }

    if opts.verify_deep {
        if which_claude_on_path() {
            checks.push(VerifyCheck {
                name: "deep:claude_version".to_string(),
                passed: true,
                detail: Some("claude binary on PATH; canary stub deferred".to_string()),
            });
            audit
                .emit(AuditEvent {
                    event: "stage.verify.deep_version".to_string(),
                    stage: Stage::Verify,
                    status: EventStatus::Success,
                    started_at: Utc::now(),
                    ended_at: None,
                    duration_ms: None,
                    payload: serde_json::Value::Null,
                    errors: vec![],
                })
                .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;
            tracing::warn!(
                target: "kernex.install.verify",
                "--verify-deep canary prompt is a stub; lands in a follow-up change"
            );
        } else {
            checks.push(VerifyCheck {
                name: "deep:claude_version".to_string(),
                passed: false,
                detail: Some("claude not on PATH; deep verify skipped".to_string()),
            });
        }
    }

    let report = VerifyReport { checks };
    let ended = Utc::now();
    audit
        .emit(AuditEvent {
            event: "stage.verify.end".to_string(),
            stage: Stage::Verify,
            status: if report.all_passed() {
                EventStatus::Success
            } else {
                EventStatus::Failure
            },
            started_at: started,
            ended_at: Some(ended),
            duration_ms: Some((ended - started).num_milliseconds().max(0) as u64),
            payload: json!({"checks": &report.checks}),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    Ok(report)
}

fn check_path_exists_and_sha256(receipt: &Receipt) -> VerifyCheck {
    match std::fs::read(&receipt.path) {
        Ok(bytes) => {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            let actual: [u8; 32] = hasher.finalize().into();
            if actual == receipt.sha256 {
                VerifyCheck {
                    name: format!("{}:sha256", receipt.component),
                    passed: true,
                    detail: None,
                }
            } else {
                VerifyCheck {
                    name: format!("{}:sha256", receipt.component),
                    passed: false,
                    detail: Some(format!("sha256 mismatch at {}", receipt.path.display())),
                }
            }
        }
        Err(err) => VerifyCheck {
            name: format!("{}:exists", receipt.component),
            passed: false,
            detail: Some(format!("{}: {err}", receipt.path.display())),
        },
    }
}

fn check_utf8(path: &std::path::Path, label: &str) -> VerifyCheck {
    match std::fs::read(path) {
        Ok(bytes) => match std::str::from_utf8(&bytes) {
            Ok(_) => VerifyCheck {
                name: format!("{label}:utf8"),
                passed: true,
                detail: None,
            },
            Err(err) => VerifyCheck {
                name: format!("{label}:utf8"),
                passed: false,
                detail: Some(format!("invalid UTF-8: {err}")),
            },
        },
        Err(err) => VerifyCheck {
            name: format!("{label}:utf8"),
            passed: false,
            detail: Some(format!("read {}: {err}", path.display())),
        },
    }
}

fn which_claude_on_path() -> bool {
    std::process::Command::new("which")
        .arg("claude")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
