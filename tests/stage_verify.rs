//! Unit tests for §10 VERIFY stage.
//!
//! Covers E-verify-1..5.

#![cfg(feature = "agent-claude")]

use std::fs;
use std::path::PathBuf;

use kernex_agent::configurator::stage_apply::{Receipt, ReceiptAction};
use kernex_agent::configurator::stage_resolve::InstallPlan;
use kernex_agent::configurator::{stage_apply, stage_backup, stage_verify, InstallOptions};
use kernex_agent::install::audit::AuditWriter;
use tempfile::TempDir;

mod common;
use common::RecordingRegistrar;

fn options(home: PathBuf, deep: bool) -> InstallOptions {
    InstallOptions {
        agent: "claude-code".to_string(),
        preset: "solo-dev".to_string(),
        yes: true,
        dry_run: false,
        verify_deep: deep,
        cwd: None,
        home,
    }
}

fn plan_for(home: &std::path::Path) -> InstallPlan {
    let claude = home.join(".claude");
    InstallPlan {
        agent: "claude-code".to_string(),
        components: vec!["claude-md".into(), "mcp-json".into()],
        target_paths: vec![
            ("claude-md".into(), claude.join("CLAUDE.md")),
            ("mcp-json".into(), claude.join("mcp.json")),
        ],
    }
}

async fn fresh_install(tmp: &TempDir) -> (InstallOptions, InstallPlan, AuditWriter, Vec<Receipt>) {
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let opts = options(tmp.path().to_path_buf(), false);
    let plan = plan_for(tmp.path());
    let backup = stage_backup::run(&opts, &plan, &audit).await.unwrap();
    let reg = RecordingRegistrar::new();
    let receipts = stage_apply::run(&opts, &plan, &backup, &audit, &reg)
        .await
        .unwrap();
    (opts, plan, audit, receipts)
}

#[tokio::test]
async fn e_verify_1_passes_on_clean_install() {
    let tmp = TempDir::new().unwrap();
    let (opts, plan, audit, receipts) = fresh_install(&tmp).await;
    let report = stage_verify::run(&opts, &plan, &receipts, &audit)
        .await
        .unwrap();
    assert!(
        report.all_passed(),
        "verify should pass clean install; failed = {:?}",
        report
            .checks
            .iter()
            .filter(|c| !c.passed)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn e_verify_1_sha256_mismatch_recorded_but_no_rollback() {
    let tmp = TempDir::new().unwrap();
    let (opts, plan, audit, receipts) = fresh_install(&tmp).await;
    // Mutate one of the files to force a sha mismatch.
    fs::write(&receipts[0].path, b"tampered content").unwrap();
    let report = stage_verify::run(&opts, &plan, &receipts, &audit)
        .await
        .unwrap();
    assert!(!report.all_passed());
    let mismatch = report
        .checks
        .iter()
        .find(|c| c.name.contains("sha256") && !c.passed)
        .expect("expected at least one sha256 failure");
    assert!(mismatch
        .detail
        .as_ref()
        .unwrap()
        .contains("sha256 mismatch"));
    // VERIFY does NOT remove or restore files; the tampered file remains.
    let bytes = fs::read(&receipts[0].path).unwrap();
    assert_eq!(bytes, b"tampered content");
}

#[tokio::test]
async fn verify_skips_the_mcp_registration_component() {
    let tmp = TempDir::new().unwrap();
    let (opts, plan, audit, receipts) = fresh_install(&tmp).await;
    let report = stage_verify::run(&opts, &plan, &receipts, &audit)
        .await
        .unwrap();
    // mcp-json is a host-CLI registration now, not a file, so VERIFY emits no
    // file check for it and still passes a clean install.
    assert!(
        receipts.iter().any(|r| r.component == "mcp-json"),
        "mcp-json receipt should be present"
    );
    assert!(
        !report.checks.iter().any(|c| c.name.starts_with("mcp-json")),
        "verify must not emit a file check for the mcp-json registration"
    );
    assert!(report.all_passed());
}

#[tokio::test]
async fn e_verify_4_emits_end_event_with_checks_payload() {
    let tmp = TempDir::new().unwrap();
    let (opts, plan, audit, receipts) = fresh_install(&tmp).await;
    let _ = stage_verify::run(&opts, &plan, &receipts, &audit)
        .await
        .unwrap();
    let raw = fs::read_to_string(audit.path()).unwrap();
    let end = raw
        .lines()
        .find(|l| l.contains("\"event\":\"stage.verify.end\""))
        .expect("stage.verify.end must exist");
    let parsed: serde_json::Value = serde_json::from_str(end).unwrap();
    assert!(parsed["payload"]["checks"].is_array());
}

#[tokio::test]
async fn e_verify_5_failed_check_does_not_abort_pipeline() {
    let tmp = TempDir::new().unwrap();
    let (opts, plan, audit, receipts) = fresh_install(&tmp).await;
    // Delete one of the files so the path-exists check fails.
    fs::remove_file(&receipts[0].path).unwrap();
    let report = stage_verify::run(&opts, &plan, &receipts, &audit).await;
    assert!(
        report.is_ok(),
        "VERIFY must not return Err on failed checks"
    );
    let report = report.unwrap();
    assert!(!report.all_passed());
}

#[tokio::test]
async fn e_verify_6_deep_records_canary_stub_check() {
    let tmp = TempDir::new().unwrap();
    let (mut opts, plan, audit, receipts) = fresh_install(&tmp).await;
    opts.verify_deep = true;
    let report = stage_verify::run(&opts, &plan, &receipts, &audit)
        .await
        .unwrap();
    let deep = report.checks.iter().find(|c| c.name.starts_with("deep:"));
    assert!(
        deep.is_some(),
        "expected a deep:* check entry under --verify-deep"
    );
    // Note: detail wording depends on whether 'claude' is on the CI runner's PATH.
    let _ = (plan, &receipts, ReceiptAction::Created);
}
