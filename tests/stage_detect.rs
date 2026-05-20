//! Unit tests for §5 DETECT stage.
//!
//! Covers E-detect-1, E-detect-3, E-detect-4, E-detect-5 / E-CC-7.

#![cfg(feature = "agent-claude")]

use std::fs;
use std::path::PathBuf;

use kernex_agent::configurator::{stage_detect, InstallOptions};
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

fn read_audit_log(writer: &AuditWriter) -> Vec<serde_json::Value> {
    let raw = fs::read_to_string(writer.path()).expect("audit file readable");
    raw.lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).expect("audit line is JSON"))
        .collect()
}

#[tokio::test]
async fn emits_start_and_end_events() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let opts = options(tmp.path().to_path_buf());
    let _ = stage_detect::run(&opts, &audit).await.expect("detect ok");
    let events = read_audit_log(&audit);
    assert!(
        events.len() >= 2,
        "expected >= 2 events, got {}",
        events.len()
    );
    assert_eq!(events[0]["event"], "stage.detect.start");
    assert_eq!(events[0]["stage"], "detect");
    assert_eq!(events[1]["event"], "stage.detect.end");
    assert_eq!(events[1]["stage"], "detect");
    assert!(events[1]["duration_ms"].is_number());
}

#[tokio::test]
async fn returns_installed_false_when_claude_missing() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let opts = options(tmp.path().to_path_buf());
    let saved_path = std::env::var_os("PATH");
    unsafe {
        std::env::set_var("PATH", "/nonexistent-kernex-detect-test");
    }
    let detection = stage_detect::run(&opts, &audit).await.expect("detect ok");
    if let Some(prior) = saved_path {
        unsafe { std::env::set_var("PATH", prior) };
    } else {
        unsafe { std::env::remove_var("PATH") };
    }
    assert!(!detection.installed);
    assert!(detection.version.is_none());
}

#[tokio::test]
async fn does_not_write_files_outside_audit_log() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let audit_path = audit.path().to_path_buf();
    let opts = options(tmp.path().to_path_buf());
    let _ = stage_detect::run(&opts, &audit).await.expect("detect ok");
    // Walk tmp; assert only the audit log was written under .kx/.
    let kx_dir = tmp.path().join(".kx");
    let mut leaked = vec![];
    if kx_dir.exists() {
        walk_paths(&kx_dir, &mut leaked);
    }
    for path in &leaked {
        assert!(
            path == &audit_path || path.parent().is_some_and(|p| p.ends_with("audit")),
            "unexpected file outside audit log: {path:?}"
        );
    }
}

#[tokio::test]
async fn unknown_agent_errors_with_unknown_agent_variant() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let mut opts = options(tmp.path().to_path_buf());
    opts.agent = "not-an-agent".to_string();
    let err = stage_detect::run(&opts, &audit)
        .await
        .expect_err("must error");
    match err {
        kernex_agent::configurator::InstallError::UnknownAgent(name) => {
            assert_eq!(name, "not-an-agent")
        }
        other => panic!("expected UnknownAgent, got {other:?}"),
    }
}

fn walk_paths(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_paths(&path, out);
        } else {
            out.push(path);
        }
    }
}
