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
//! share code, so byte-equivalence on the record set is a property of
//! the single shared handler.
//!
//! What this harness covers: seed a `kernex_memory::Store`, call each
//! `mem::cli::*` handler through the library crate's public surface
//! (reachable since `src/lib.rs` was extracted), and assert the returned
//! record fields are observable and stable across two consecutive calls
//! (no row drift on idempotent reads).

#![cfg(feature = "memory-cli")]

use std::sync::Arc;

use kernex_agent::mem::cli::{
    facts_add, facts_delete, facts_list, history, search, stats, HistoryOpts, SearchOpts,
    StatsOpts, CLI_CHANNEL, CLI_SENDER_ID,
};
use kernex_core::config::MemoryConfig;
use kernex_core::message::{Request, Response};
use kernex_memory::{into_handle, MemoryStore, Store};
use tempfile::TempDir;

const TEST_PROJECT: &str = "parity-demo";

/// Build an isolated `Store` rooted under a temp dir and seed it with
/// the supplied `(user_text, assistant_text)` tuples. All exchanges
/// land in the same `TEST_PROJECT` so a single call sequence exercises
/// the project-scoped surface uniformly.
async fn seeded_store(seeds: &[(&str, &str)]) -> (TempDir, Arc<dyn MemoryStore>) {
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("memory.db");
    let cfg = MemoryConfig {
        db_path: db_path.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let store = Store::new(&cfg).await.unwrap();
    for (user_text, asst_text) in seeds {
        let req = Request::text(CLI_SENDER_ID, user_text);
        let resp = Response {
            text: (*asst_text).to_string(),
            ..Default::default()
        };
        store
            .store_exchange(CLI_CHANNEL, &req, &resp, TEST_PROJECT)
            .await
            .unwrap();
    }
    (tmp, into_handle(store))
}

fn search_opts(query: &str, limit: usize) -> SearchOpts {
    SearchOpts {
        query: query.to_string(),
        limit,
        since: None,
        kind: None,
    }
}

#[tokio::test]
async fn parity_search() {
    let (_tmp, store) = seeded_store(&[
        ("first marker question", "first answer"),
        ("second marker question", "second answer"),
    ])
    .await;

    let first = search(store.as_ref(), search_opts("marker", 10))
        .await
        .unwrap();
    let second = search(store.as_ref(), search_opts("marker", 10))
        .await
        .unwrap();

    // Idempotent reads return the same row count and id set across calls.
    // This is the structural-parity assertion: REPL and CLI both reach
    // this exact handler; if it drifts, both surfaces drift together.
    assert!(!first.is_empty(), "seeded store must match the query");
    assert_eq!(first.len(), second.len(), "stable row count across calls");
    let first_ids: Vec<&str> = first.iter().map(|r| r.id.as_str()).collect();
    let second_ids: Vec<&str> = second.iter().map(|r| r.id.as_str()).collect();
    assert_eq!(first_ids, second_ids, "stable id ordering across calls");
}

#[tokio::test]
async fn parity_history() {
    let (_tmp, store) = seeded_store(&[
        ("history seed alpha", "ack alpha"),
        ("history seed beta", "ack beta"),
    ])
    .await;

    let opts = || HistoryOpts {
        last: 10,
        project: TEST_PROJECT.to_string(),
    };

    let first = history(store.as_ref(), opts()).await.unwrap();
    let second = history(store.as_ref(), opts()).await.unwrap();

    assert_eq!(first.len(), second.len(), "stable row count across calls");
    // `project` is echoed onto every row and must match the resolved
    // input. Locks in the structural contract that resolving project
    // upstream and rendering downstream see the same string.
    for row in &first {
        assert_eq!(row.project, TEST_PROJECT);
    }
}

#[tokio::test]
async fn parity_stats() {
    let (_tmp, store) =
        seeded_store(&[("stats seed one", "ack one"), ("stats seed two", "ack two")]).await;

    let opts = || StatsOpts {
        project: TEST_PROJECT.to_string(),
    };

    let first = stats(store.as_ref(), opts()).await.unwrap();
    let second = stats(store.as_ref(), opts()).await.unwrap();

    assert_eq!(first.project, TEST_PROJECT);
    assert_eq!(first.project, second.project, "project echo stable");
    assert_eq!(
        first.conversations, second.conversations,
        "conversation count stable across consecutive idempotent reads"
    );
    assert_eq!(first.facts, second.facts, "fact count stable");
}

#[tokio::test]
async fn parity_facts_list() {
    let (_tmp, store) = seeded_store(&[]).await;
    facts_add(store.as_ref(), "stack", "rust").await.unwrap();
    facts_add(store.as_ref(), "editor", "neovim").await.unwrap();

    let first = facts_list(store.as_ref()).await.unwrap();
    let second = facts_list(store.as_ref()).await.unwrap();

    assert_eq!(first.len(), 2, "two facts seeded");
    assert_eq!(first.len(), second.len(), "stable count across calls");
    let mut first_keys: Vec<&str> = first.iter().map(|r| r.key.as_str()).collect();
    let mut second_keys: Vec<&str> = second.iter().map(|r| r.key.as_str()).collect();
    first_keys.sort_unstable();
    second_keys.sort_unstable();
    assert_eq!(first_keys, second_keys, "stable key set across calls");
}

#[tokio::test]
async fn parity_facts_delete() {
    // Soft-delete contract: after delete, facts_list excludes the row
    // (CC-9). The REPL `/facts delete <key>` and `kx mem facts delete
    // <key>` reach the same handler; whatever they do, they do
    // together.
    let (_tmp, store) = seeded_store(&[]).await;
    facts_add(store.as_ref(), "auth-pattern", "OIDC + PKCE")
        .await
        .unwrap();
    facts_add(store.as_ref(), "stack", "rust").await.unwrap();

    facts_delete(store.as_ref(), "auth-pattern").await.unwrap();

    let listing = facts_list(store.as_ref()).await.unwrap();
    assert!(
        listing.iter().all(|r| r.key != "auth-pattern"),
        "soft-deleted key must not appear in default listing"
    );
    assert_eq!(listing.len(), 1, "only the non-deleted key remains");
    assert_eq!(listing[0].key, "stack");
}
