//! REPL parity harness for `kx mem *` (ADR-009 enforcement).
//!
//! The harness asserts byte-equivalence on the underlying record set when a
//! given operation is invoked via the REPL slash command vs the equivalent
//! `kx mem *` CLI subcommand. Render path differs (table vs JSON); the data
//! does not.
//!
//! Parity matrix (from `openspec/changes/kx-mem-cli-promotion/design.md`
//! ADR-009):
//!
//! | Slash command            | CLI subcommand                  |
//! |--------------------------|---------------------------------|
//! | `/search <q>`            | `kx mem search <q>`             |
//! | `/history`               | `kx mem history`                |
//! | `/memory`                | `kx mem stats`                  |
//! | `/facts`                 | `kx mem facts list`             |
//! | `/facts delete <key>`    | `kx mem facts delete <key>`     |
//!
//! Each row below is a placeholder test marked `#[ignore]`. The
//! `#[ignore]` attribute is removed in the same commit that lands the
//! corresponding handler (see `tasks.md` Step 1.1 + Step 2.x).
//!
//! Implementation note: `kernex-agent` does not currently expose a `lib.rs`,
//! so integration tests cannot call internal handler functions directly. The
//! parity harness lands via one of two paths in a follow-up commit:
//!
//! 1. Extract a thin `src/lib.rs` that re-exports `mem::cli::*`, then call
//!    handler fns directly with seeded `kernex_memory::MemoryStore` stores.
//! 2. Spawn `target/debug/kx` as a subprocess and diff its piped JSON output
//!    against the equivalent REPL-mode invocation captured via expect or a
//!    similar pty harness.
//!
//! The handler-implementation commit (Step 2.3 onward) decides the path
//! based on what compiles cleanly under the existing `#![deny(warnings)]`
//! discipline.

#![cfg(feature = "memory-cli")]

#[test]
#[ignore = "scaffold placeholder; flips on with the search handler in Step 2.3"]
fn parity_search() {}

#[test]
#[ignore = "scaffold placeholder; flips on with the history handler in Step 2.5"]
fn parity_history() {}

#[test]
#[ignore = "scaffold placeholder; flips on with the stats handler in Step 2.6"]
fn parity_stats() {}

#[test]
#[ignore = "scaffold placeholder; flips on with the facts-list handler in Step 2.7"]
fn parity_facts_list() {}

#[test]
#[ignore = "scaffold placeholder; flips on with the facts-delete handler in Step 2.10"]
fn parity_facts_delete() {}
