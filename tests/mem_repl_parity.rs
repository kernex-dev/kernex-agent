//! REPL parity harness for `kx mem *` (ADR-009 enforcement).
//!
//! Parity contract: invoking a memory operation through a REPL slash
//! command and through the equivalent `kx mem *` subcommand must operate
//! on the same underlying record set. Render path differs (REPL colored
//! table vs CLI auto-JSON / `--json`); the data does not.
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
//! As of Step 2.14 the REPL slash commands in `src/commands.rs` delegate
//! through the same `crate::mem::cli::*` handler functions that the CLI
//! subcommands dispatch to. Parity is now **structural**: the two paths
//! share code, so byte-equivalence on the record set is a property of the
//! single shared handler, not of two competing implementations. There is
//! no remaining divergence the harness can usefully catch against the
//! current trait surface (which already returns string-typed rows).
//!
//! The harness is therefore retained as a placeholder until
//! `memory-typed-row-shape` Slice B lands. Slice B replaces the trait's
//! `(String, String, String)` tuples with typed `MessageRow` /
//! `HistoryRow` and adds `MemoryStore::get_message_by_id`. At that point
//! the harness flips to: seed a `kernex_memory::Store`, call each
//! `mem::cli::*` handler, assert the returned record fields are
//! observable and stable across two consecutive calls (no row drift).
//! That assertion path needs `src/lib.rs` to re-export the handlers; the
//! lib.rs extraction is deferred until Slice B because there is no
//! observable test before then.

#![cfg(feature = "memory-cli")]

#[test]
#[ignore = "REPL+CLI share mem::cli::search; structural parity. Flips on at Slice B with handler-call assertions."]
fn parity_search() {}

#[test]
#[ignore = "REPL+CLI share mem::cli::history; structural parity. Flips on at Slice B with handler-call assertions."]
fn parity_history() {}

#[test]
#[ignore = "REPL+CLI share mem::cli::stats; structural parity. Flips on at Slice B with handler-call assertions."]
fn parity_stats() {}

#[test]
#[ignore = "REPL+CLI share mem::cli::facts_list; structural parity. Flips on at Slice B with handler-call assertions."]
fn parity_facts_list() {}

#[test]
#[ignore = "REPL+CLI share mem::cli::facts_delete (soft-delete); structural parity. Flips on at Slice B with handler-call assertions."]
fn parity_facts_delete() {}
