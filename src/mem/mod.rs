//! `kx mem *` subcommand surface.
//!
//! See [openspec/changes/kx-mem-cli-promotion/](../../openspec/changes/kx-mem-cli-promotion)
//! for the change spec. This module owns the CLI subcommand handlers, the
//! auto-JSON renderer, and the structured error type that maps to the
//! exit-code taxonomy (ADR-005).
//!
//! Handlers are stubs in this commit; they return `CliError::NotImplemented`
//! which the dispatcher surfaces as an exit-2 error with a `Try:` hint. The
//! handler bodies fill in across follow-up commits per `tasks.md` Step 2.

#![cfg(feature = "memory-cli")]

pub mod cli;
pub mod errors;
pub mod render;
pub mod types;

use crate::cli::MemAction;

/// Dispatch a `kx mem ...` invocation to the matching handler.
///
/// In this scaffold commit every handler returns `CliError::NotImplemented`.
/// Subsequent commits replace the stub bodies with real trait calls into
/// `kernex_memory::MemoryStore`.
pub async fn dispatch(action: MemAction) -> anyhow::Result<()> {
    match action {
        MemAction::Search { .. } => cli::search().await,
        MemAction::Get { .. } => cli::get().await,
        MemAction::History { .. } => cli::history().await,
        MemAction::Stats { .. } => cli::stats().await,
        MemAction::Facts { .. } => cli::facts().await,
        MemAction::Save(_) => cli::save().await,
    }
    .map_err(anyhow::Error::from)
}
