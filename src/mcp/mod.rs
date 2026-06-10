//! `kx mcp` — a minimal stdio MCP server exposing the local memory store.
//!
//! Claude Code (or any MCP host) spawns `kx mcp` in a project directory and
//! speaks newline-delimited JSON-RPC 2.0 over stdio. This server resolves the
//! project from the cwd the same way `kx mem` / `kx dev` do, opens that
//! project's memory store once, and serves the memory tool surface (see
//! [`tools`]). It is hand-rolled (mirroring the workspace's minimal MCP
//! client) rather than pulling a heavy SDK, keeping the `mcp` feature small.
//!
//! Methods handled: `initialize`, `notifications/initialized`, `ping`,
//! `tools/list`, `tools/call`. Everything else returns method-not-found.

#![cfg(feature = "mcp")]

mod protocol;
mod tools;

use kernex_memory::MemoryStore;
use serde_json::{json, Value};

use protocol::{
    Incoming, Outgoing, INVALID_PARAMS, MCP_PROTOCOL_VERSION, METHOD_NOT_FOUND, PARSE_ERROR,
};

/// Entry point for the `kx mcp` subcommand. Resolves the project from the
/// cwd, opens its memory store, and runs the stdio JSON-RPC loop until EOF.
#[tracing::instrument(name = "kernex.mcp.serve", skip_all, err)]
pub async fn serve() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let project = crate::stack::project_name(&cwd);
    let data_dir = crate::data_dir_for(&project);
    let store = crate::mem::open_store(&data_dir)
        .await
        .map_err(anyhow::Error::from)?;
    tracing::info!(project = %project, "kx mcp serving memory tools over stdio");
    run_loop(store.as_ref(), &project).await
}

async fn run_loop(store: &dyn MemoryStore, project: &str) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut stdout = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(resp) = handle_line(&line, store, project).await {
            stdout.write_all(resp.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}

/// Parse one JSON-RPC line and produce the response line, or `None` for a
/// notification (which must not be answered) or a blank parse of one.
async fn handle_line(line: &str, store: &dyn MemoryStore, project: &str) -> Option<String> {
    let incoming: Incoming = match serde_json::from_str(line) {
        Ok(inc) => inc,
        Err(e) => {
            // Per JSON-RPC, a parse error is answered with a null id.
            return Some(serialize(&Outgoing::err(
                Value::Null,
                PARSE_ERROR,
                format!("parse error: {e}"),
            )));
        }
    };

    if incoming.is_notification() {
        return None;
    }
    let id = incoming.id.clone().unwrap_or(Value::Null);
    Some(serialize(&dispatch(incoming, id, store, project).await))
}

async fn dispatch(
    incoming: Incoming,
    id: Value,
    store: &dyn MemoryStore,
    project: &str,
) -> Outgoing {
    match incoming.method.as_str() {
        "initialize" => Outgoing::ok(id, initialize_result(incoming.params.as_ref())),
        "ping" => Outgoing::ok(id, json!({})),
        "tools/list" => Outgoing::ok(id, json!({ "tools": tools::definitions() })),
        "tools/call" => {
            let params = incoming.params.unwrap_or(Value::Null);
            let name = match params.get("name").and_then(|v| v.as_str()) {
                Some(n) => n,
                None => return Outgoing::err(id, INVALID_PARAMS, "missing tool name"),
            };
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match tools::call(store, project, name, &arguments).await {
                Ok(out) => Outgoing::ok(
                    id,
                    json!({
                        "content": [{ "type": "text", "text": serialize_value(&out.value) }],
                        "isError": out.is_error,
                    }),
                ),
                Err(ce) => Outgoing::err(id, ce.code, ce.message),
            }
        }
        other => Outgoing::err(id, METHOD_NOT_FOUND, format!("method not found: {other}")),
    }
}

fn initialize_result(params: Option<&Value>) -> Value {
    // Echo the host's requested protocolVersion so the two ends agree on
    // whatever the host speaks; fall back to our default if absent.
    let version = params
        .and_then(|p| p.get("protocolVersion"))
        .and_then(|v| v.as_str())
        .unwrap_or(MCP_PROTOCOL_VERSION);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "kernex", "version": env!("CARGO_PKG_VERSION") },
    })
}

fn serialize(out: &Outgoing) -> String {
    serde_json::to_string(out).unwrap_or_else(|_| {
        r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"response serialization failed"}}"#
            .to_string()
    })
}

fn serialize_value(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn open_test_store(dir: &std::path::Path) -> std::sync::Arc<dyn MemoryStore> {
        crate::mem::open_store(dir).await.unwrap()
    }

    #[tokio::test]
    async fn full_handshake_list_and_call_stats() {
        let tmp = TempDir::new().unwrap();
        let store = open_test_store(tmp.path()).await;

        // initialize echoes the host protocolVersion and our serverInfo.
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
            store.as_ref(),
            "proj",
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(v["result"]["serverInfo"]["name"], "kernex");

        // notifications/initialized gets no reply.
        assert!(handle_line(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            store.as_ref(),
            "proj",
        )
        .await
        .is_none());

        // tools/list returns the seven tools.
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            store.as_ref(),
            "proj",
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["tools"].as_array().unwrap().len(), 7);

        // tools/call mem_stats runs the real handler against the temp store.
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"mem_stats","arguments":{}}}"#,
            store.as_ref(),
            "proj",
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["result"]["isError"], false);
        let text = v["result"]["content"][0]["text"].as_str().unwrap();
        let stats: Value = serde_json::from_str(text).unwrap();
        assert_eq!(stats["project"], "proj");
        assert_eq!(stats["observations"], 0);
    }

    #[tokio::test]
    async fn save_returns_persisted_observation() {
        let tmp = TempDir::new().unwrap();
        let store = open_test_store(tmp.path()).await;

        let save = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"mem_save","arguments":{"type":"bugfix","title":"fix the thing","what":"did a thing"}}}"#,
            store.as_ref(),
            "proj",
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&save).unwrap();
        assert_eq!(v["result"]["isError"], false);
        let saved: Value =
            serde_json::from_str(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(saved["type"], "bugfix");
        assert_eq!(saved["title"], "fix the thing");
        assert!(saved["id"].as_str().is_some());
    }

    #[tokio::test]
    async fn unknown_method_and_unknown_tool_report_distinct_errors() {
        let tmp = TempDir::new().unwrap();
        let store = open_test_store(tmp.path()).await;

        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"bogus/method"}"#,
            store.as_ref(),
            "proj",
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], METHOD_NOT_FOUND);

        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#,
            store.as_ref(),
            "proj",
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        assert_eq!(v["error"]["code"], INVALID_PARAMS);
    }

    #[tokio::test]
    async fn bad_save_type_is_an_in_band_tool_error() {
        let tmp = TempDir::new().unwrap();
        let store = open_test_store(tmp.path()).await;

        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"mem_save","arguments":{"type":"bogus","title":"x"}}}"#,
            store.as_ref(),
            "proj",
        )
        .await
        .unwrap();
        let v: Value = serde_json::from_str(&resp).unwrap();
        // A bad observation type is a tool-level failure, not a protocol error.
        assert!(v.get("error").is_none());
        assert_eq!(v["result"]["isError"], true);
    }
}
