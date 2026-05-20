//! CLI dispatcher for `kx install` per §12.
//!
//! Parses the flag bundle, resolves `$HOME` (or `$KX_HOME` override for
//! tests), runs the configurator pipeline, maps the typed report to an
//! exit code.

use std::path::PathBuf;

use crate::configurator::stage_report::exit_code_for;
use crate::configurator::{run, InstallOptions};

/// Parsed input from the `kx install` clap subcommand.
pub struct InstallArgs {
    pub agent: String,
    pub preset: String,
    pub yes: bool,
    pub dry_run: bool,
    pub verify_deep: bool,
}

/// Entry point invoked from `main.rs`. Returns the process exit code.
pub async fn dispatch(args: InstallArgs) -> anyhow::Result<i32> {
    let home = resolve_home()?;
    let cwd = std::env::current_dir().ok();
    let opts = InstallOptions {
        agent: args.agent,
        preset: args.preset,
        yes: args.yes,
        dry_run: args.dry_run,
        verify_deep: args.verify_deep,
        home,
        cwd,
    };
    match run(opts).await {
        Ok(report) => Ok(exit_code_for(&report)),
        Err(err) => {
            eprintln!("kx install failed: {err}");
            Ok(map_error_to_exit_code(&err))
        }
    }
}

fn resolve_home() -> anyhow::Result<PathBuf> {
    if let Ok(override_home) = std::env::var("KX_HOME") {
        return Ok(PathBuf::from(override_home));
    }
    let home = std::env::var("HOME")
        .map_err(|_| anyhow::anyhow!("$HOME is not set; cannot resolve install root"))?;
    Ok(PathBuf::from(home))
}

fn map_error_to_exit_code(err: &crate::configurator::InstallError) -> i32 {
    use crate::configurator::InstallError;
    match err {
        InstallError::UnknownAgent(_)
        | InstallError::UnknownPreset(_)
        | InstallError::NonInteractive => 2,
        InstallError::UserDeclined => 0,
        InstallError::SandboxRefused { .. } | InstallError::Transient(_) => 7,
        InstallError::PathNotInPlan(_) | InstallError::Permanent(_) | InstallError::Io(_) => 5,
    }
}
