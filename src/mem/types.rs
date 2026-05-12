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
/// `conversations` is an extra field surfaced for v1 since the trait
/// already exposes it; it is in the `--select` allowlist but not in
/// `--compact`.
///
/// `last_write_at` is `None` for an empty project (S-stats-2). Today it
/// is derived from the most recent closed-conversation row via
/// `get_history(.., .., 1)`; a typed `max(updated_at)` query on the
/// observations table is a future tightening.
#[derive(Debug, Clone, Serialize)]
pub struct StatsRecord {
    /// Resolved project name (echoed for operator confirmation).
    pub project: String,
    /// Closed + active conversation rows for this sender.
    pub conversations: i64,
    /// Active observation rows for this sender (from the typed observation
    /// table, as of `kernex-memory 0.8.0`). Soft-deleted observations are
    /// excluded.
    pub observations: i64,
    /// Active (not soft-deleted) fact rows for this sender.
    pub facts: i64,
    /// On-disk byte size of the SQLite database file (memory.db).
    pub db_size_bytes: u64,
    /// ISO-8601 timestamp of the most recent closed-conversation update,
    /// or `None` when no rows exist (S-stats-2).
    pub last_write_at: Option<String>,
}

/// One row of `kx mem facts list` / `kx mem facts get` output, plus the
/// single-record response from `kx mem facts add`.
///
/// `MemoryStore::get_facts` returns `(key, value)` tuples today; the
/// schema has an `updated_at` column but the trait does not surface it.
/// FU-D-AG-04 (the typed-row data-model bump) adds `updated_at` to the
/// trait response; this struct gains the field at that point without
/// changing the JSON key names.
#[derive(Debug, Clone, Serialize)]
pub struct FactsRecord {
    /// The fact key (unique per `(sender_id, key)`).
    pub key: String,
    /// The fact value.
    pub value: String,
}

/// Valid observation types per the kx-mem-cli-promotion proposal.
///
/// `--type bogus` exits 2 and stderr lists this set (S-search-5,
/// S-save-5). The seven strings match `kernex_memory::ObservationType`'s
/// `snake_case` / `lowercase` serialization exactly.
pub const OBSERVATION_TYPES: &[&str] = &[
    "bugfix",
    "decision",
    "pattern",
    "config",
    "discovery",
    "learning",
    "architecture",
];

/// One row of `kx mem save` output: the persisted observation echoed
/// back to the operator. The seven structured fields mirror
/// `kernex_memory::Observation`; `id` is the freshly-assigned UUIDv4 and
/// `created_at` is the ISO-8601 timestamp at write time.
///
/// Optional fields (`what`, `why`, `where`, `learned`) render as JSON
/// `null` when absent so consumers can rely on a stable key set.
#[derive(Debug, Clone, Serialize)]
pub struct SaveRecord {
    /// Stable UUIDv4 assigned by `MemoryStore::save_observation`.
    pub id: String,
    /// Observation type, one of [`OBSERVATION_TYPES`].
    #[serde(rename = "type")]
    pub kind: String,
    /// Operator-provided title (non-empty).
    pub title: String,
    /// What changed (optional).
    pub what: Option<String>,
    /// Why it changed (optional).
    pub why: Option<String>,
    /// Where the change applied (optional file path or scope).
    #[serde(rename = "where")]
    pub where_field: Option<String>,
    /// What was learned (optional).
    pub learned: Option<String>,
    /// ISO-8601 timestamp at write time.
    pub created_at: String,
}
