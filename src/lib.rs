//! Library surface for `kernex-agent`.
//!
//! The binary entry point lives in [`src/main.rs`]. This `lib.rs` exists
//! so integration tests under `tests/` can reach the shared module
//! surface (notably `mem::cli::*`) without going through the binary
//! crate. Module declarations are `pub` re-exports of the same modules
//! `main.rs` previously declared as bin-only `mod` entries; nothing
//! moves on disk.
//!
//! Adding a new top-level module: declare it here as `pub mod`, then
//! reference it from `main.rs` via the `kernex_agent::` path. Do NOT
//! also declare it in `main.rs` (that would compile the module twice
//! and split its `crate::` paths between two crates).

#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod builtins;
pub mod cli;
pub mod commands;
pub mod config;
pub mod loader;
#[cfg(feature = "memory-cli")]
pub mod mem;
pub mod prompts;
pub mod runtime_glue;
pub mod scheduler;
#[cfg(feature = "serve")]
pub mod serve;
pub mod skills;
pub mod stack;
pub mod utils;

// Re-export runtime glue at the crate root so sibling modules can keep
// referencing `crate::build_provider`, `crate::CliHookRunner`, etc.
// without sub-module qualifiers. This is the seam that lets `serve`
// continue compiling unchanged after the bin -> lib extraction.
pub use runtime_glue::{
    api_key_var, build_provider, context_needs, data_dir_for, CliHookRunner, ProviderFlags,
    ProviderSpec, PROVIDERS,
};

#[cfg(any(
    feature = "agent-claude",
    feature = "agent-codex",
    feature = "agent-opencode",
    feature = "agent-cursor",
    feature = "agent-cline",
    feature = "agent-windsurf",
))]
pub mod adapters;

#[cfg(feature = "tui")]
pub mod tui;
