//! End-to-end integration test for the Codex CLI adapter (F-1.10).
//!
//! Drives the full configurator pipeline (DETECT -> RESOLVE -> REVIEW
//! -> BACKUP -> APPLY -> VERIFY -> REPORT) against a TempDir HOME plus
//! a TempDir project root, and asserts:
//!
//! 1. `~/.codex/config.toml`, `~/.codex/output-style.md`, and
//!    `<cwd>/AGENTS.md` are all written under the install fixture.
//! 2. `config.toml` carries the rendered `[mcp_servers.kernex]` block;
//!    pre-seeded non-kernex `[mcp_servers.*]` entries are preserved.
//! 3. `AGENTS.md` carries the `<!-- kernex:begin --> ... <!-- kernex:end -->`
//!    marker block; pre-existing user prose around the markers is
//!    preserved byte-for-byte.
//! 4. The pipeline emits per-stage trace events with no skipped stage
//!    and a terminal `install.summary` event of status `success`.
//!
//! Covers Sprint F-1 task F-1.10 plus the configurator render/write
//! seam refactor that landed in the same PR (`stage_apply.rs` dispatch
//! on `(agent, component)` and codex merge helper wiring).

#![cfg(all(feature = "agent-claude", feature = "agent-codex"))]

use std::fs;
use std::path::PathBuf;

use kernex_agent::configurator::{run_with_audit, InstallOptions, InstallStatus};
use kernex_agent::install::audit::AuditWriter;
use tempfile::TempDir;

fn codex_opts(home: PathBuf, cwd: PathBuf) -> InstallOptions {
    InstallOptions {
        agent: "codex".to_string(),
        preset: "solo-dev".to_string(),
        yes: true,
        dry_run: false,
        verify_deep: false,
        home,
        cwd: Some(cwd),
    }
}

#[tokio::test]
async fn clean_home_writes_all_three_codex_components() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let audit = AuditWriter::new(home.path()).unwrap();

    let report = run_with_audit(
        codex_opts(home.path().to_path_buf(), project.path().to_path_buf()),
        &audit,
    )
    .await
    .expect("codex install ok on clean home");

    let codex_dir = home.path().join(".codex");
    let config = codex_dir.join("config.toml");
    let output_style = codex_dir.join("output-style.md");
    let agents_md = project.path().join("AGENTS.md");

    for (label, path) in [
        ("config.toml", &config),
        ("output-style.md", &output_style),
        ("AGENTS.md", &agents_md),
    ] {
        assert!(path.exists(), "{label} not written at {path:?}");
        assert!(
            fs::metadata(path).unwrap().len() > 0,
            "{label} is empty at {path:?}"
        );
    }

    let config_body = fs::read_to_string(&config).unwrap();
    assert!(
        config_body.contains("[mcp_servers.kernex]"),
        "config.toml missing kernex MCP entry: {config_body}"
    );
    assert!(
        config_body.contains("command = \"kx\""),
        "config.toml kernex command not rendered: {config_body}"
    );

    let agents_body = fs::read_to_string(&agents_md).unwrap();
    assert!(
        agents_body.contains("<!-- kernex:begin -->"),
        "AGENTS.md missing kernex begin marker: {agents_body}"
    );
    assert!(
        agents_body.contains("<!-- kernex:end -->"),
        "AGENTS.md missing kernex end marker: {agents_body}"
    );
    assert!(
        agents_body.contains("kernex-managed agent instructions"),
        "AGENTS.md missing rendered template body: {agents_body}"
    );

    assert!(matches!(
        report.status,
        InstallStatus::Success | InstallStatus::SuccessWithVerifyFailures
    ));

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

    let summary = events
        .iter()
        .find(|e| e["event"] == "install.summary")
        .expect("install.summary event present");
    assert_eq!(summary["status"], "success");
}

#[tokio::test]
async fn pre_seeded_config_preserves_non_kernex_mcp_servers() {
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let audit = AuditWriter::new(home.path()).unwrap();

    // Seed ~/.codex/config.toml with two non-kernex `[mcp_servers.*]`
    // sub-tables plus an unrelated top-level key. The merge MUST keep
    // all of them while upserting kernex's own block.
    let codex_dir = home.path().join(".codex");
    fs::create_dir_all(&codex_dir).unwrap();
    let seeded = r#"# Pre-existing user comment

[other_setting]
flag = true

[mcp_servers.figma]
command = "figma-mcp"
args = ["--port", "8765"]

[mcp_servers.codegraph]
command = "codegraph"
args = ["serve"]
"#;
    fs::write(codex_dir.join("config.toml"), seeded).unwrap();

    // Pre-existing AGENTS.md with user prose; the marker block does
    // not yet exist. The merge MUST append the kernex block and leave
    // the pre-existing prose untouched.
    let prior_agents = "# Project agent notes\n\nDo not remove this paragraph; it is user-owned.\n";
    fs::write(project.path().join("AGENTS.md"), prior_agents).unwrap();

    let report = run_with_audit(
        codex_opts(home.path().to_path_buf(), project.path().to_path_buf()),
        &audit,
    )
    .await
    .expect("codex install ok on pre-seeded home");

    let config_body = fs::read_to_string(codex_dir.join("config.toml")).unwrap();
    assert!(
        config_body.contains("[mcp_servers.figma]"),
        "non-kernex figma entry dropped after merge: {config_body}"
    );
    assert!(
        config_body.contains("command = \"figma-mcp\""),
        "non-kernex figma command dropped after merge: {config_body}"
    );
    assert!(
        config_body.contains("[mcp_servers.codegraph]"),
        "non-kernex codegraph entry dropped after merge: {config_body}"
    );
    assert!(
        config_body.contains("[mcp_servers.kernex]"),
        "kernex entry not upserted into pre-seeded config: {config_body}"
    );
    assert!(
        config_body.contains("[other_setting]"),
        "unrelated top-level table dropped after merge: {config_body}"
    );
    assert!(
        config_body.contains("# Pre-existing user comment"),
        "leading comment dropped after merge: {config_body}"
    );

    let agents_body = fs::read_to_string(project.path().join("AGENTS.md")).unwrap();
    assert!(
        agents_body.contains("# Project agent notes"),
        "user agents-md heading dropped: {agents_body}"
    );
    assert!(
        agents_body.contains("Do not remove this paragraph; it is user-owned."),
        "user paragraph dropped: {agents_body}"
    );
    assert!(
        agents_body.contains("<!-- kernex:begin -->")
            && agents_body.contains("<!-- kernex:end -->"),
        "kernex marker block missing: {agents_body}"
    );

    assert!(matches!(
        report.status,
        InstallStatus::Success | InstallStatus::SuccessWithVerifyFailures
    ));
}

#[tokio::test]
async fn re_run_is_idempotent_for_codex() {
    // Two back-to-back installs must produce the same final file
    // contents (no drift, no duplicate marker blocks).
    let home = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    let audit1 = AuditWriter::new(home.path()).unwrap();
    let audit2 = AuditWriter::new(home.path()).unwrap();

    let _ = run_with_audit(
        codex_opts(home.path().to_path_buf(), project.path().to_path_buf()),
        &audit1,
    )
    .await
    .expect("first install ok");

    let first_config = fs::read_to_string(home.path().join(".codex").join("config.toml")).unwrap();
    let first_agents = fs::read_to_string(project.path().join("AGENTS.md")).unwrap();

    let _ = run_with_audit(
        codex_opts(home.path().to_path_buf(), project.path().to_path_buf()),
        &audit2,
    )
    .await
    .expect("second install ok");

    let second_config = fs::read_to_string(home.path().join(".codex").join("config.toml")).unwrap();
    let second_agents = fs::read_to_string(project.path().join("AGENTS.md")).unwrap();

    assert_eq!(
        first_config, second_config,
        "config.toml diverged across two consecutive installs"
    );
    assert_eq!(
        first_agents, second_agents,
        "AGENTS.md diverged across two consecutive installs"
    );

    let begin_count = second_agents.matches("<!-- kernex:begin -->").count();
    let end_count = second_agents.matches("<!-- kernex:end -->").count();
    assert_eq!(
        begin_count, 1,
        "duplicate kernex:begin marker after re-run: {second_agents}"
    );
    assert_eq!(
        end_count, 1,
        "duplicate kernex:end marker after re-run: {second_agents}"
    );
}
