//! Integration tests for §12 CLI dispatcher.
//!
//! Exercises kernex_agent::install::cli::dispatch with a TempDir HOME
//! override (KX_HOME) so the test never touches the real user HOME.

#![cfg(feature = "agent-claude")]

use kernex_agent::install::cli::{dispatch, InstallArgs};
use std::fs;
use tempfile::TempDir;
use tokio::sync::Mutex;

/// Tests in this file flip env vars (KX_HOME) which is process-global.
/// Serialize them so they don't race when cargo runs tests in parallel.
/// `tokio::sync::Mutex` is safe to hold across `.await` points (unlike
/// `std::sync::Mutex`).
static ENV_LOCK: Mutex<()> = Mutex::const_new(());

fn args_solo_dev(yes: bool, dry_run: bool) -> InstallArgs {
    InstallArgs {
        agent: "claude-code".to_string(),
        preset: "solo-dev".to_string(),
        yes,
        dry_run,
        verify_deep: false,
    }
}

#[tokio::test]
async fn happy_path_writes_files_and_exits_zero() {
    let _g = ENV_LOCK.lock().await;
    let tmp = TempDir::new().unwrap();
    let saved = std::env::var_os("KX_HOME");
    unsafe { std::env::set_var("KX_HOME", tmp.path()) };

    let code = dispatch(args_solo_dev(true, false)).await.unwrap();
    if let Some(prior) = saved {
        unsafe { std::env::set_var("KX_HOME", prior) };
    } else {
        unsafe { std::env::remove_var("KX_HOME") };
    }

    assert_eq!(code, 0);
    let claude = tmp.path().join(".claude");
    assert!(claude.join("CLAUDE.md").exists());
    assert!(claude.join("mcp-servers.json").exists());
    assert!(claude.join("output-style.md").exists());
    // Audit log is present.
    let audit_dir = tmp.path().join(".kx").join("audit");
    let entries: Vec<_> = fs::read_dir(audit_dir).unwrap().collect();
    assert!(!entries.is_empty(), "audit log should be written");
}

#[tokio::test]
async fn dry_run_writes_nothing_and_exits_zero() {
    let _g = ENV_LOCK.lock().await;
    let tmp = TempDir::new().unwrap();
    let saved = std::env::var_os("KX_HOME");
    unsafe { std::env::set_var("KX_HOME", tmp.path()) };

    let code = dispatch(args_solo_dev(true, true)).await.unwrap();
    if let Some(prior) = saved {
        unsafe { std::env::set_var("KX_HOME", prior) };
    } else {
        unsafe { std::env::remove_var("KX_HOME") };
    }

    assert_eq!(code, 0);
    assert!(
        !tmp.path().join(".claude").exists(),
        ".claude/ should NOT exist after dry-run"
    );
}

#[tokio::test]
async fn unknown_preset_returns_exit_two() {
    let _g = ENV_LOCK.lock().await;
    let tmp = TempDir::new().unwrap();
    let saved = std::env::var_os("KX_HOME");
    unsafe { std::env::set_var("KX_HOME", tmp.path()) };

    let mut args = args_solo_dev(true, false);
    args.preset = "does-not-exist".to_string();
    let code = dispatch(args).await.unwrap();
    if let Some(prior) = saved {
        unsafe { std::env::set_var("KX_HOME", prior) };
    } else {
        unsafe { std::env::remove_var("KX_HOME") };
    }

    assert_eq!(code, 2, "unknown preset must surface as usage exit 2");
}

#[tokio::test]
async fn unknown_agent_returns_exit_two() {
    let _g = ENV_LOCK.lock().await;
    let tmp = TempDir::new().unwrap();
    let saved = std::env::var_os("KX_HOME");
    unsafe { std::env::set_var("KX_HOME", tmp.path()) };

    let mut args = args_solo_dev(true, false);
    args.agent = "not-an-agent".to_string();
    let code = dispatch(args).await.unwrap();
    if let Some(prior) = saved {
        unsafe { std::env::set_var("KX_HOME", prior) };
    } else {
        unsafe { std::env::remove_var("KX_HOME") };
    }

    assert_eq!(code, 2, "unknown agent must surface as usage exit 2");
}
