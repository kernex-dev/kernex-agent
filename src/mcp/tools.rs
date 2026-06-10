//! Tool surface for `kx mcp`: the local memory store exposed as MCP tools.
//!
//! v1 surfaces the read-mostly memory operations plus `mem_save`, each a thin
//! adapter over the existing `mem::cli` handlers (the same code path `kx mem`
//! uses), so an MCP host (Claude Code) recalls and records against the same
//! per-project store the CLI does. `run`/`skills` tools are intentionally
//! deferred until the runtime agentic loop is hardened.

use kernex_memory::{MemoryStore, SaveEntry};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::mem::cli::{
    self, HistoryOpts, SearchOpts, StatsOpts, CLI_SENDER_ID, DEFAULT_HISTORY_LIMIT,
};

/// Outcome of a `tools/call`. `is_error` maps to the MCP result's `isError`
/// flag: a handler failure is reported in-band (isError + message) rather
/// than as a JSON-RPC protocol error, per the MCP convention.
pub struct ToolOutcome {
    pub value: Value,
    pub is_error: bool,
}

/// JSON-RPC error returned for a malformed call (unknown tool, bad argument
/// shape) — distinct from a tool-level failure, which is an `is_error`
/// outcome. Carries a JSON-RPC error code and message.
pub struct CallError {
    pub code: i64,
    pub message: String,
}

/// The `tools/list` payload: one entry per exposed tool. Kept as data so the
/// list is trivially testable and stays in lockstep with [`call`].
pub fn definitions() -> Value {
    // Built as a Value (not a raw slice) so it interpolates cleanly into the
    // json! schema below; mirrors crate::mem::types::OBSERVATION_TYPES.
    let obs_enum = Value::from(crate::mem::types::OBSERVATION_TYPES.to_vec());
    json!([
        {
            "name": "mem_search",
            "description": "Full-text search the project's memory (messages and observations). Returns best-first matches.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string", "description": "FTS5 query string"},
                    "limit": {"type": "integer", "description": "Max results (default 20)", "minimum": 1},
                    "since": {"type": "string", "description": "Recency window, e.g. 7d, 12h, 30m, 2w"},
                    "type": {"type": "string", "description": "Filter by observation type", "enum": obs_enum.clone()}
                },
                "required": ["query"]
            }
        },
        {
            "name": "mem_get",
            "description": "Fetch a single memory record by its id.",
            "inputSchema": {
                "type": "object",
                "properties": {"id": {"type": "string"}},
                "required": ["id"]
            }
        },
        {
            "name": "mem_history",
            "description": "Recent closed conversations for the project, newest first.",
            "inputSchema": {
                "type": "object",
                "properties": {"last": {"type": "integer", "description": "Max records (default 20)", "minimum": 1}}
            }
        },
        {
            "name": "mem_stats",
            "description": "Counts (conversations, observations, facts), db size, and last write time for the project.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "mem_facts_list",
            "description": "List all stored key/value facts for the project.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "mem_facts_get",
            "description": "Get a single fact by key.",
            "inputSchema": {
                "type": "object",
                "properties": {"key": {"type": "string"}},
                "required": ["key"]
            }
        },
        {
            "name": "mem_save",
            "description": "Record a structured observation (bugfix, decision, pattern, config, discovery, learning, architecture).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "type": {"type": "string", "enum": obs_enum.clone()},
                    "title": {"type": "string"},
                    "what": {"type": "string"},
                    "why": {"type": "string"},
                    "where": {"type": "string"},
                    "learned": {"type": "string"}
                },
                "required": ["type", "title"]
            }
        }
    ])
}

#[derive(Deserialize)]
struct SearchArgs {
    query: String,
    limit: Option<usize>,
    since: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
struct IdArg {
    id: String,
}

#[derive(Deserialize)]
struct HistoryArgs {
    last: Option<usize>,
}

#[derive(Deserialize)]
struct KeyArg {
    key: String,
}

#[derive(Deserialize)]
struct SaveArgs {
    #[serde(rename = "type")]
    kind: String,
    title: String,
    what: Option<String>,
    why: Option<String>,
    #[serde(rename = "where")]
    where_field: Option<String>,
    learned: Option<String>,
}

fn parse_args<T: for<'de> Deserialize<'de>>(arguments: &Value) -> Result<T, CallError> {
    serde_json::from_value(arguments.clone()).map_err(|e| CallError {
        code: crate::mcp::protocol::INVALID_PARAMS,
        message: format!("invalid arguments: {e}"),
    })
}

/// Dispatch a `tools/call` to the matching memory handler. `project` is the
/// resolved project name (echoed back on the records that carry it). Returns
/// `Err(CallError)` only for a malformed call; a handler failure is an
/// `Ok(ToolOutcome { is_error: true, .. })`.
pub async fn call(
    store: &dyn MemoryStore,
    project: &str,
    name: &str,
    arguments: &Value,
) -> Result<ToolOutcome, CallError> {
    match name {
        "mem_search" => {
            let a: SearchArgs = parse_args(arguments)?;
            let opts = SearchOpts {
                query: a.query,
                limit: a.limit.unwrap_or(20),
                since: a.since,
                kind: a.kind,
            };
            Ok(outcome(cli::search(store, opts).await))
        }
        "mem_get" => {
            let a: IdArg = parse_args(arguments)?;
            Ok(outcome(cli::get(store, &a.id).await))
        }
        "mem_history" => {
            let a: HistoryArgs = parse_args(arguments)?;
            let opts = HistoryOpts {
                last: a.last.unwrap_or(DEFAULT_HISTORY_LIMIT),
                project: project.to_string(),
            };
            Ok(outcome(cli::history(store, opts).await))
        }
        "mem_stats" => {
            let opts = StatsOpts {
                project: project.to_string(),
            };
            Ok(outcome(cli::stats(store, opts).await))
        }
        "mem_facts_list" => Ok(outcome(cli::facts_list(store).await)),
        "mem_facts_get" => {
            let a: KeyArg = parse_args(arguments)?;
            Ok(outcome(cli::facts_get(store, &a.key).await))
        }
        "mem_save" => {
            let a: SaveArgs = parse_args(arguments)?;
            // Build the SaveEntry exactly as the inline `kx mem save` path does
            // (sender_id = the shared CLI identity, so MCP saves are visible to
            // the CLI and REPL), surfacing a bad type as an is_error outcome.
            let kind = match cli::parse_observation_type(&a.kind) {
                Ok(k) => k,
                Err(e) => return Ok(error_outcome(&e.to_string())),
            };
            let mut entry = SaveEntry::new(CLI_SENDER_ID, kind, a.title);
            entry.what = a.what;
            entry.why = a.why;
            entry.where_field = a.where_field;
            entry.learned = a.learned;
            Ok(outcome(cli::save(store, entry).await))
        }
        other => Err(CallError {
            code: crate::mcp::protocol::INVALID_PARAMS,
            message: format!("unknown tool '{other}'"),
        }),
    }
}

/// Wrap a handler `Result<impl Serialize, CliError>` into a `ToolOutcome`:
/// success serializes the record(s); a `CliError` becomes an `is_error`
/// outcome carrying the error's display text.
fn outcome<T: serde::Serialize>(res: Result<T, crate::mem::errors::CliError>) -> ToolOutcome {
    match res {
        Ok(value) => ToolOutcome {
            value: serde_json::to_value(value).unwrap_or(Value::Null),
            is_error: false,
        },
        Err(e) => error_outcome(&e.to_string()),
    }
}

fn error_outcome(message: &str) -> ToolOutcome {
    ToolOutcome {
        value: json!({ "error": message }),
        is_error: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definitions_list_all_seven_tools_with_schemas() {
        let defs = definitions();
        let arr = defs.as_array().unwrap();
        assert_eq!(arr.len(), 7);
        let names: Vec<&str> = arr.iter().map(|t| t["name"].as_str().unwrap()).collect();
        for expected in [
            "mem_search",
            "mem_get",
            "mem_history",
            "mem_stats",
            "mem_facts_list",
            "mem_facts_get",
            "mem_save",
        ] {
            assert!(names.contains(&expected), "missing tool {expected}");
        }
        // Every tool advertises an object inputSchema (MCP requires it).
        for t in arr {
            assert_eq!(t["inputSchema"]["type"], "object", "tool {t:?} schema");
        }
    }

    #[test]
    fn save_schema_enum_matches_observation_types() {
        let defs = definitions();
        let save = defs
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == "mem_save")
            .unwrap();
        let enum_vals: Vec<&str> = save["inputSchema"]["properties"]["type"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(enum_vals, crate::mem::types::OBSERVATION_TYPES);
    }
}
