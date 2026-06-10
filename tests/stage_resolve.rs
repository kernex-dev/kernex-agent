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
        cwd: None,
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
    assert_eq!(plan.components, vec!["claude-md", "mcp-json"]);
    assert_eq!(plan.target_paths.len(), 2);
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

#[cfg(feature = "agent-codex")]
#[tokio::test]
async fn f_1_8_codex_solo_dev_returns_codex_paths() {
    // Detection from the Codex adapter populates both config_root
    // (~/.codex) and project_root (cwd at install time). The resolver
    // turns them into <home>/.codex/{config.toml,output-style.md} plus
    // <project>/AGENTS.md.
    let home_tmp = TempDir::new().unwrap();
    let project_tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(home_tmp.path()).unwrap();
    let mut opts = options(home_tmp.path().to_path_buf());
    opts.agent = "codex".to_string();
    let detection = Detection::with_project_root(
        false,
        Some(home_tmp.path().join(".codex")),
        Some(project_tmp.path().to_path_buf()),
        None,
    );
    let plan = stage_resolve::run(&opts, &detection, &audit)
        .await
        .expect("resolve ok");
    assert_eq!(plan.agent, "codex");
    assert_eq!(
        plan.components,
        vec!["agents-md", "config-toml", "output-style"]
    );
    let by_component: std::collections::HashMap<_, _> = plan.target_paths.iter().cloned().collect();
    assert_eq!(
        by_component.get("agents-md"),
        Some(&project_tmp.path().join("AGENTS.md")),
        "agents-md path lives at <project_root>/AGENTS.md"
    );
    assert_eq!(
        by_component.get("config-toml"),
        Some(&home_tmp.path().join(".codex").join("config.toml")),
        "config-toml path lives at <config_root>/config.toml"
    );
    assert_eq!(
        by_component.get("output-style"),
        Some(&home_tmp.path().join(".codex").join("output-style.md")),
        "output-style path lives at <config_root>/output-style.md"
    );
}
