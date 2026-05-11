//! CLI-side record shapes for `kx mem *` subcommands.
//!
//! Each subcommand returns a typed record that the renderer projects to
//! JSON (auto-JSON when stdout is not a TTY, or `--json` forced) or to a
//! human-friendly table. Splitting handler output from rendering lets
//! `tests/mem_repl_parity.rs` assert byte-equivalence on the underlying
//! record set without coupling to the rendered form.
//!
//! The shapes here track what `kernex-memory 0.6.1` exposes today. The
//! richer observation model (with explicit `type`, `title`, and
//! BM25 `score`) is reserved on `SearchRecord`; until the upstream
//! `SaveEntry` schema lands, `type` carries the message `role`, `title`
//! carries a content preview, and `score` carries the rank position.

use serde::Serialize;

/// One row of `kx mem search` output.
#[derive(Debug, Clone, Serialize)]
pub struct SearchRecord {
    /// Stable identifier for the matched record. Today this is the message
    /// row's UUID. When `SaveEntry` lands, the same field will carry the
    /// observation id.
    pub id: String,
    /// Observation type. Today carries the message role (`user`,
    /// `assistant`) until the typed-save schema migrates.
    #[serde(rename = "type")]
    pub kind: String,
    /// Display title. Today derived from the first 80 chars of `content`;
    /// when `SaveEntry` lands, this becomes the operator-provided title.
    pub title: String,
    /// Full body text returned by the FTS5 match.
    pub content: String,
    /// ISO-8601 timestamp the message was stored. Reused as `updated_at`
    /// when the spec demands that name (e.g., `--compact` projection).
    pub updated_at: String,
    /// Rank position in the result set (1-based). FTS5 BM25 scores are
    /// not exposed by the underlying trait surface today, so we expose
    /// the deterministic position instead. Lower is a better match.
    pub score: usize,
}

/// One row of `kx mem history` output.
///
/// Backed today by `MemoryStore::get_history`, which returns closed
/// conversation summaries newest-first. When the typed observation
/// schema lands (FU-D-AG-04), this struct picks up the full observation
/// row shape (`id`, `type`, `title`, save-body fields); the `summary`
/// field becomes `title` and a new `id` field replaces the synthetic
/// `msg-<rank>-<ts>` placeholder.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryRecord {
    /// Stable identifier. Today: synthesized from rank + timestamp.
    pub id: String,
    /// Observation type. Today: hardcoded to `"conversation"` because
    /// the underlying row is a conversation summary, not a typed
    /// observation. Becomes the operator-supplied type post-FU-D-AG-04.
    #[serde(rename = "type")]
    pub kind: String,
    /// Display title. Today: the conversation summary text (truncated
    /// to 80 chars). Becomes the operator-provided title post-FU-D-AG-04.
    pub title: String,
    /// Full body text. Today: the full conversation summary.
    pub content: String,
    /// ISO-8601 timestamp the row was last updated. SQLite emits the
    /// `YYYY-MM-DD HH:MM:SS` shape; kept as a string here so downstream
    /// renderers can pass it through verbatim.
    pub updated_at: String,
    /// Rank position in the result set (1 = newest). Maps to the
    /// `--compact` projection's `score` field; lower = more recent.
    pub score: usize,
    /// Project the record belongs to. Echoed back so the operator can
    /// confirm `--project` resolution worked as intended.
    pub project: String,
}

/// `kx mem stats` output. Single object (not a list).
///
/// `observations` is the spec field name (per S-stats-1). Today it maps
/// to the underlying message count because the typed-observation table
/// does not exist yet (tracked under FU-D-AG-04). `conversations` is an
/// extra field surfaced for v1 since the trait already exposes it; it
/// is in the `--select` allowlist but not in `--compact`.
///
/// `last_write_at` is `None` for an empty project (S-stats-2). Today it
/// is derived from the most recent closed-conversation row via
/// `get_history(.., .., 1)`; when FU-D-AG-04 lands a typed `max(updated_at)`
/// query on the observations table replaces the derivation.
#[derive(Debug, Clone, Serialize)]
pub struct StatsRecord {
    /// Resolved project name (echoed for operator confirmation).
    pub project: String,
    /// Closed + active conversation rows for this sender.
    pub conversations: i64,
    /// Message rows for this sender. The spec calls this "observations"
    /// since the long-term model is the typed observation table; until
    /// FU-D-AG-04 lands, the underlying row source is messages.
    #[serde(rename = "observations")]
    pub observations: i64,
    /// Active (not soft-deleted) fact rows for this sender.
    pub facts: i64,
    /// On-disk byte size of the SQLite database file (memory.db).
    pub db_size_bytes: u64,
    /// ISO-8601 timestamp of the most recent closed-conversation update,
    /// or `None` when no rows exist (S-stats-2).
    pub last_write_at: Option<String>,
}

/// Valid observation types per the kx-mem-cli-promotion proposal.
///
/// `--type bogus` exits 2 and stderr lists this set (S-search-5,
/// S-save-5). Until the typed-save schema lands, supplying a known type
/// from this list returns zero results (records have no type column to
/// match against yet); supplying an unknown type still errors out.
pub const OBSERVATION_TYPES: &[&str] = &[
    "bugfix",
    "decision",
    "pattern",
    "config",
    "discovery",
    "learning",
    "architecture",
];
