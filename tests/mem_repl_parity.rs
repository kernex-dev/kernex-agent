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
//! As of `memory-typed-row-shape` Slice B (kernex-memory 0.7.0) the
//! trait returns typed `MessageRow` / `HistoryRow` and exposes
//! `MemoryStore::get_message_by_id`. The harness now has a stable record
//! shape to assert against; what remains is the `src/lib.rs` extraction
//! that lets integration tests reach `mem::cli::*` directly. Once that
//! lands the harness flips to: seed a `kernex_memory::Store`, call each
//! `mem::cli::*` handler, assert the returned record fields are
//! observable and stable across two consecutive calls (no row drift).
//! Tracked as FU-D-AG-06 (lib.rs extraction for parity harness).

#![cfg(feature = "memory-cli")]

#[test]
#[ignore = "REPL+CLI share mem::cli::search; structural parity. Flips on when lib.rs extraction lands (FU-D-AG-06)."]
fn parity_search() {}

#[test]
#[ignore = "REPL+CLI share mem::cli::history; structural parity. Flips on when lib.rs extraction lands (FU-D-AG-06)."]
fn parity_history() {}

#[test]
#[ignore = "REPL+CLI share mem::cli::stats; structural parity. Flips on when lib.rs extraction lands (FU-D-AG-06)."]
fn parity_stats() {}

#[test]
#[ignore = "REPL+CLI share mem::cli::facts_list; structural parity. Flips on when lib.rs extraction lands (FU-D-AG-06)."]
fn parity_facts_list() {}

#[test]
#[ignore = "REPL+CLI share mem::cli::facts_delete (soft-delete); structural parity. Flips on when lib.rs extraction lands (FU-D-AG-06)."]
fn parity_facts_delete() {}
