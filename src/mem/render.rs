//! Renderer for `kx mem *` output.
//!
//! Per ADR-004, every `kx mem *` subcommand checks
//! `std::io::IsTerminal::is_terminal` on stdout. When false, output is
//! JSON; ANSI color codes are suppressed; help and error text route to
//! stderr.
//!
//! `--compact` projects to high-gravity fields (`id`, `type`, `title`,
//! `updated_at`, `score`). `--select fld1,fld2` projects arbitrary
//! fields; unknown fields exit `2`.

use std::io::IsTerminal;

use serde_json::{Map, Value};

use crate::mem::errors::CliError;
use crate::mem::types::{HistoryRecord, SearchRecord};

/// Fields that survive `--compact` projection. Defined by spec CC-3.
pub const COMPACT_FIELDS: &[&str] = &["id", "type", "title", "updated_at", "score"];

/// Fields valid on a `SearchRecord` for `--select`. Anything not in this
/// set causes exit 2 with a hint listing the valid names.
pub const SEARCH_FIELDS: &[&str] = &["id", "type", "title", "content", "updated_at", "score"];

/// Fields valid on a `HistoryRecord` for `--select`. Superset of
/// `SEARCH_FIELDS` with `project` echoed back.
pub const HISTORY_FIELDS: &[&str] = &[
    "id",
    "type",
    "title",
    "content",
    "updated_at",
    "score",
    "project",
];

/// True when stdout is a real terminal (CLI is being read by a human),
/// false when piped or redirected (output will be consumed by another
/// program).
pub fn is_tty() -> bool {
    std::io::stdout().is_terminal()
}

/// Resolve the effective output mode given the runtime TTY state and the
/// global `--json` flag. `--json` overrides the TTY check (CC-2).
pub fn json_mode(force_json: bool) -> bool {
    force_json || !is_tty()
}

/// Project a slice of records into a JSON array, honoring `--compact`
/// and `--select`. Returns the JSON string (without trailing newline).
pub fn render_search_json(
    records: &[SearchRecord],
    compact: bool,
    select: &[String],
) -> Result<String, CliError> {
    let validated = validate_select(select, SEARCH_FIELDS)?;

    let arr: Vec<Value> = records
        .iter()
        .map(|r| project(record_to_value(r), compact, &validated))
        .collect();
    // Empty results render as `[]` per CC-5. serde_json never emits null
    // for an empty Vec, so this is enforced by the type system.
    serde_json::to_string(&arr).map_err(|e| CliError::Runtime {
        message: format!("failed to serialize results: {e}"),
        hint: "If this reproduces, please open an issue.".to_string(),
    })
}

/// Render a slice of records as a human-readable list on a TTY.
pub fn render_search_table(records: &[SearchRecord]) -> String {
    if records.is_empty() {
        return "  No results found.\n".to_string();
    }
    let mut out = String::new();
    for r in records {
        let preview: String = r.content.chars().take(120).collect();
        let ellipsis = if r.content.chars().count() > 120 {
            "..."
        } else {
            ""
        };
        out.push_str(&format!("  [{}] {}\n", r.kind, r.title));
        out.push_str(&format!("      {preview}{ellipsis}\n"));
        out.push_str(&format!("      {}\n\n", r.updated_at));
    }
    out
}

/// Project a slice of history records into a JSON array, honoring
/// `--compact` and `--select`. Returns the JSON string (no trailing newline).
pub fn render_history_json(
    records: &[HistoryRecord],
    compact: bool,
    select: &[String],
) -> Result<String, CliError> {
    let validated = validate_select(select, HISTORY_FIELDS)?;
    let arr: Vec<Value> = records
        .iter()
        .map(|r| project(history_record_to_value(r), compact, &validated))
        .collect();
    serde_json::to_string(&arr).map_err(|e| CliError::Runtime {
        message: format!("failed to serialize history: {e}"),
        hint: "If this reproduces, please open an issue.".to_string(),
    })
}

/// Render a slice of history records as a human-readable list on a TTY.
pub fn render_history_table(records: &[HistoryRecord]) -> String {
    if records.is_empty() {
        return "  No conversation history yet.\n".to_string();
    }
    let mut out = String::new();
    for r in records {
        out.push_str(&format!("  [{}] {}\n", r.kind, r.title));
        out.push_str(&format!("      {}\n", r.updated_at));
        out.push_str(&format!("      project: {}\n\n", r.project));
    }
    out
}

fn history_record_to_value(r: &HistoryRecord) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".to_string(), Value::String(r.id.clone()));
    m.insert("type".to_string(), Value::String(r.kind.clone()));
    m.insert("title".to_string(), Value::String(r.title.clone()));
    m.insert("content".to_string(), Value::String(r.content.clone()));
    m.insert(
        "updated_at".to_string(),
        Value::String(r.updated_at.clone()),
    );
    m.insert(
        "score".to_string(),
        Value::Number(serde_json::Number::from(r.score)),
    );
    m.insert("project".to_string(), Value::String(r.project.clone()));
    m
}

fn record_to_value(r: &SearchRecord) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".to_string(), Value::String(r.id.clone()));
    m.insert("type".to_string(), Value::String(r.kind.clone()));
    m.insert("title".to_string(), Value::String(r.title.clone()));
    m.insert("content".to_string(), Value::String(r.content.clone()));
    m.insert(
        "updated_at".to_string(),
        Value::String(r.updated_at.clone()),
    );
    m.insert(
        "score".to_string(),
        Value::Number(serde_json::Number::from(r.score)),
    );
    m
}

fn project(record: Map<String, Value>, compact: bool, select: &[String]) -> Value {
    // `--select` wins over `--compact` when both are set: explicit
    // field projection beats the compact shorthand.
    if !select.is_empty() {
        let keep: Vec<&str> = select.iter().map(String::as_str).collect();
        Value::Object(filter_keys(record, &keep))
    } else if compact {
        Value::Object(filter_keys(record, COMPACT_FIELDS))
    } else {
        Value::Object(record)
    }
}

fn filter_keys(record: Map<String, Value>, keep: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    for k in keep {
        if let Some(v) = record.get(*k) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    out
}

fn validate_select(select: &[String], valid: &[&str]) -> Result<Vec<String>, CliError> {
    for f in select {
        if !valid.contains(&f.as_str()) {
            return Err(CliError::Usage {
                message: format!("unknown --select field: {f}"),
                hint: format!("Valid fields: {}", valid.join(", ")),
            });
        }
    }
    Ok(select.to_vec())
}

/// Render an error per CC-6. Returns the JSON string suitable for stderr
/// when JSON mode is active; returns the plain message for TTY mode.
///
/// Message and hint are sourced via `Display` and [`CliError::hint`] so
/// the two render modes stay structurally identical; only the framing
/// differs.
pub fn render_error(err: &CliError, json_mode: bool) -> String {
    let message = err.to_string();
    let hint = err.hint();
    if json_mode {
        let obj = serde_json::json!({
            "error": {
                "code": err.exit_code(),
                "message": message,
                "hint": hint,
            }
        });
        obj.to_string()
    } else {
        format!("error: {message}\n  Try: {hint}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Vec<SearchRecord> {
        vec![
            SearchRecord {
                id: "id-1".to_string(),
                kind: "user".to_string(),
                title: "first match".to_string(),
                content: "first body".to_string(),
                updated_at: "2026-05-01T00:00:00Z".to_string(),
                score: 1,
            },
            SearchRecord {
                id: "id-2".to_string(),
                kind: "assistant".to_string(),
                title: "second match".to_string(),
                content: "second body".to_string(),
                updated_at: "2026-05-02T00:00:00Z".to_string(),
                score: 2,
            },
        ]
    }

    #[test]
    fn json_default_includes_all_fields() {
        let out = render_search_json(&sample(), false, &[]).unwrap();
        assert!(out.contains("\"id\":\"id-1\""));
        assert!(out.contains("\"content\":\"first body\""));
        assert!(out.contains("\"score\":1"));
    }

    #[test]
    fn json_compact_strips_content() {
        let out = render_search_json(&sample(), true, &[]).unwrap();
        assert!(out.contains("\"id\":\"id-1\""));
        assert!(!out.contains("\"content\""));
        // compact keeps id/type/title/updated_at/score
        assert!(out.contains("\"title\":\"first match\""));
        assert!(out.contains("\"score\":1"));
    }

    #[test]
    fn json_select_projects_named_fields() {
        let out = render_search_json(&sample(), false, &["id".into(), "title".into()]).unwrap();
        assert!(out.contains("\"id\":\"id-1\""));
        assert!(out.contains("\"title\":\"first match\""));
        assert!(!out.contains("\"score\""));
        assert!(!out.contains("\"content\""));
    }

    #[test]
    fn json_select_unknown_field_is_usage_error() {
        let err = render_search_json(&sample(), false, &["bogus".into()]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        let msg = format!("{err}");
        assert!(msg.contains("unknown --select field"));
    }

    #[test]
    fn empty_results_render_as_empty_array() {
        let out = render_search_json(&[], false, &[]).unwrap();
        assert_eq!(out, "[]");
    }

    #[test]
    fn select_wins_over_compact_when_both_set() {
        // Spec doesn't pick a precedence explicitly, but the implementation
        // (and the comment in `project`) commits to: --select beats --compact.
        // Verify by asking for a field --compact would strip (`content`) and
        // confirming it survives.
        let out = render_search_json(
            &sample(),
            true,                             // compact=true would strip `content`
            &["id".into(), "content".into()], // but --select keeps it
        )
        .unwrap();
        assert!(out.contains("\"id\":\"id-1\""));
        assert!(out.contains("\"content\":\"first body\""));
        // --compact's other fields are NOT present because --select takes over.
        assert!(!out.contains("\"score\""));
        assert!(!out.contains("\"title\""));
        assert!(!out.contains("\"updated_at\""));
    }

    #[test]
    fn table_handles_empty_results() {
        let out = render_search_table(&[]);
        assert!(out.contains("No results"));
    }

    #[test]
    fn error_json_shape_includes_code_message_hint() {
        let err = CliError::NotFound {
            message: "id 9999 not found".to_string(),
            hint: "run kx mem search to list ids".to_string(),
        };
        let out = render_error(&err, true);
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["error"]["code"], 3);
        assert_eq!(v["error"]["message"], "id 9999 not found");
        assert!(v["error"]["hint"].as_str().unwrap().contains("kx mem"));
    }
}
