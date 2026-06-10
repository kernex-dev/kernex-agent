//! Minimal JSON-RPC 2.0 types for the `kx mcp` stdio MCP server.
//!
//! MCP's stdio transport is newline-delimited JSON-RPC 2.0: one JSON object
//! per line on stdin (requests/notifications from the host) and stdout
//! (responses from this server). This mirrors the request/response shapes
//! the workspace's MCP *client* already uses, but for the server direction.

use serde::{Deserialize, Serialize};

/// MCP `protocolVersion` this server defaults to when the host does not send
/// one. Matches the version the workspace client negotiates. When the host
/// sends its own version in `initialize`, we echo that back instead so the
/// two ends agree on whatever the host speaks.
pub const MCP_PROTOCOL_VERSION: &str = "2025-03-26";

/// An incoming JSON-RPC message. A request carries an `id`; a notification
/// omits it (and gets no response).
#[derive(Debug, Deserialize)]
pub struct Incoming {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    /// Absent for notifications. `serde_json::Value` because the spec allows
    /// string or number ids and we only ever echo it back verbatim.
    #[serde(default)]
    pub id: Option<serde_json::Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

impl Incoming {
    /// True when this is a notification (no `id`): per JSON-RPC, the server
    /// must NOT reply to notifications.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// A JSON-RPC error object. Codes follow the JSON-RPC spec: -32601 method
/// not found, -32602 invalid params, -32603 internal error, -32700 parse.
#[derive(Debug, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
}

/// An outgoing JSON-RPC response. Exactly one of `result`/`error` is set.
#[derive(Debug, Serialize)]
pub struct Outgoing {
    pub jsonrpc: &'static str,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Outgoing {
    pub fn ok(id: serde_json::Value, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: serde_json::Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

// JSON-RPC error codes used by this server.
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const PARSE_ERROR: i64 = -32700;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_with_id_is_not_a_notification() {
        let raw = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let inc: Incoming = serde_json::from_str(raw).unwrap();
        assert!(!inc.is_notification());
        assert_eq!(inc.method, "tools/list");
    }

    #[test]
    fn message_without_id_is_a_notification() {
        let raw = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        let inc: Incoming = serde_json::from_str(raw).unwrap();
        assert!(inc.is_notification());
    }

    #[test]
    fn ok_response_serializes_without_error_field() {
        let out = Outgoing::ok(serde_json::json!(7), serde_json::json!({"x": 1}));
        let v = serde_json::to_value(&out).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 7);
        assert_eq!(v["result"]["x"], 1);
        assert!(v.get("error").is_none(), "error must be omitted on success");
    }

    #[test]
    fn err_response_serializes_without_result_field() {
        let out = Outgoing::err(serde_json::json!(8), METHOD_NOT_FOUND, "nope");
        let v = serde_json::to_value(&out).unwrap();
        assert_eq!(v["error"]["code"], METHOD_NOT_FOUND);
        assert_eq!(v["error"]["message"], "nope");
        assert!(v.get("result").is_none(), "result must be omitted on error");
    }
}
