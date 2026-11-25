//! The flags the kx binary starts the Claude CLI with, end to end.
//!
//! A stand-in `claude` first on `PATH` records its arguments and answers
//! without a model, so these tests run the real binary, the real provider
//! and the real OS sandbox without calling any API.

#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const FAKE_CLAUDE: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "2.9.0 (Claude Code)"
  exit 0
fi
for a in "$@"; do printf '%s\n' "$a"; done > "$PWD/claude-args.txt"
printf '{"type":"result","subtype":"success","result":"ok","is_error":false,"session_id":"s1"}\n'
"#;

struct Run {
    ok: bool,
    stderr: String,
    /// The CLI's arguments, `None` when kx never started it.
    args: Option<Vec<String>>,
}

/// Run `kx --no-memory <extra> dev hello` in a fresh HOME and project, with
/// `config` as `~/.kx/config.toml` when given.
fn run_kx(extra: &[&str], config: Option<&str>) -> Run {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let bin = root.path().join("bin");
    let project = home.join("proj");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        project.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let claude = bin.join("claude");
    std::fs::write(&claude, FAKE_CLAUDE).unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    if let Some(config) = config {
        std::fs::create_dir_all(home.join(".kx")).unwrap();
        std::fs::write(home.join(".kx/config.toml"), config).unwrap();
    }

    let out = Command::new(env!("CARGO_BIN_EXE_kx"))
        .arg("--no-memory")
        .args(extra)
        .args(["dev", "hello"])
        .current_dir(&project)
        .env_clear()
        .env("HOME", &home)
        .env("PATH", path_with(&bin))
        .output()
        .unwrap();
    let args = read_args(&project.join("claude-args.txt"));
    Run {
        ok: out.status.success(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        args,
    }
}

fn path_with(bin: &Path) -> String {
    format!("{}:/usr/bin:/bin", bin.display())
}

fn read_args(path: &PathBuf) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    Some(text.lines().map(str::to_string).collect())
}

/// The value after each `flag`.
fn values<'a>(args: &'a [String], flag: &str) -> Vec<&'a str> {
    args.windows(2)
        .filter(|w| w[0] == flag)
        .map(|w| w[1].as_str())
        .collect()
}

/// The arguments of a run that must have reached the CLI. kx refuses to
/// start without an OS sandbox, and these tests do not skip that case: a
/// skipped run would hide the regressions they exist for.
fn cli_args(run: Run) -> Vec<String> {
    match run.args {
        Some(args) => args,
        None => panic!("the CLI was not started: {}", run.stderr),
    }
}

#[test]
fn by_default_the_cli_gets_a_list_without_a_shell() {
    let args = cli_args(run_kx(&[], None));
    assert!(!args.iter().any(|a| a == "--dangerously-skip-permissions"));
    assert_eq!(
        values(&args, "--tools"),
        vec!["Read,Glob,Grep,Edit,Write,NotebookEdit,TodoWrite"]
    );
    assert_eq!(values(&args, "--permission-mode"), vec!["dontAsk"]);
}

/// The provider factory ignores case; kx must too, or `Claude-Code`
/// would skip the list and run with the permission checks off.
#[test]
fn the_provider_name_case_does_not_skip_the_list() {
    let args = cli_args(run_kx(&["--provider", "Claude-Code"], None));
    assert!(!args.iter().any(|a| a == "--dangerously-skip-permissions"));
    assert_eq!(values(&args, "--permission-mode"), vec!["dontAsk"]);
}

#[test]
fn the_user_config_and_the_flag_choose_the_list() {
    let all = Some("cli_tools = \"all\"\n");
    let args = cli_args(run_kx(&[], all));
    assert!(args.iter().any(|a| a == "--dangerously-skip-permissions"));

    let args = cli_args(run_kx(&["--cli-tools", "Read,Bash(cargo test *)"], all));
    assert!(!args.iter().any(|a| a == "--dangerously-skip-permissions"));
    assert_eq!(values(&args, "--tools"), vec!["Read,Bash"]);
    assert_eq!(
        values(&args, "--allowedTools"),
        vec!["Read", "Bash(cargo test *)"]
    );

    // A fetch tool web_fetch keeps off is left out of the list.
    let args = cli_args(run_kx(&["--cli-tools", "Read,WebFetch"], None));
    assert_eq!(values(&args, "--tools"), vec!["Read"]);
}

#[test]
fn opencode_and_bad_lists_are_refused_before_anything_starts() {
    for (extra, reason) in [
        (&["--provider", "OpenCode"][..], "restricted tool list"),
        (&["--cli-tools", "none"][..], "cannot run with no tools"),
        (&["--cli-tools", "WebFetch"][..], "names only WebFetch"),
        (&["--provider", " opencode"][..], "restricted tool list"),
    ] {
        let run = run_kx(extra, None);
        assert!(!run.ok, "{extra:?} was accepted");
        assert!(run.args.is_none(), "{extra:?} started the CLI");
        assert!(run.stderr.contains(reason), "{extra:?}: {}", run.stderr);
    }
}
