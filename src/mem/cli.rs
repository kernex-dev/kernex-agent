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

use kernex_memory::MemoryStore;

use crate::mem::errors::CliError;
use crate::mem::types::{FactsRecord, HistoryRecord, SearchRecord, StatsRecord, OBSERVATION_TYPES};

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
/// `--since` and `--type` are applied client-side after the FTS5 fetch.
/// Known v1 limitation: the limit caps the pre-filter fetch, so a query
/// combined with `--since` may return fewer than `limit` rows even when
/// more would qualify; pushing the filter into `MemoryStore::search_messages`
/// is tracked as a follow-up.
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

    let raw = store
        .search_messages(&opts.query, "", CLI_SENDER_ID, limit_i64)
        .await
        .map_err(|e| CliError::Runtime {
            message: format!("memory search failed: {e}"),
            hint: "Run `kx doctor` to verify the local memory database.".to_string(),
        })?;

    let mut records: Vec<SearchRecord> = raw
        .into_iter()
        .enumerate()
        .map(|(idx, (role, content, updated_at))| {
            let title = preview(&content, 80);
            SearchRecord {
                // The trait surface does not return a stable id today;
                // synthesize one from rank+timestamp so the CLI emits
                // something the user can copy. When the typed-save
                // schema lands this becomes the observation row id.
                id: format!("msg-{}-{}", idx + 1, updated_at),
                kind: role,
                title,
                content,
                updated_at,
                score: idx + 1,
            }
        })
        .collect();

    if let Some(cutoff) = cutoff {
        records.retain(|r| timestamp_after(&r.updated_at, cutoff));
    }
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

    let raw = store
        .get_history(CLI_CHANNEL, CLI_SENDER_ID, limit_i64)
        .await
        .map_err(|e| CliError::Runtime {
            message: format!("memory history fetch failed: {e}"),
            hint: "Run `kx doctor` to verify the local memory database.".to_string(),
        })?;

    let records: Vec<HistoryRecord> = raw
        .into_iter()
        .enumerate()
        .map(|(idx, (summary, updated_at))| HistoryRecord {
            id: format!("conv-{}-{}", idx + 1, updated_at),
            kind: "conversation".to_string(),
            title: preview(&summary, 80),
            content: summary,
            updated_at,
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
    let (conversations, observations, facts) = store
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
        .map(|(_summary, updated_at)| updated_at);

    Ok(StatsRecord {
        project: opts.project,
        conversations,
        observations,
        facts,
        db_size_bytes,
        last_write_at,
    })
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

/// Compare a record timestamp string against a cutoff. Permissive: an
/// unparseable timestamp is treated as "older than the cutoff" so the
/// `--since` filter never silently surfaces a malformed row. Today the
/// memory store writes timestamps via SQLite `CURRENT_TIMESTAMP`, which
/// emits `YYYY-MM-DD HH:MM:SS` in UTC.
fn timestamp_after(ts: &str, cutoff: SystemTime) -> bool {
    let Some(parsed) = parse_sqlite_utc(ts) else {
        return false;
    };
    parsed >= cutoff
}

/// Parse SQLite's `CURRENT_TIMESTAMP` shape (`YYYY-MM-DD HH:MM:SS` in
/// UTC) and the ISO-8601 variant (`YYYY-MM-DDTHH:MM:SSZ`) into a
/// `SystemTime`. Returns `None` on any parse failure; callers treat that
/// as "skip this row" rather than panicking.
fn parse_sqlite_utc(ts: &str) -> Option<SystemTime> {
    // Hand-rolled to avoid pulling chrono into kernex-agent's direct
    // deps. The two shapes we accept are well-defined fixed-width.
    let bytes = ts.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let year: i64 = ts.get(0..4)?.parse().ok()?;
    let month: u32 = ts.get(5..7)?.parse().ok()?;
    let day: u32 = ts.get(8..10)?.parse().ok()?;
    let sep = bytes[10];
    if sep != b' ' && sep != b'T' {
        return None;
    }
    let hour: u32 = ts.get(11..13)?.parse().ok()?;
    let minute: u32 = ts.get(14..16)?.parse().ok()?;
    let second: u32 = ts.get(17..19)?.parse().ok()?;

    let days_since_epoch = days_from_civil(year, month, day)?;
    let secs = (days_since_epoch as i64)
        .checked_mul(86_400)?
        .checked_add(hour as i64 * 3_600)?
        .checked_add(minute as i64 * 60)?
        .checked_add(second as i64)?;
    if secs < 0 {
        return None;
    }
    SystemTime::UNIX_EPOCH.checked_add(Duration::from_secs(secs as u64))
}

/// Convert a proleptic Gregorian (year, month, day) to days since the
/// unix epoch (1970-01-01). Adapted from Howard Hinnant's `days_from_civil`
/// algorithm. Returns `None` for inputs that fall outside the supported
/// range (year < 0 or month/day out of range).
fn days_from_civil(year: i64, month: u32, day: u32) -> Option<i64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = month as i64;
    let d = day as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
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

    #[test]
    fn parse_sqlite_utc_round_trips_within_a_second() {
        // SQLite shape.
        let parsed = parse_sqlite_utc("2026-05-11 12:34:56").unwrap();
        let epoch = parsed
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        // Hand-computed: 2026-05-11T12:34:56Z = 1778502896 seconds.
        assert_eq!(epoch, 1_778_502_896);

        // ISO-8601 shape with the `T` separator.
        let parsed_t = parse_sqlite_utc("2026-05-11T12:34:56Z").unwrap();
        assert_eq!(parsed, parsed_t);
    }

    #[test]
    fn parse_sqlite_utc_rejects_garbage() {
        assert!(parse_sqlite_utc("").is_none());
        assert!(parse_sqlite_utc("not-a-timestamp").is_none());
        assert!(parse_sqlite_utc("2026-99-01 00:00:00").is_none());
    }

    #[tokio::test]
    async fn s_search_3_since_filters_old_records() {
        // We cannot backdate SQLite's CURRENT_TIMESTAMP from the public
        // trait surface; instead, exercise the filter at the helper
        // boundary. A 30-day window must include "now" and exclude a
        // synthetic timestamp from 60 days ago.
        let now = SystemTime::now();
        let cutoff = now.checked_sub(Duration::from_secs(30 * 86_400)).unwrap();

        let recent = "2099-01-01 00:00:00";
        let ancient = "2000-01-01 00:00:00";

        assert!(timestamp_after(recent, cutoff));
        assert!(!timestamp_after(ancient, cutoff));
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
        // Seed two closed conversations (each writes one user + one
        // assistant message) plus one fact. Expect: conversations=2,
        // observations=4, facts=1, last_write_at=Some(...).
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
        assert_eq!(record.observations, 4);
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
}
