//! Unit tests for §6 RESOLVE stage.
//!
//! Covers E-resolve-1..5.

#![cfg(feature = "agent-claude")]

use std::fs;
use std::path::PathBuf;

use kernex_adapter_core::Detection;
use kernex_agent::configurator::{stage_resolve, InstallError, InstallOptions};
use kernex_agent::install::audit::AuditWriter;
use tempfile::TempDir;

fn options(home: PathBuf) -> InstallOptions {
    InstallOptions {
        agent: "claude-code".to_string(),
        preset: "solo-dev".to_string(),
        yes: true,
        dry_run: false,
        verify_deep: false,
        home,
    }
}

fn detection_stub() -> Detection {
    Detection::new(false, None, None)
}

fn read_events(writer: &AuditWriter) -> Vec<serde_json::Value> {
    fs::read_to_string(writer.path())
        .unwrap()
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
async fn e_resolve_1_returns_plan_with_agent_components_target_paths() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let opts = options(tmp.path().to_path_buf());
    let plan = stage_resolve::run(&opts, &detection_stub(), &audit)
        .await
        .expect("resolve ok");
    assert_eq!(plan.agent, "claude-code");
    assert_eq!(
        plan.components,
        vec!["claude-md", "mcp-json", "output-style"]
    );
    assert_eq!(plan.target_paths.len(), 3);
}

#[tokio::test]
async fn e_resolve_2_expands_solo_dev_preset() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let opts = options(tmp.path().to_path_buf());
    let plan = stage_resolve::run(&opts, &detection_stub(), &audit)
        .await
        .expect("resolve ok");
    let paths: Vec<&PathBuf> = plan.target_paths.iter().map(|(_, p)| p).collect();
    // Every target path sits under <home>/.claude/.
    let claude_dir = opts.home.join(".claude");
    for p in &paths {
        assert!(
            p.starts_with(&claude_dir),
            "target path {p:?} not under {claude_dir:?}"
        );
    }
}

#[tokio::test]
async fn e_resolve_2_unknown_preset_errors() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let mut opts = options(tmp.path().to_path_buf());
    opts.preset = "no-such-preset".to_string();
    let err = stage_resolve::run(&opts, &detection_stub(), &audit)
        .await
        .expect_err("must error");
    match err {
        InstallError::UnknownPreset(name) => assert_eq!(name, "no-such-preset"),
        other => panic!("expected UnknownPreset, got {other:?}"),
    }
}

#[tokio::test]
async fn e_resolve_4_emits_start_and_end_with_plan_payload() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let opts = options(tmp.path().to_path_buf());
    let _ = stage_resolve::run(&opts, &detection_stub(), &audit)
        .await
        .expect("resolve ok");
    let events = read_events(&audit);
    let resolve_events: Vec<_> = events.iter().filter(|e| e["stage"] == "resolve").collect();
    assert_eq!(resolve_events.len(), 2, "expected exactly 2 resolve events");
    assert_eq!(resolve_events[0]["event"], "stage.resolve.start");
    assert_eq!(resolve_events[1]["event"], "stage.resolve.end");
    // End event payload contains the plan.
    assert_eq!(resolve_events[1]["payload"]["agent"], "claude-code");
    assert!(resolve_events[1]["payload"]["components"].is_array());
}

#[tokio::test]
async fn e_resolve_5_writes_no_files_under_home() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let opts = options(tmp.path().to_path_buf());
    let _ = stage_resolve::run(&opts, &detection_stub(), &audit)
        .await
        .expect("resolve ok");
    // The .claude/ dir does NOT get created at RESOLVE; only at APPLY.
    assert!(
        !opts.home.join(".claude").exists(),
        ".claude/ should not exist after RESOLVE"
    );
}
