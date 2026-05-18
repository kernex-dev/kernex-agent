//! Stage 7 REPORT — human-readable summary + `install.summary` event.
//!
//! Behavior per E-report-1..3. Maps the typed pipeline outcome to an
//! `InstallReport` whose `status` field drives the exit code at the CLI
//! dispatcher boundary (§12.4).

use chrono::Utc;
use serde_json::json;

use crate::install::audit::{AuditEvent, AuditWriter, EventStatus, Stage};

use super::stage_apply::Receipt;
use super::stage_resolve::InstallPlan;
use super::stage_verify::VerifyReport;
use super::{InstallError, InstallOptions, InstallReport, InstallStatus};

pub async fn run(
    opts: &InstallOptions,
    plan: &InstallPlan,
    apply: &[Receipt],
    verify: &VerifyReport,
    audit: &AuditWriter,
) -> Result<InstallReport, InstallError> {
    let now = Utc::now();
    let status = if verify.all_passed() {
        InstallStatus::Success
    } else {
        InstallStatus::SuccessWithVerifyFailures
    };

    print_summary(opts, plan, apply, verify, audit, status);

    audit
        .emit(AuditEvent {
            event: "install.summary".to_string(),
            stage: Stage::Install,
            status: EventStatus::Success,
            started_at: now,
            ended_at: Some(now),
            duration_ms: Some(0),
            payload: json!({
                "status": status,
                "agent": &plan.agent,
                "preset": &opts.preset,
                "components": &plan.components,
                "audit_log_path": audit.path(),
                "verify_checks": verify.checks.len(),
                "verify_failures": verify.checks.iter().filter(|c| !c.passed).count(),
                "receipts": apply.iter().map(|r| &r.path).collect::<Vec<_>>(),
            }),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    Ok(InstallReport {
        status,
        audit_log_path: audit.path().to_path_buf(),
        backup_path: None,
        components_written: apply.iter().map(|r| r.component.clone()).collect(),
    })
}

pub async fn run_dry_run(
    opts: &InstallOptions,
    plan: &InstallPlan,
    audit: &AuditWriter,
) -> Result<InstallReport, InstallError> {
    let now = Utc::now();
    println!("\nDry run complete. No files were written.");
    println!("  agent: {}", plan.agent);
    println!("  preset: {}", opts.preset);
    println!("  components: {}", plan.components.join(", "));
    println!("  audit log: {}", audit.path().display());
    println!();

    audit
        .emit(AuditEvent {
            event: "install.summary".to_string(),
            stage: Stage::Install,
            status: EventStatus::Success,
            started_at: now,
            ended_at: Some(now),
            duration_ms: Some(0),
            payload: json!({
                "status": "success",
                "dry_run": true,
                "agent": &plan.agent,
                "preset": &opts.preset,
                "components": &plan.components,
                "audit_log_path": audit.path(),
            }),
            errors: vec![],
        })
        .map_err(|e| InstallError::Permanent(format!("audit emit failed: {e}")))?;

    Ok(InstallReport {
        status: InstallStatus::Success,
        audit_log_path: audit.path().to_path_buf(),
        backup_path: None,
        components_written: Vec::new(),
    })
}

fn print_summary(
    opts: &InstallOptions,
    plan: &InstallPlan,
    apply: &[Receipt],
    verify: &VerifyReport,
    audit: &AuditWriter,
    status: InstallStatus,
) {
    let label = match status {
        InstallStatus::Success => "Install complete.",
        InstallStatus::SuccessWithVerifyFailures => {
            "Install complete (with verify failures; files were written)."
        }
        InstallStatus::RolledBack => "Install rolled back.",
        InstallStatus::UserDeclined => "Install declined at REVIEW.",
        InstallStatus::Aborted => "Install aborted.",
    };

    println!("\n{label}");
    println!("  agent: {}", plan.agent);
    println!("  preset: {}", opts.preset);
    println!("  components written:");
    for r in apply {
        println!("    - {} -> {}", r.component, r.path.display());
    }
    if !verify.all_passed() {
        let failed: Vec<&str> = verify
            .checks
            .iter()
            .filter(|c| !c.passed)
            .map(|c| c.name.as_str())
            .collect();
        println!("  verify failures: {}", failed.join(", "));
    }
    println!("  audit log: {}", audit.path().display());
    println!("\nNext: kx mem stats");
    println!();
}

/// Map an `InstallReport` to a process exit code per E-report-3.
pub fn exit_code_for(report: &InstallReport) -> i32 {
    match report.status {
        InstallStatus::Success | InstallStatus::SuccessWithVerifyFailures => 0,
        InstallStatus::UserDeclined => 0,
        InstallStatus::RolledBack => 1,
        InstallStatus::Aborted => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_success_is_zero() {
        let report = InstallReport {
            status: InstallStatus::Success,
            audit_log_path: std::path::PathBuf::from("/tmp/x"),
            backup_path: None,
            components_written: vec![],
        };
        assert_eq!(exit_code_for(&report), 0);
    }

    #[test]
    fn exit_code_user_declined_is_zero() {
        let report = InstallReport {
            status: InstallStatus::UserDeclined,
            audit_log_path: std::path::PathBuf::from("/tmp/x"),
            backup_path: None,
            components_written: vec![],
        };
        assert_eq!(exit_code_for(&report), 0);
    }

    #[test]
    fn exit_code_rolled_back_is_one() {
        let report = InstallReport {
            status: InstallStatus::RolledBack,
            audit_log_path: std::path::PathBuf::from("/tmp/x"),
            backup_path: None,
            components_written: vec![],
        };
        assert_eq!(exit_code_for(&report), 1);
    }

    #[test]
    fn exit_code_aborted_is_two() {
        let report = InstallReport {
            status: InstallStatus::Aborted,
            audit_log_path: std::path::PathBuf::from("/tmp/x"),
            backup_path: None,
            components_written: vec![],
        };
        assert_eq!(exit_code_for(&report), 2);
    }
}
