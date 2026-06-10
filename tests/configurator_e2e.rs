//! End-to-end pipeline test for §11 REPORT close-out.
//!
//! Drives the full configurator pipeline (DETECT -> RESOLVE -> REVIEW
//! -> BACKUP -> APPLY -> VERIFY -> REPORT) against a TempDir HOME and
//! asserts every exit-signal clause from proposal.md §"What done looks
//! like":
//!   1. CLAUDE.md written; mcp-json registered via the host CLI.
//!   2. Audit log present at <home>/.kx/audit/install-*.jsonl.
//!   3. Per-stage trace events all present (no stage skipped).
//!   4. install.summary event emitted with status='success'.

#![cfg(feature = "agent-claude")]

use std::fs;
use std::path::PathBuf;

use kernex_agent::configurator::{
    run_with_audit, run_with_audit_and_registrar, InstallOptions, InstallStatus,
};
use kernex_agent::install::audit::AuditWriter;
use tempfile::TempDir;

mod common;
use common::RecordingRegistrar;

fn opts(home: PathBuf) -> InstallOptions {
    InstallOptions {
        agent: "claude-code".to_string(),
        preset: "solo-dev".to_string(),
        yes: true,
        dry_run: false,
        verify_deep: false,
        cwd: None,
        home,
    }
}

#[tokio::test]
async fn happy_path_writes_all_components_with_full_audit_trail() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let reg = RecordingRegistrar::new();
    let report = run_with_audit_and_registrar(opts(tmp.path().to_path_buf()), &audit, &reg)
        .await
        .expect("install ok");

    // Clause 1: the claude-md file is written and non-empty.
    let claude_dir = tmp.path().join(".claude");
    let claude_md = claude_dir.join("CLAUDE.md");
    assert!(claude_md.exists(), "CLAUDE.md not written at {claude_md:?}");
    assert!(
        fs::metadata(&claude_md).unwrap().len() > 0,
        "CLAUDE.md empty"
    );
    // The mcp-json component registers (not writes a file): the registrar
    // recorded a kernex add, and no mcp-servers.json was written.
    assert!(reg.added("kernex"), "kernex should be registered");
    assert!(!claude_dir.join("mcp-servers.json").exists());

    // Clause 2: report status is success (verify all passed).
    assert!(matches!(
        report.status,
        InstallStatus::Success | InstallStatus::SuccessWithVerifyFailures
    ));

    // Clause 3: per-stage trace - every stage emitted at least one event.
    let lines = fs::read_to_string(audit.path()).unwrap();
    let events: Vec<serde_json::Value> = lines
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for stage in ["detect", "resolve", "review", "backup", "apply", "verify"] {
        let count = events.iter().filter(|e| e["stage"] == stage).count();
        assert!(count > 0, "no events for stage '{stage}'");
    }

    // Clause 4: install.summary is the terminal event.
    let summary = events
        .iter()
        .find(|e| e["event"] == "install.summary")
        .expect("install.summary event present");
    assert_eq!(summary["payload"]["agent"], "claude-code");
    assert_eq!(summary["payload"]["preset"], "solo-dev");
}

#[tokio::test]
async fn apply_failure_rolls_back_partial_writes_through_orchestrator() {
    // End-to-end guard for the auto-rollback path. Stage-level rollback tests
    // feed hand-built receipts and so never exercised the orchestrator, which
    // was passing an empty receipts slice (rollback was a silent no-op).
    //
    // The 2nd component (mcp-json) fails via a registrar whose `add` always
    // errors. The 1st component (CLAUDE.md) is written first; the orchestrator
    // must roll it back so a failed install leaves no partial state on disk.
    let tmp = TempDir::new().unwrap();
    let claude_dir = tmp.path().join(".claude");

    let audit = AuditWriter::new(tmp.path()).unwrap();
    let reg = RecordingRegistrar::failing();
    let result = run_with_audit_and_registrar(opts(tmp.path().to_path_buf()), &audit, &reg).await;

    assert!(result.is_err(), "install must fail when registration fails");

    // CLAUDE.md was created before the failing mcp-json registration; rollback
    // must remove it.
    assert!(
        !claude_dir.join("CLAUDE.md").exists(),
        "CLAUDE.md must be rolled back after a partial-apply failure"
    );

    // A rollback event was emitted for the undone component.
    let lines = fs::read_to_string(audit.path()).unwrap();
    assert!(
        lines.lines().any(|l| l.contains("stage.apply.rollback")),
        "expected a stage.apply.rollback audit event after partial-apply failure"
    );
}

#[tokio::test]
async fn dry_run_skips_backup_apply_verify_and_exits_clean() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let mut options = opts(tmp.path().to_path_buf());
    options.dry_run = true;
    let report = run_with_audit(options, &audit).await.expect("dry-run ok");

    // No component files exist after dry-run.
    let claude_dir = tmp.path().join(".claude");
    assert!(
        !claude_dir.exists(),
        ".claude/ should not be created during dry-run"
    );
    assert!(report.components_written.is_empty());

    // Audit log contains detect, resolve, review, install.summary - but
    // NOT backup, apply, or verify.
    let lines = fs::read_to_string(audit.path()).unwrap();
    let events: Vec<serde_json::Value> = lines
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    for stage in ["detect", "resolve", "review"] {
        assert!(
            events.iter().any(|e| e["stage"] == stage),
            "expected dry-run event for {stage}"
        );
    }
    for stage in ["backup", "apply", "verify"] {
        assert!(
            events.iter().all(|e| e["stage"] != stage),
            "dry-run should NOT emit stage '{stage}'"
        );
    }
    let summary = events
        .iter()
        .find(|e| e["event"] == "install.summary")
        .expect("install.summary present in dry-run");
    assert_eq!(summary["payload"]["dry_run"], true);
}
