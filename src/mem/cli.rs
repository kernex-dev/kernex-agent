//! Handlers for `kx mem *` subcommands.
//!
//! Each handler is a pure async function that takes a `MemoryStore`
//! handle plus typed options and returns the typed record set. The
//! dispatcher in [`super`] is responsible for opening the store, picking
//! the renderer (JSON vs table), and threading render flags through.
//!
//! Keeping handlers pure lets the parity harness in
//! `tests/mem_repl_parity.rs` assert byte-equivalence on the underlying
//! record set without involving stdout or terminal detection.

use std::time::{Duration, SystemTime};

use chrono::{DateTime, Utc};
use kernex_memory::{HistoryRow, MemoryError, MemoryStore, MessageRow, ObservationType, SaveEntry};

use crate::mem::errors::CliError;
use crate::mem::types::{
    FactsRecord, HistoryRecord, SaveRecord, SearchRecord, StatsRecord, OBSERVATION_TYPES,
};

/// Classify a `kernex_memory::MemoryError` into a `CliError` so the
/// dispatch site emits the right exit code. SQLite contention (`database
/// is locked` / `SQLITE_BUSY`) and sqlx pool timeouts surface as
/// [`CliError::Transient`] (exit 7) so scripts and a future `--retry`
/// flag can branch without parsing the message string. Every other
/// `MemoryError` flavor maps to [`CliError::Runtime`] (exit 5).
///
/// `op` describes the failing operation in human terms (e.g.
/// `"memory search"`, `"history fetch"`). It is folded into the
/// message so the operator sees both what failed and why.
pub fn classify_memory_error(err: MemoryError, op: &str) -> CliError {
    if memory_error_is_transient(&err) {
        return CliError::Transient {
            message: format!("{op} hit transient contention: {err}"),
            hint: "Retry in a moment. If this reproduces under load, \
                   reduce concurrent writers or open an issue."
                .to_string(),
            retry_after: None,
        };
    }
    CliError::Runtime {
        message: format!("{op} failed: {err}"),
        hint: "Run `kx doctor` to verify the local memory database.".to_string(),
    }
}

/// True when the `MemoryError` represents a retryable contention class
/// (SQLite busy / database locked / sqlx pool timeout). Detection runs
/// on the `Display` output of the inner `sqlx::Error` carried by
/// `MemoryError::Sqlite { source, .. }`. `MemoryError::Io / Serde /
/// Logic` are never transient.
///
/// String detection is intentional: `sqlx` is not a direct dependency
/// of kernex-agent and the canonical SQLite text `database is locked`
/// plus sqlx's Display strings `pool timed out` / `closed pool` are
/// stable across sqlx 0.x releases. If a future kernex-memory release
/// exposes a typed `is_transient()` helper on `MemoryError`, this
/// function can switch to it without changing the signature here.
fn memory_error_is_transient(err: &MemoryError) -> bool {
    let MemoryError::Sqlite { source, .. } = err else {
        return false;
    };
    let msg = format!("{source}").to_ascii_lowercase();
    msg.contains("database is locked")
        || msg.contains("pool timed out")
        || msg.contains("sqlite_busy")
        || msg.contains("closed pool")
}

/// Project a `SystemTime` to the SQLite `TIMESTAMP` shape used by the
/// JSON output (`"%Y-%m-%d %H:%M:%S"`, UTC). Slice B of
/// `memory-typed-row-shape` types the trait's `timestamp` and
/// `updated_at` columns as `SystemTime`; we project back to a string at
/// the JSON-projection boundary so the CC-1 contract stays stable.
fn format_timestamp(t: SystemTime) -> String {
    let dt: DateTime<Utc> = t.into();
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Sender identifier the CLI uses when reading from the memory store.
/// Matches the REPL convention so `/search` and `kx mem search` operate
/// on the same row set.
pub const CLI_SENDER_ID: &str = "user";

/// Channel identifier the CLI uses when calling per-channel trait
/// methods (`get_history`, `close_current_conversation`). Matches the
/// REPL convention used by `cmd_dev` so the CLI and REPL share rows.
pub const CLI_CHANNEL: &str = "cli";

/// Default record count for `kx mem history` when `--last` is omitted.
/// Matches the spec scenario S-history-1.
pub const DEFAULT_HISTORY_LIMIT: usize = 20;

/// Options accepted by `kx mem search`. Mirror the CLI flags one-to-one.
#[derive(Debug, Clone)]
pub struct SearchOpts {
    /// FTS5 query string (operator content; never logged as a span field).
    pub query: String,
    /// Operator-supplied upper bound on the result set.
    pub limit: usize,
    /// Recency window (`Nd`/`Nh`/`Nm`/`Nw`). Parsed by `parse_since`.
    pub since: Option<String>,
    /// Observation-type filter (must be one of `OBSERVATION_TYPES`).
    pub kind: Option<String>,
}

/// Run a search against the memory store and return the typed record set.
///
/// Records are returned in best-first order. `score` is the FTS5 rank
/// position (1 = best), so ascending score == descending match quality.
/// `--since` is pushed server-side via the `MemoryStore::search_messages`
/// `since: Option<SystemTime>` parameter (introduced in
/// `kernex-memory 0.7.0`), so `LIMIT` applies after the recency filter.
/// `--type` stays client-side because the FTS5 index has no observation
/// type column today; the typed-save schema lifts that later.
#[tracing::instrument(
    name = "kernex.mem.search",
    skip_all,
    fields(
        sender_id = %CLI_SENDER_ID,
        query_len = opts.query.len(),
        limit = opts.limit,
        kind = opts.kind.as_deref().unwrap_or("any"),
        since = opts.since.as_deref().unwrap_or("none"),
        result_count = tracing::field::Empty,
    ),
)]
pub async fn search(
    store: &dyn MemoryStore,
    opts: SearchOpts,
) -> Result<Vec<SearchRecord>, CliError> {
    if let Some(t) = &opts.kind {
        validate_obs_type(t)?;
    }

    let cutoff = match &opts.since {
        Some(s) => Some(parse_since(s)?),
        None => None,
    };

    // Guard against a usize→i64 wrap on 64-bit platforms when the operator
    // passes an absurdly large `--limit`. Clamping silently would mask the
    // intent; an explicit usage error tells them why.
    let limit_i64 = i64::try_from(opts.limit).map_err(|_| CliError::Usage {
        message: format!("--limit value too large: {}", opts.limit),
        hint: "Use a limit that fits in i64 (max 9223372036854775807).".to_string(),
    })?;

    let raw: Vec<MessageRow> = store
        .search_messages(&opts.query, "", CLI_SENDER_ID, limit_i64, cutoff)
        .await
        .map_err(|e| classify_memory_error(e, "memory search"))?;

    let mut records: Vec<SearchRecord> = raw
        .into_iter()
        .enumerate()
        .map(|(idx, row)| {
            let title = preview(&row.content, 80);
            SearchRecord {
                id: row.id,
                kind: row.role,
                title,
                content: row.content,
                updated_at: format_timestamp(row.timestamp),
                score: idx + 1,
            }
        })
        .collect();

    if let Some(t) = &opts.kind {
        // v1: records carry no observation-type column. A known type
        // value is accepted by the parser (S-search-4 stays open until
        // the typed-save schema lands) but currently filters everything
        // out because no row matches. Unknown types already errored
        // above in `validate_obs_type`.
        records.retain(|r| r.kind == *t);
    }

    tracing::Span::current().record("result_count", records.len());
    Ok(records)
}

/// Options accepted by `kx mem history`. Mirror the CLI flags one-to-one.
#[derive(Debug, Clone)]
pub struct HistoryOpts {
    /// Maximum record count to return. Defaults to `DEFAULT_HISTORY_LIMIT`
    /// at the dispatch boundary; the handler does not infer its own
    /// default so the spec value lives in exactly one place.
    pub last: usize,
    /// Resolved project name (per-subcommand `--project` override OR the
    /// global default). The handler echoes it back on every row so the
    /// operator can confirm scope resolution worked. Existence of the
    /// project's data dir is checked upstream in
    /// `mod.rs::resolve_project_data_dir`.
    pub project: String,
}

/// Recent closed conversations for the resolved project, newest first.
///
/// Backed by `MemoryStore::get_history`, which today returns
/// `(summary, updated_at)` tuples for `status = 'closed'` conversations
/// scoped to `(channel, sender_id)`. The richer observation row shape
/// arrives with FU-D-AG-04; this handler upgrades transparently when
/// the trait signature lifts.
#[tracing::instrument(
    name = "kernex.mem.history",
    skip_all,
    fields(
        sender_id = %CLI_SENDER_ID,
        channel = %CLI_CHANNEL,
        project = %opts.project,
        last = opts.last,
        result_count = tracing::field::Empty,
    ),
)]
pub async fn history(
    store: &dyn MemoryStore,
    opts: HistoryOpts,
) -> Result<Vec<HistoryRecord>, CliError> {
    let limit_i64 = i64::try_from(opts.last).map_err(|_| CliError::Usage {
        message: format!("--last value too large: {}", opts.last),
        hint: "Use a count that fits in i64 (max 9223372036854775807).".to_string(),
    })?;

    let raw: Vec<HistoryRow> = store
        .get_history(CLI_CHANNEL, CLI_SENDER_ID, limit_i64)
        .await
        .map_err(|e| CliError::Runtime {
            message: format!("memory history fetch failed: {e}"),
            hint: "Run `kx doctor` to verify the local memory database.".to_string(),
        })?;

    let records: Vec<HistoryRecord> = raw
        .into_iter()
        .enumerate()
        .map(|(idx, row)| HistoryRecord {
            id: row.conversation_id,
            kind: "conversation".to_string(),
            title: preview(&row.summary, 80),
            content: row.summary,
            updated_at: format_timestamp(row.updated_at),
            score: idx + 1,
            project: opts.project.clone(),
        })
        .collect();

    tracing::Span::current().record("result_count", records.len());
    Ok(records)
}

/// Options accepted by `kx mem stats`. Mirror the CLI flags one-to-one.
#[derive(Debug, Clone)]
pub struct StatsOpts {
    /// Resolved project name (per-subcommand `--project` override OR the
    /// global default). Echoed back on the returned record.
    pub project: String,
}

/// Counts plus last-write timestamp for the resolved project.
///
/// Pulls three trait surfaces:
/// - `MemoryStore::get_memory_stats(sender_id)` → `(conversations, messages, facts)`
/// - `MemoryStore::db_size()` → byte count of the SQLite file
/// - `MemoryStore::get_history(channel, sender_id, 1)` → most-recent
///   `updated_at` to derive `last_write_at` (None when no rows; S-stats-2)
///
/// Returns a single record (not a list); CC-5's empty-array contract
/// does not apply here per spec phrasing "empty project returns zero counts".
#[tracing::instrument(
    name = "kernex.mem.stats",
    skip_all,
    fields(
        sender_id = %CLI_SENDER_ID,
        channel = %CLI_CHANNEL,
        project = %opts.project,
    ),
)]
pub async fn stats(store: &dyn MemoryStore, opts: StatsOpts) -> Result<StatsRecord, CliError> {
    let (conversations, _messages, observations, facts) = store
        .get_memory_stats(CLI_SENDER_ID)
        .await
        .map_err(|e| CliError::Runtime {
            message: format!("memory stats fetch failed: {e}"),
            hint: "Run `kx doctor` to verify the local memory database.".to_string(),
        })?;

    let db_size_bytes = store.db_size().await.map_err(|e| CliError::Runtime {
        message: format!("memory db size fetch failed: {e}"),
        hint: "Run `kx doctor` to verify the local memory database.".to_string(),
    })?;

    // Derive last_write_at from the most recent closed-conversation row.
    // When the project is empty, get_history returns an empty Vec and
    // last_write_at is None (S-stats-2).
    let last_write_at = store
        .get_history(CLI_CHANNEL, CLI_SENDER_ID, 1)
        .await
        .map_err(|e| CliError::Runtime {
            message: format!("memory history fetch failed: {e}"),
            hint: "Run `kx doctor` to verify the local memory database.".to_string(),
        })?
        .into_iter()
        .next()
        .map(|row| format_timestamp(row.updated_at));

    Ok(StatsRecord {
        project: opts.project,
        conversations,
        observations,
        facts,
        db_size_bytes,
        last_write_at,
    })
}

/// Fetch a single message by its UUID.
///
/// Backed by `MemoryStore::get_message_by_id`. Returns a `SearchRecord`
/// so the renderer (JSON or table) can reuse the same projection the
/// `search` handler emits. A missing id maps to `CliError::NotFound`
/// (exit code 3) per the CC-7 taxonomy.
#[tracing::instrument(
    name = "kernex.mem.get",
    skip_all,
    fields(sender_id = %CLI_SENDER_ID, id = %id),
)]
pub async fn get(store: &dyn MemoryStore, id: &str) -> Result<SearchRecord, CliError> {
    let row = store
        .get_message_by_id(id)
        .await
        .map_err(|e| CliError::Runtime {
            message: format!("memory get failed: {e}"),
            hint: "Run `kx doctor` to verify the local memory database.".to_string(),
        })?;

    match row {
        Some(row) => Ok(SearchRecord {
            title: preview(&row.content, 80),
            id: row.id,
            kind: row.role,
            content: row.content,
            updated_at: format_timestamp(row.timestamp),
            score: 1,
        }),
        None => Err(CliError::NotFound {
            message: format!("no message with id {id:?}"),
            hint: "Use `kx mem search <query>` to find an id first.".to_string(),
        }),
    }
}

// ---------------------------------------------------------------------------
// Facts CRUD (Step 2.7..2.10)
// ---------------------------------------------------------------------------
//
// The four facts handlers are pure async functions over `MemoryStore`'s
// fact surface (`get_facts`, `get_fact`, `store_fact`, `soft_delete_fact`).
// `--stdin` for `facts add` is read at the dispatcher boundary so the
// handler stays I/O-free.

/// List every active (not soft-deleted) fact for the resolved project.
///
/// Backed by `MemoryStore::get_facts`. Returns the trait's `(key, value)`
/// shape today; `updated_at` lands with FU-D-AG-04. Empty result is
/// `[]` per CC-5; exit code is 0 even for a project with zero facts
/// (mirrors S-stats-2's "empty is still valid" rule).
#[tracing::instrument(
    name = "kernex.mem.facts.list",
    skip_all,
    fields(
        sender_id = %CLI_SENDER_ID,
        result_count = tracing::field::Empty,
    ),
)]
pub async fn facts_list(store: &dyn MemoryStore) -> Result<Vec<FactsRecord>, CliError> {
    let rows = store
        .get_facts(CLI_SENDER_ID)
        .await
        .map_err(|e| CliError::Runtime {
            message: format!("memory facts list failed: {e}"),
            hint: "Run `kx doctor` to verify the local memory database.".to_string(),
        })?;
    let records: Vec<FactsRecord> = rows
        .into_iter()
        .map(|(key, value)| FactsRecord { key, value })
        .collect();
    tracing::Span::current().record("result_count", records.len());
    Ok(records)
}

/// Fetch a single active fact by key (S-facts-get-1).
///
/// Returns `CliError::NotFound` (exit 3) when the key is absent or
/// soft-deleted (CC-9, S-facts-get-2). The hint steers operators to
/// `kx mem facts list` to discover known keys.
#[tracing::instrument(
    name = "kernex.mem.facts.get",
    skip_all,
    fields(sender_id = %CLI_SENDER_ID, key = %key),
)]
pub async fn facts_get(store: &dyn MemoryStore, key: &str) -> Result<FactsRecord, CliError> {
    let value = store
        .get_fact(CLI_SENDER_ID, key)
        .await
        .map_err(|e| CliError::Runtime {
            message: format!("memory facts get failed: {e}"),
            hint: "Run `kx doctor` to verify the local memory database.".to_string(),
        })?;
    match value {
        Some(value) => Ok(FactsRecord {
            key: key.to_string(),
            value,
        }),
        None => Err(CliError::NotFound {
            message: format!("fact '{key}' not found"),
            hint: "Run `kx mem facts list` to see known keys.".to_string(),
        }),
    }
}

/// Upsert a fact value for the given key (S-facts-add-1, S-facts-add-3).
///
/// Empty `value` is rejected as a usage error per S-facts-add-4. The
/// underlying `store_fact` clears `deleted_at` on re-add, so a key that
/// was previously soft-deleted becomes visible again to default reads.
#[tracing::instrument(
    name = "kernex.mem.facts.add",
    skip_all,
    fields(
        sender_id = %CLI_SENDER_ID,
        key = %key,
        value_len = value.len(),
    ),
)]
pub async fn facts_add(
    store: &dyn MemoryStore,
    key: &str,
    value: &str,
) -> Result<FactsRecord, CliError> {
    if value.is_empty() {
        return Err(CliError::Usage {
            message: "fact value cannot be empty".to_string(),
            hint: "Use `kx mem facts delete <key>` to remove a fact.".to_string(),
        });
    }
    store
        .store_fact(CLI_SENDER_ID, key, value)
        .await
        .map_err(|e| CliError::Runtime {
            message: format!("memory facts add failed: {e}"),
            hint: "Run `kx doctor` to verify the local memory database.".to_string(),
        })?;
    Ok(FactsRecord {
        key: key.to_string(),
        value: value.to_string(),
    })
}

/// Soft-delete a fact by key (S-facts-delete-1).
///
/// Returns `CliError::NotFound` when the key is absent or already
/// soft-deleted (S-facts-delete-2, S-facts-delete-3; idempotent absence
/// surfaces as exit 3 per spec). The trait's `soft_delete_fact` returns
/// `true` only on the active→deleted transition.
#[tracing::instrument(
    name = "kernex.mem.facts.delete",
    skip_all,
    fields(sender_id = %CLI_SENDER_ID, key = %key),
)]
pub async fn facts_delete(store: &dyn MemoryStore, key: &str) -> Result<(), CliError> {
    let transitioned = store
        .soft_delete_fact(CLI_SENDER_ID, key)
        .await
        .map_err(|e| CliError::Runtime {
            message: format!("memory facts delete failed: {e}"),
            hint: "Run `kx doctor` to verify the local memory database.".to_string(),
        })?;
    if transitioned {
        Ok(())
    } else {
        Err(CliError::NotFound {
            message: format!("fact '{key}' not found"),
            hint: "Run `kx mem facts list` to see known keys.".to_string(),
        })
    }
}

/// Persist a typed observation (`kx mem save`).
///
/// The dispatcher normalizes operator input (inline flags vs `--stdin`
/// JSON) into a `SaveEntry` before reaching this handler, so the handler
/// only owns the persistence step plus the `SaveRecord` projection. The
/// `sender_id` is fixed to [`CLI_SENDER_ID`]; project scoping comes from
/// the on-disk DB location the dispatcher opens.
///
/// Errors from `MemoryStore::save_observation` (including the DB-layer
/// CHECK constraints for empty title and unknown type) route through
/// [`classify_memory_error`] so SQLite contention surfaces as
/// [`CliError::Transient`] (exit 7) and everything else lands as
/// [`CliError::Runtime`] (exit 5).
#[tracing::instrument(
    name = "kernex.mem.save",
    skip_all,
    fields(
        sender_id = %CLI_SENDER_ID,
        kind = %entry.kind.as_db_str(),
        title_len = entry.title.len(),
    ),
)]
pub async fn save(store: &dyn MemoryStore, entry: SaveEntry) -> Result<SaveRecord, CliError> {
    let saved = store
        .save_observation(entry)
        .await
        .map_err(|e| classify_memory_error(e, "memory save"))?;
    Ok(SaveRecord {
        id: saved.id,
        kind: saved.kind.as_db_str().to_string(),
        title: saved.title,
        what: saved.what,
        why: saved.why,
        where_field: saved.where_field,
        learned: saved.learned,
        created_at: format_timestamp(saved.created_at),
    })
}

/// Resolve an operator-supplied type string into an [`ObservationType`].
/// Returns [`CliError::Usage`] (exit 2) listing the valid set when the
/// input is unknown (S-save-5). Empty input is also a usage error so the
/// operator sees a helpful hint instead of a parse error.
pub fn parse_observation_type(s: &str) -> Result<ObservationType, CliError> {
    if s.is_empty() {
        return Err(CliError::Usage {
            message: "observation type is required".to_string(),
            hint: format!(
                "Pass --type=<kind>. Valid types: {}",
                OBSERVATION_TYPES.join(", ")
            ),
        });
    }
    ObservationType::from_db_str(s).ok_or_else(|| CliError::Usage {
        message: format!("unknown observation type: {s}"),
        hint: format!("Valid types: {}", OBSERVATION_TYPES.join(", ")),
    })
}

fn preview(s: &str, max_chars: usize) -> String {
    let preview: String = s.chars().take(max_chars).collect();
    let truncated = s.chars().count() > max_chars;
    if truncated {
        format!("{preview}...")
    } else {
        preview
    }
}

/// Validate an observation type string against the spec's taxonomy
/// (`OBSERVATION_TYPES`). Returns a usage error (exit 2) listing the
/// valid set when the input is unknown.
fn validate_obs_type(t: &str) -> Result<(), CliError> {
    if OBSERVATION_TYPES.contains(&t) {
        Ok(())
    } else {
        Err(CliError::Usage {
            message: format!("unknown observation type: {t}"),
            hint: format!("Valid types: {}", OBSERVATION_TYPES.join(", ")),
        })
    }
}

/// Parse a recency window string into the cutoff `SystemTime`. Accepts
/// `Nd`, `Nh`, `Nm`, `Nw` (case-insensitive) where N is a positive
/// integer. Returns the resulting `SystemTime` (now minus the duration).
fn parse_since(s: &str) -> Result<SystemTime, CliError> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err(CliError::Usage {
            message: "--since requires a value like 30d, 12h, 90m, 5w".to_string(),
            hint: "Example: kx mem search foo --since 30d".to_string(),
        });
    }
    // `split_at` operates on byte indices and panics if the index falls
    // mid-char. Use `char_indices().next_back()` so a non-ASCII suffix
    // (`30日`, `5м`) is rejected as a usage error instead of crashing.
    let (num_part, unit) = match trimmed.char_indices().next_back() {
        Some((byte_idx, _)) => trimmed.split_at(byte_idx),
        None => unreachable!("trimmed is non-empty per the is_empty check above"),
    };
    let n: u64 = num_part
        .parse()
        .map_err(|e: std::num::ParseIntError| CliError::Usage {
            message: format!("invalid --since value: {trimmed} ({e})"),
            hint: "Use the form Nd, Nh, Nm, or Nw (e.g. 30d).".to_string(),
        })?;
    let secs = match unit.to_ascii_lowercase().as_str() {
        "m" => n.checked_mul(60),
        "h" => n.checked_mul(60 * 60),
        "d" => n.checked_mul(60 * 60 * 24),
        "w" => n.checked_mul(60 * 60 * 24 * 7),
        other => {
            return Err(CliError::Usage {
                message: format!("unknown --since unit: {other}"),
                hint: "Valid units: m (minutes), h (hours), d (days), w (weeks).".to_string(),
            });
        }
    }
    .ok_or_else(|| CliError::Usage {
        message: format!("--since value overflowed: {trimmed}"),
        hint: "Use a smaller window.".to_string(),
    })?;

    SystemTime::now()
        .checked_sub(Duration::from_secs(secs))
        .ok_or_else(|| CliError::Usage {
            message: format!("--since window predates the unix epoch: {trimmed}"),
            hint: "Use a smaller window.".to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use kernex_core::config::MemoryConfig;
    use kernex_core::message::{Request, Response};
    use kernex_memory::{into_handle, Store};
    use tempfile::TempDir;

    /// Build an isolated `Store` rooted under a temp dir and seed it with
    /// the supplied (user_text, assistant_text, project) tuples.
    async fn seeded_store(seeds: &[(&str, &str, &str)]) -> (TempDir, Arc<dyn MemoryStore>) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("memory.db");
        let cfg = MemoryConfig {
            db_path: db_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let store = Store::new(&cfg).await.unwrap();
        for (user_text, asst_text, project) in seeds {
            let req = Request::text(CLI_SENDER_ID, user_text);
            let resp = Response {
                text: (*asst_text).to_string(),
                ..Default::default()
            };
            store
                .store_exchange("cli", &req, &resp, project)
                .await
                .unwrap();
        }
        (tmp, into_handle(store))
    }

    fn opts(query: &str, limit: usize) -> SearchOpts {
        SearchOpts {
            query: query.to_string(),
            limit,
            since: None,
            kind: None,
        }
    }

    /// Like `seeded_store` but closes each conversation after the
    /// exchange so the row surfaces via `get_history` (which only returns
    /// `status = 'closed'` rows). Uses a distinct project per seed to
    /// force one conversation per row; sharing a project would coalesce
    /// the exchanges into a single conversation.
    async fn seeded_store_with_closed_conversations(
        seeds: &[(&str, &str, &str)],
    ) -> (TempDir, Arc<dyn MemoryStore>) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("memory.db");
        let cfg = MemoryConfig {
            db_path: db_path.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let store = Store::new(&cfg).await.unwrap();
        for (user_text, asst_text, project) in seeds {
            let req = Request::text(CLI_SENDER_ID, user_text);
            let resp = Response {
                text: (*asst_text).to_string(),
                ..Default::default()
            };
            store
                .store_exchange(CLI_CHANNEL, &req, &resp, project)
                .await
                .unwrap();
            // Close so `get_history` (which filters `status = 'closed'`)
            // surfaces the row.
            store
                .close_current_conversation(CLI_CHANNEL, CLI_SENDER_ID, project)
                .await
                .unwrap();
        }
        (tmp, into_handle(store))
    }

    fn history_opts(last: usize, project: &str) -> HistoryOpts {
        HistoryOpts {
            last,
            project: project.to_string(),
        }
    }

    #[tokio::test]
    async fn s_search_1_happy_path_returns_records() {
        let (_tmp, store) = seeded_store(&[
            ("Fixed N+1 query in UserList", "ok", "demo"),
            ("unrelated message", "ok", "demo"),
        ])
        .await;
        let out = search(store.as_ref(), opts("N+1", 10)).await.unwrap();
        assert!(!out.is_empty(), "expected at least one match for N+1");
        let first = &out[0];
        assert!(!first.id.is_empty());
        assert_eq!(first.kind, "user");
        assert!(first.content.contains("N+1"));
        assert!(!first.updated_at.is_empty());
        assert_eq!(first.score, 1);
    }

    #[tokio::test]
    async fn s_search_2_limit_caps_result_count() {
        let mut seeds = Vec::new();
        for i in 0..10 {
            seeds.push(("matching message about foo bar baz", "ok", "demo"));
            let _ = i;
        }
        let (_tmp, store) = seeded_store(&seeds).await;
        let out = search(store.as_ref(), opts("foo bar", 3)).await.unwrap();
        assert_eq!(out.len(), 3);
        // Scores increase monotonically (rank position).
        for (i, r) in out.iter().enumerate() {
            assert_eq!(r.score, i + 1);
        }
    }

    #[tokio::test]
    async fn s_search_4_type_filter_user_role_passthrough() {
        // The current data model has no observation type column, so a
        // known-type filter passes the syntax check but matches nothing
        // (no row carries a `bugfix` `kind` until the typed-save schema
        // lands). This locks in the parser surface so S-search-5's
        // negative test holds even before the data model migrates.
        let (_tmp, store) = seeded_store(&[("contains foo", "ok", "demo")]).await;
        let mut o = opts("foo", 10);
        o.kind = Some("bugfix".to_string());
        let out = search(store.as_ref(), o).await.unwrap();
        assert!(
            out.is_empty(),
            "no rows carry observation type yet; expected zero hits"
        );
    }

    #[tokio::test]
    async fn s_search_5_unknown_type_is_usage_error() {
        let (_tmp, store) = seeded_store(&[]).await;
        let mut o = opts("foo", 10);
        o.kind = Some("bogus".to_string());
        let err = search(store.as_ref(), o).await.unwrap_err();
        assert_eq!(err.exit_code(), 2);
        let msg = format!("{err}");
        assert!(msg.contains("unknown observation type"));
    }

    #[tokio::test]
    async fn s_search_6_no_matches_returns_empty() {
        let (_tmp, store) = seeded_store(&[("matches one thing", "ok", "demo")]).await;
        let out = search(store.as_ref(), opts("definitely-not-a-match-zzz", 10))
            .await
            .unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn parse_since_accepts_days_hours_minutes_weeks() {
        let now = SystemTime::now();
        let one_day_ago = parse_since("1d").unwrap();
        assert!(now > one_day_ago);
        // Within 5 seconds of 86400 ago.
        let delta = now.duration_since(one_day_ago).unwrap();
        assert!(delta.as_secs() >= 86_400 - 5 && delta.as_secs() <= 86_400 + 5);

        assert!(parse_since("12h").is_ok());
        assert!(parse_since("90m").is_ok());
        assert!(parse_since("5w").is_ok());
    }

    #[test]
    fn parse_since_rejects_unknown_unit() {
        let err = parse_since("10y").unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn parse_since_rejects_non_numeric() {
        let err = parse_since("abc").unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[tokio::test]
    async fn s_search_3_since_filters_at_trait_surface() {
        // Slice B pushes `since` server-side; the agent's job here is to
        // pass the parsed cutoff straight through to `MemoryStore::search_messages`.
        // Recent corpus + cutoff in the past: result returned. Same
        // corpus + cutoff in the future: result filtered.
        let (_tmp, store) = seeded_store(&[("hello unique-marker world", "ack", "demo")]).await;

        let past = SystemTime::now() - Duration::from_secs(3_600);
        let rows = search(
            store.as_ref(),
            SearchOpts {
                query: "unique-marker".to_string(),
                limit: 10,
                since: None,
                kind: None,
            },
        )
        .await
        .unwrap();
        assert!(!rows.is_empty(), "control: query matches");

        // Push the same query through `parse_since`-equivalent path.
        // We pass an explicit "1h" cutoff (interpreted as "since one hour
        // ago") which must still surface the row.
        let rows_past = search(
            store.as_ref(),
            SearchOpts {
                query: "unique-marker".to_string(),
                limit: 10,
                since: Some("1h".to_string()),
                kind: None,
            },
        )
        .await
        .unwrap();
        assert!(
            !rows_past.is_empty(),
            "since=1h must include just-stored row"
        );

        // A cutoff in the future filters everything out via the
        // server-side WHERE m.timestamp >= ? clause. We construct the
        // future cutoff directly (parse_since only accepts "since N ago"
        // shapes, not future windows) by drilling through the trait
        // method that the handler ultimately invokes.
        let future = SystemTime::now() + Duration::from_secs(3_600);
        let direct = store
            .search_messages("unique-marker", "", CLI_SENDER_ID, 10, Some(future))
            .await
            .unwrap();
        assert!(direct.is_empty(), "since=future must filter out all rows");
        let _ = past; // suppress unused-binding if path above is restructured
    }

    #[test]
    fn parse_since_rejects_non_ascii_unit_without_panic() {
        // Regression test for a `split_at(len - 1)` panic on multi-byte
        // suffixes. `日` (3 bytes UTF-8) means `len() - 1` landed mid-char
        // before the fix. The current code uses `char_indices().next_back()`
        // and surfaces a usage error instead of crashing.
        let err = parse_since("30日").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        // The unit is `日`, which fails the m/h/d/w match arm; the error
        // message should call that out rather than blame the number.
        let msg = format!("{err}");
        assert!(
            msg.contains("unknown --since unit") || msg.contains("invalid --since value"),
            "unexpected error message: {msg}"
        );
    }

    #[tokio::test]
    async fn search_limit_larger_than_result_set_returns_all() {
        // Confirms `--limit 100` against a 2-row store does not error or
        // truncate; it returns the 2 available rows.
        let (_tmp, store) = seeded_store(&[
            ("first matching record", "ok", "demo"),
            ("second matching record", "ok", "demo"),
        ])
        .await;
        let out = search(store.as_ref(), opts("matching", 100)).await.unwrap();
        // FTS5 returns the 2 user messages; the assistant rows contain
        // `ok` which does not match `matching`.
        assert_eq!(out.len(), 2);
    }

    #[tokio::test]
    async fn s_history_1_default_returns_recent_records() {
        // Seed 3 closed conversations and ask for up to 20 (the default).
        // All 3 should come back, newest-first, with the project echo.
        let (_tmp, store) = seeded_store_with_closed_conversations(&[
            ("first message", "ok", "demo-a"),
            ("second message", "ok", "demo-b"),
            ("third message", "ok", "demo-c"),
        ])
        .await;
        let out = history(
            store.as_ref(),
            history_opts(DEFAULT_HISTORY_LIMIT, "demo-default"),
        )
        .await
        .unwrap();
        assert_eq!(out.len(), 3);
        for (i, r) in out.iter().enumerate() {
            assert_eq!(r.score, i + 1);
            assert_eq!(r.kind, "conversation");
            assert_eq!(r.project, "demo-default");
            assert!(!r.id.is_empty());
            assert!(!r.updated_at.is_empty());
        }
    }

    #[tokio::test]
    async fn s_history_2_last_caps_result_count() {
        let mut seeds = Vec::new();
        for i in 0..10 {
            // Unique project per seed so each row is its own conversation.
            let project = format!("proj-{i}");
            // Box::leak gives us a 'static &str without per-seed allocation
            // tracking in the test harness — fine in test code.
            let project: &'static str = Box::leak(project.into_boxed_str());
            seeds.push(("msg", "ok", project));
        }
        let (_tmp, store) = seeded_store_with_closed_conversations(&seeds).await;
        let out = history(store.as_ref(), history_opts(5, "any"))
            .await
            .unwrap();
        assert_eq!(out.len(), 5);
    }

    #[tokio::test]
    async fn history_empty_store_returns_empty() {
        // Covers the CC-5 empty-array contract at the handler boundary.
        // Dispatcher renders `[]` for an empty Vec; handler just returns it.
        let (_tmp, store) = seeded_store_with_closed_conversations(&[]).await;
        let out = history(store.as_ref(), history_opts(20, "demo"))
            .await
            .unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn history_records_carry_resolved_project() {
        // S-history-3 is enforced upstream in
        // `mod.rs::resolve_project_data_dir` (which picks a different DB).
        // The handler's job is to echo the project back on each row so
        // the operator can confirm scope resolution. Verify that here.
        let (_tmp, store) =
            seeded_store_with_closed_conversations(&[("echoed", "ok", "echo-source")]).await;
        let out = history(store.as_ref(), history_opts(20, "echo-override"))
            .await
            .unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].project, "echo-override");
    }

    fn stats_opts(project: &str) -> StatsOpts {
        StatsOpts {
            project: project.to_string(),
        }
    }

    #[tokio::test]
    async fn s_stats_1_returns_counts_and_last_write() {
        // Seed two closed conversations plus one fact. With kernex-memory
        // 0.8.0 the observations column counts the typed observation table
        // independently from messages, so no observations are recorded
        // here. Expect: conversations=2, observations=0, facts=1,
        // last_write_at=Some(...).
        let (_tmp, store) = seeded_store_with_closed_conversations(&[
            ("first message", "first reply", "demo-a"),
            ("second message", "second reply", "demo-b"),
        ])
        .await;
        store
            .store_fact(CLI_SENDER_ID, "auth-pattern", "OIDC + PKCE")
            .await
            .unwrap();

        let record = stats(store.as_ref(), stats_opts("demo-stats"))
            .await
            .unwrap();
        assert_eq!(record.project, "demo-stats");
        assert_eq!(record.conversations, 2);
        assert_eq!(record.observations, 0);
        assert_eq!(record.facts, 1);
        assert!(record.last_write_at.is_some());
        assert!(record.db_size_bytes > 0);
    }

    #[tokio::test]
    async fn s_stats_2_empty_project_zero_counts_null_last_write() {
        // Empty project: no conversations, no facts. last_write_at must
        // be None so the JSON renderer emits `null` (spec S-stats-2).
        let (_tmp, store) = seeded_store_with_closed_conversations(&[]).await;
        let record = stats(store.as_ref(), stats_opts("empty")).await.unwrap();
        assert_eq!(record.project, "empty");
        assert_eq!(record.conversations, 0);
        assert_eq!(record.observations, 0);
        assert_eq!(record.facts, 0);
        assert!(
            record.last_write_at.is_none(),
            "empty project must have null last_write_at"
        );
    }

    // ---- Facts CRUD (Step 2.7..2.10) ----

    #[tokio::test]
    async fn s_facts_list_1_returns_active_facts() {
        let (_tmp, store) = seeded_store(&[]).await;
        store
            .store_fact(CLI_SENDER_ID, "auth-pattern", "OIDC + PKCE")
            .await
            .unwrap();
        store
            .store_fact(CLI_SENDER_ID, "db-driver", "rusqlite")
            .await
            .unwrap();
        let out = facts_list(store.as_ref()).await.unwrap();
        assert_eq!(out.len(), 2);
        // Returned in insertion-ish order; assert by set membership to
        // stay robust against future ordering changes upstream.
        let keys: Vec<&str> = out.iter().map(|r| r.key.as_str()).collect();
        assert!(keys.contains(&"auth-pattern"));
        assert!(keys.contains(&"db-driver"));
    }

    #[tokio::test]
    async fn s_facts_list_2_empty_returns_empty_vec() {
        let (_tmp, store) = seeded_store(&[]).await;
        let out = facts_list(store.as_ref()).await.unwrap();
        assert!(out.is_empty());
    }

    #[tokio::test]
    async fn s_facts_get_1_returns_single_record() {
        let (_tmp, store) = seeded_store(&[]).await;
        store
            .store_fact(CLI_SENDER_ID, "auth-pattern", "OIDC + PKCE")
            .await
            .unwrap();
        let r = facts_get(store.as_ref(), "auth-pattern").await.unwrap();
        assert_eq!(r.key, "auth-pattern");
        assert_eq!(r.value, "OIDC + PKCE");
    }

    #[tokio::test]
    async fn s_facts_get_2_missing_key_is_exit_3() {
        let (_tmp, store) = seeded_store(&[]).await;
        let err = facts_get(store.as_ref(), "bogus").await.unwrap_err();
        assert_eq!(err.exit_code(), 3);
        let msg = format!("{err}");
        assert!(msg.contains("'bogus' not found"));
    }

    #[tokio::test]
    async fn s_facts_add_1_inline_value_writes_new_fact() {
        let (_tmp, store) = seeded_store(&[]).await;
        let r = facts_add(store.as_ref(), "auth-pattern", "OIDC + PKCE")
            .await
            .unwrap();
        assert_eq!(r.key, "auth-pattern");
        assert_eq!(r.value, "OIDC + PKCE");
        // Confirm the round-trip via the get path.
        let g = facts_get(store.as_ref(), "auth-pattern").await.unwrap();
        assert_eq!(g.value, "OIDC + PKCE");
    }

    #[tokio::test]
    async fn s_facts_add_3_existing_key_upserts() {
        let (_tmp, store) = seeded_store(&[]).await;
        facts_add(store.as_ref(), "auth-pattern", "basic")
            .await
            .unwrap();
        let r = facts_add(store.as_ref(), "auth-pattern", "OIDC + PKCE")
            .await
            .unwrap();
        assert_eq!(r.value, "OIDC + PKCE");
        // Underlying store still has exactly one row for this key
        // (upsert, not insert).
        let list = facts_list(store.as_ref()).await.unwrap();
        let matches: Vec<_> = list.iter().filter(|r| r.key == "auth-pattern").collect();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].value, "OIDC + PKCE");
    }

    #[tokio::test]
    async fn s_facts_add_4_empty_value_is_exit_2() {
        let (_tmp, store) = seeded_store(&[]).await;
        let err = facts_add(store.as_ref(), "auth-pattern", "")
            .await
            .unwrap_err();
        assert_eq!(err.exit_code(), 2);
        let msg = format!("{err}");
        assert!(msg.contains("cannot be empty"));
    }

    #[tokio::test]
    async fn s_facts_delete_1_soft_deletes_by_default() {
        let (_tmp, store) = seeded_store(&[]).await;
        facts_add(store.as_ref(), "auth-pattern", "OIDC + PKCE")
            .await
            .unwrap();
        facts_delete(store.as_ref(), "auth-pattern").await.unwrap();
        // Soft-deleted rows are invisible to default reads (CC-9).
        let list = facts_list(store.as_ref()).await.unwrap();
        assert!(list.iter().all(|r| r.key != "auth-pattern"));
        let err = facts_get(store.as_ref(), "auth-pattern").await.unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[tokio::test]
    async fn s_facts_delete_2_missing_key_is_exit_3() {
        let (_tmp, store) = seeded_store(&[]).await;
        let err = facts_delete(store.as_ref(), "bogus").await.unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[tokio::test]
    async fn s_facts_delete_3_already_deleted_is_exit_3() {
        // Idempotent absence per spec: deleting an already-deleted key
        // exits 3, not 0, so scripts get a clear signal.
        let (_tmp, store) = seeded_store(&[]).await;
        facts_add(store.as_ref(), "auth-pattern", "OIDC + PKCE")
            .await
            .unwrap();
        facts_delete(store.as_ref(), "auth-pattern").await.unwrap();
        let err = facts_delete(store.as_ref(), "auth-pattern")
            .await
            .unwrap_err();
        assert_eq!(err.exit_code(), 3);
    }

    #[test]
    fn classify_pool_timeout_is_transient() {
        let err = MemoryError::sqlite("acquire conn", sqlx::Error::PoolTimedOut);
        let cli = classify_memory_error(err, "memory search");
        assert_eq!(cli.exit_code(), 7);
        assert_eq!(cli.kind_name(), "transient");
        let msg = format!("{cli}");
        assert!(
            msg.contains("transient contention"),
            "message should signal transient, got {msg:?}"
        );
    }

    #[test]
    fn classify_pool_closed_is_transient() {
        let err = MemoryError::sqlite("acquire conn", sqlx::Error::PoolClosed);
        let cli = classify_memory_error(err, "memory search");
        assert_eq!(cli.exit_code(), 7);
    }

    #[test]
    fn classify_logic_is_runtime_not_transient() {
        let err = MemoryError::logic("malformed migration row");
        let cli = classify_memory_error(err, "memory search");
        assert_eq!(cli.exit_code(), 5);
        assert_eq!(cli.kind_name(), "runtime");
    }

    #[test]
    fn classify_io_is_runtime_not_transient() {
        let err = MemoryError::io(
            "open audit log",
            std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        );
        let cli = classify_memory_error(err, "memory search");
        assert_eq!(cli.exit_code(), 5);
    }

    #[tokio::test]
    async fn facts_re_add_after_delete_clears_soft_delete() {
        // The trait doc on `store_fact` promises that a previously
        // soft-deleted row becomes visible again on re-add. Lock that
        // in here so a future schema change doesn't silently regress.
        let (_tmp, store) = seeded_store(&[]).await;
        facts_add(store.as_ref(), "auth-pattern", "old")
            .await
            .unwrap();
        facts_delete(store.as_ref(), "auth-pattern").await.unwrap();
        facts_add(store.as_ref(), "auth-pattern", "new")
            .await
            .unwrap();
        let r = facts_get(store.as_ref(), "auth-pattern").await.unwrap();
        assert_eq!(r.value, "new");
    }

    fn full_save_entry() -> SaveEntry {
        let mut e = SaveEntry::new(CLI_SENDER_ID, ObservationType::Bugfix, "Fixed N+1 query");
        e.what = Some("added eager loading in UserList".to_string());
        e.why = Some("lists were 12s slow on 5k users".to_string());
        e.where_field = Some("src/users/list.rs".to_string());
        e.learned =
            Some("FTS5 query rewriter cannot fix N+1; only the ORM call site can".to_string());
        e
    }

    #[tokio::test]
    async fn s_save_1_inline_structured_fields_round_trip() {
        // S-save-1: all seven fields populated, save_observation
        // round-trips through SaveRecord with stable shape.
        let (_tmp, store) = seeded_store(&[]).await;
        let rec = save(store.as_ref(), full_save_entry()).await.unwrap();
        assert_eq!(rec.kind, "bugfix");
        assert_eq!(rec.title, "Fixed N+1 query");
        assert_eq!(rec.what.as_deref(), Some("added eager loading in UserList"));
        assert_eq!(rec.where_field.as_deref(), Some("src/users/list.rs"));
        // ISO 8601 space-separated form (project convention), so a colon
        // (in the time component) is the minimal stable structural
        // assertion that survives daylight-savings / time-zone changes.
        assert!(rec.created_at.contains(':'));
        // UUIDv4 hyphenated form, 36 chars including 4 hyphens.
        assert_eq!(rec.id.len(), 36);
        assert_eq!(rec.id.matches('-').count(), 4);
    }

    #[tokio::test]
    async fn s_save_1_none_optionals_persist_as_null() {
        // Optional fields stay None when the operator omits them; the
        // SaveRecord echoes them back as None so the JSON renderer can
        // emit `null` for stable consumer parsing.
        let (_tmp, store) = seeded_store(&[]).await;
        let entry = SaveEntry::new(CLI_SENDER_ID, ObservationType::Decision, "no extras");
        let rec = save(store.as_ref(), entry).await.unwrap();
        assert_eq!(rec.kind, "decision");
        assert!(rec.what.is_none());
        assert!(rec.why.is_none());
        assert!(rec.where_field.is_none());
        assert!(rec.learned.is_none());
    }

    #[tokio::test]
    async fn s_save_5_unknown_type_is_exit_2() {
        // S-save-5: an unknown type string never reaches the store. The
        // parser refuses it with a usage error (exit 2) that lists the
        // valid set.
        let err = parse_observation_type("bogus").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        let hint = err.hint();
        assert!(hint.contains("bugfix"));
        assert!(hint.contains("architecture"));
    }

    #[tokio::test]
    async fn s_save_5_known_type_resolves() {
        // Every spec type maps cleanly to an ObservationType. Lock the
        // mapping in here so a future enum reshuffle is caught.
        for t in OBSERVATION_TYPES {
            let kind = parse_observation_type(t).unwrap();
            assert_eq!(&kind.as_db_str(), t);
        }
    }

    #[tokio::test]
    async fn s_save_empty_type_is_exit_2() {
        // The parser rejects an empty string explicitly so the operator
        // sees the type-required hint instead of the unknown-type hint.
        let err = parse_observation_type("").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(format!("{err}").contains("required"));
    }
}
