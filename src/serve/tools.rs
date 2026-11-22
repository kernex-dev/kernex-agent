//! Which tools a `kx serve` job may use.
//!
//! A serve job runs with nobody watching, on text the operator did not write:
//! a webhook payload, a file the job was asked to review, a page it fetched.
//! Anything in that text can try to steer the model. With every tool
//! available, a successful attempt can run commands, rewrite files and send
//! data out. With read-only tools the worst outcome is a wrong answer in the
//! job output, which only the caller sees.

use async_trait::async_trait;
use kernex_core::hooks::{HookOutcome, HookRunner};
use serde_json::Value;

/// Tool profile for every job of one `kx serve` process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum ToolProfile {
    /// Read, search and list files. No shell, no writes, no network.
    #[default]
    ReadOnly,
    /// Every tool, including the shell, file writes, `web_fetch`, MCP
    /// servers and toolboxes.
    Full,
}

/// In-process tools a read-only job may call (API providers).
const READ_ONLY_TOOLS: &[&str] = &["read", "grep", "glob"];

/// The same set under the names Claude Code uses for its own tools.
const READ_ONLY_CLAUDE_CODE_TOOLS: &[&str] = &["Read", "Grep", "Glob"];

impl ToolProfile {
    /// Whether a job under this profile may call `tool`.
    pub fn allows(self, tool: &str) -> bool {
        match self {
            ToolProfile::Full => true,
            ToolProfile::ReadOnly => READ_ONLY_TOOLS
                .iter()
                .any(|allowed| tool.eq_ignore_ascii_case(allowed)),
        }
    }

    /// Tool allowlist for providers that run their own tools in a child
    /// process, where the hook below never sees a call. `None` means no
    /// restriction.
    pub fn cli_tool_allowlist(self) -> Option<Vec<String>> {
        match self {
            ToolProfile::Full => None,
            ToolProfile::ReadOnly => Some(
                READ_ONLY_CLAUDE_CODE_TOOLS
                    .iter()
                    .map(|t| (*t).to_string())
                    .collect(),
            ),
        }
    }

    /// Name as the operator types it.
    pub fn as_str(self) -> &'static str {
        match self {
            ToolProfile::ReadOnly => "read-only",
            ToolProfile::Full => "full",
        }
    }
}

/// Pre-tool hook that enforces a [`ToolProfile`] on in-process tool calls.
pub struct ServeHookRunner {
    pub profile: ToolProfile,
    pub verbose: bool,
}

#[async_trait]
impl HookRunner for ServeHookRunner {
    async fn pre_tool(&self, tool_name: &str, _input: &Value) -> HookOutcome {
        if !self.profile.allows(tool_name) {
            tracing::warn!(
                tool = tool_name,
                "tool call refused by the serve tool profile"
            );
            return HookOutcome::Blocked(format!(
                "`{tool_name}` is not available: this server runs jobs with the \
                 `{}` tool profile. The operator can start it with `--tools full`.",
                self.profile.as_str()
            ));
        }
        if self.verbose {
            eprintln!("[tool] {tool_name}");
        }
        HookOutcome::Allow
    }

    async fn post_tool(&self, tool_name: &str, _result: &str, is_error: bool) {
        if self.verbose && is_error {
            eprintln!("[tool error] {tool_name}");
        }
    }

    async fn on_stop(&self, _final_text: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_is_read_only() {
        assert_eq!(ToolProfile::default(), ToolProfile::ReadOnly);
    }

    #[test]
    fn read_only_allows_reading_and_nothing_else() {
        let p = ToolProfile::ReadOnly;
        for tool in ["read", "grep", "glob", "Read", "GREP"] {
            assert!(p.allows(tool), "{tool} should be allowed");
        }
        for tool in [
            "bash",
            "write",
            "edit",
            "web_fetch",
            "mcp__github__create_issue",
            "some-toolbox",
            "",
        ] {
            assert!(!p.allows(tool), "{tool} should be refused");
        }
    }

    #[test]
    fn full_allows_everything() {
        for tool in ["bash", "write", "web_fetch", "mcp__x__y"] {
            assert!(ToolProfile::Full.allows(tool));
        }
    }

    #[test]
    fn cli_allowlist_is_set_only_when_restricted() {
        assert_eq!(ToolProfile::Full.cli_tool_allowlist(), None);
        assert_eq!(
            ToolProfile::ReadOnly.cli_tool_allowlist(),
            Some(vec![
                "Read".to_string(),
                "Grep".to_string(),
                "Glob".to_string()
            ])
        );
    }

    #[tokio::test]
    async fn hook_blocks_a_shell_call_under_read_only() {
        let hook = ServeHookRunner {
            profile: ToolProfile::ReadOnly,
            verbose: false,
        };
        let args = serde_json::json!({ "command": "curl https://example.com" });
        match hook.pre_tool("bash", &args).await {
            HookOutcome::Blocked(reason) => assert!(reason.contains("read-only")),
            HookOutcome::Allow => panic!("bash must be refused under read-only"),
        }
        assert!(matches!(
            hook.pre_tool("read", &args).await,
            HookOutcome::Allow
        ));
    }
}
