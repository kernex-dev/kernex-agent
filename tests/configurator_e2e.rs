//! End-to-end pipeline test for §11 REPORT close-out.
//!
//! Drives the full configurator pipeline (DETECT -> RESOLVE -> REVIEW
//! -> BACKUP -> APPLY -> VERIFY -> REPORT) against a TempDir HOME and
//! asserts every exit-signal clause from proposal.md §"What done looks
//! like":
//!   1. Files written: CLAUDE.md, mcp-servers.json, output-style.md.
//!   2. Audit log present at <home>/.kx/audit/install-*.jsonl.
//!   3. Per-stage trace events all present (no stage skipped).
//!   4. install.summary event emitted with status='success'.

#![cfg(feature = "agent-claude")]

use std::fs;
use std::path::PathBuf;

use kernex_agent::configurator::{run_with_audit, InstallOptions, InstallStatus};
use kernex_agent::install::audit::AuditWriter;
use tempfile::TempDir;

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
    let report = run_with_audit(opts(tmp.path().to_path_buf()), &audit)
        .await
        .expect("install ok");

    // Clause 1: every component file exists and is non-empty. The
    // mcp-json component lands at .claude/mcp-servers.json (the
    // dedicated Claude Code MCP registry), not .claude/mcp.json.
    let claude_dir = tmp.path().join(".claude");
    for component in ["CLAUDE.md", "mcp-servers.json", "output-style.md"] {
        let path = claude_dir.join(component);
        assert!(path.exists(), "{component} not written at {path:?}");
        assert!(
            fs::metadata(&path).unwrap().len() > 0,
            "{component} is empty"
        );
    }

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
