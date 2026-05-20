//! Unit tests for §8 BACKUP stage.
//!
//! Covers E-backup-1..6.

#![cfg(feature = "agent-claude")]

use std::fs;
use std::path::PathBuf;

use kernex_agent::configurator::stage_backup;
use kernex_agent::configurator::stage_resolve::InstallPlan;
use kernex_agent::configurator::InstallOptions;
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

fn plan_with(home: &std::path::Path, components: &[&str]) -> InstallPlan {
    let claude = home.join(".claude");
    let target_paths = components
        .iter()
        .map(|c| {
            let path = match *c {
                "claude-md" => claude.join("CLAUDE.md"),
                "mcp-json" => claude.join("mcp.json"),
                "output-style" => claude.join("output-style.md"),
                other => panic!("unknown component {other}"),
            };
            ((*c).to_string(), path)
        })
        .collect();
    InstallPlan {
        agent: "claude-code".to_string(),
        components: components.iter().map(|c| (*c).to_string()).collect(),
        target_paths,
    }
}

#[tokio::test]
async fn e_backup_1_writes_tarball_under_kx_backups() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    // Pre-create one of the target files so the backup has something to capture.
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("CLAUDE.md"), b"pre-existing").unwrap();
    let opts = options(tmp.path().to_path_buf());
    let plan = plan_with(tmp.path(), &["claude-md", "mcp-json"]);
    let receipt = stage_backup::run(&opts, &plan, &audit).await.unwrap();
    assert!(receipt.tarball_path.exists(), "tarball should be written");
    assert!(
        receipt
            .tarball_path
            .starts_with(tmp.path().join(".kx").join("backups")),
        "tarball not under .kx/backups: {:?}",
        receipt.tarball_path
    );
    assert!(receipt.tarball_path.to_string_lossy().ends_with(".tar.gz"));
}

#[tokio::test]
async fn e_backup_2_includes_only_existing_paths() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("CLAUDE.md"), b"exists").unwrap();
    // Note: mcp.json deliberately NOT written - should not appear in `files`.
    let opts = options(tmp.path().to_path_buf());
    let plan = plan_with(tmp.path(), &["claude-md", "mcp-json"]);
    let receipt = stage_backup::run(&opts, &plan, &audit).await.unwrap();
    assert_eq!(receipt.files.len(), 1);
    assert!(receipt.files[0].ends_with(".claude/CLAUDE.md"));
}

#[tokio::test]
async fn e_backup_4_emits_end_event_with_payload() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("CLAUDE.md"), b"x").unwrap();
    let opts = options(tmp.path().to_path_buf());
    let plan = plan_with(tmp.path(), &["claude-md"]);
    let _ = stage_backup::run(&opts, &plan, &audit).await.unwrap();
    let lines = fs::read_to_string(audit.path()).unwrap();
    let events: Vec<serde_json::Value> = lines
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let end = events
        .iter()
        .find(|e| e["event"] == "stage.backup.end")
        .expect("stage.backup.end must exist");
    assert!(end["payload"]["tarball_path"].is_string());
    assert!(end["payload"]["files"].is_array());
    assert!(end["payload"]["bytes"].is_number());
}

#[tokio::test]
async fn e_backup_5_creates_backups_dir_if_missing() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let opts = options(tmp.path().to_path_buf());
    let plan = plan_with(tmp.path(), &["claude-md"]);
    assert!(!tmp.path().join(".kx").join("backups").exists());
    let _ = stage_backup::run(&opts, &plan, &audit).await.unwrap();
    assert!(tmp.path().join(".kx").join("backups").exists());
}

#[tokio::test]
async fn e_backup_empty_plan_produces_empty_files() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let opts = options(tmp.path().to_path_buf());
    // Plan with no existing targets on disk.
    let plan = plan_with(tmp.path(), &["claude-md", "mcp-json"]);
    let receipt = stage_backup::run(&opts, &plan, &audit).await.unwrap();
    assert!(receipt.files.is_empty(), "no files should be included");
}

#[tokio::test]
async fn e_backup_tarball_filename_includes_iso8601_and_agent() {
    let tmp = TempDir::new().unwrap();
    let audit = AuditWriter::new(tmp.path()).unwrap();
    let claude_dir = tmp.path().join(".claude");
    fs::create_dir_all(&claude_dir).unwrap();
    fs::write(claude_dir.join("CLAUDE.md"), b"x").unwrap();
    let opts = options(tmp.path().to_path_buf());
    let plan = plan_with(tmp.path(), &["claude-md"]);
    let receipt = stage_backup::run(&opts, &plan, &audit).await.unwrap();
    let name = receipt.tarball_path.file_name().unwrap().to_str().unwrap();
    // Format: <ISO8601-stamp>-<agent>.tar.gz with the agent at the end.
    assert!(name.contains("claude-code"));
    assert!(name.ends_with(".tar.gz"));
    // ISO 8601 marker characters
    assert!(name.contains('T'));
    assert!(name.contains('Z'));
}
