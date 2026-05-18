//! Claude Code adapter implementation per ADR-002 / E-claude-1..8.
//!
//! `ClaudeAdapter` implements `kernex_adapter_core::Adapter` for
//! `AdapterId::ClaudeCode`. Templates are compiled in via `include_str!`
//! so air-gapped installs work (E-CC-7). The hand-rolled `{{key}}`
//! substituter avoids a template-engine dependency that would breach the
//! 800 KiB delta budget (E-LOCK-08 / E-CC-5).
//!
//! Known upstream limitation: `kernex_adapter_core::Detection` is
//! `#[non_exhaustive]` and has no constructor in 0.8.x, so `detect()`
//! routes through `serde_json::from_value` to build the return value. The
//! cleaner fix is a constructor or builder on the upstream type; tracked
//! as FU-E-01 (open an upstream patch release).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use async_trait::async_trait;
use kernex_adapter_core::Detection;
use kernex_runtime::{Adapter, AdapterError, AdapterId, Capability};

/// Canonical Anthropic install one-liner per E-claude-4.
const INSTALL_COMMAND: &str = "curl -fsSL https://claude.ai/install.sh | sh";

/// Compiled-in templates per ADR-002. Loaded once at binary link time;
/// no runtime template directory lookup.
pub const CLAUDE_MD_TMPL: &str = include_str!("../../templates/claude/CLAUDE.md.tmpl");
pub const MCP_JSON_TMPL: &str = include_str!("../../templates/claude/mcp.json.tmpl");
pub const OUTPUT_STYLE_TMPL: &str = include_str!("../../templates/claude/output-style.md.tmpl");

/// Unit struct identity for the Claude Code adapter. The adapter is
/// stateless; configuration flows through `InstallOptions` at the
/// configurator boundary.
#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeAdapter;

#[async_trait]
impl Adapter for ClaudeAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::ClaudeCode
    }

    fn supports(&self, cap: Capability) -> bool {
        matches!(
            cap,
            Capability::Skills | Capability::Memory | Capability::Mcp | Capability::OutputStyle
        )
    }

    async fn detect(&self) -> Result<Detection, AdapterError> {
        let claude_path = locate_claude();
        let installed = claude_path.is_some();
        let version = if installed {
            read_claude_version()
        } else {
            None
        };
        let config_root = home_dir().map(|h| h.join(".claude"));
        // `Detection` is `#[non_exhaustive]` upstream (kernex-adapter-core
        // 0.8.x) and has no constructor, so we route through serde.
        // FU-E-01 tracks the upstream patch.
        let value = serde_json::json!({
            "installed": installed,
            "config_root": config_root,
            "version": version,
        });
        Ok(serde_json::from_value(value)?)
    }

    async fn install_command(&self) -> Result<String, AdapterError> {
        Ok(INSTALL_COMMAND.to_string())
    }
}

/// Replace `{{key}}` occurrences in `tmpl` with `vars[key]`. Unknown keys
/// are left literal in the output AND surface a `tracing::warn!` per
/// E-claude-6.
pub fn render(tmpl: &str, vars: &HashMap<&str, String>) -> String {
    let mut out = String::with_capacity(tmpl.len());
    let mut rest = tmpl;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after_open = &rest[start + 2..];
        let Some(end) = after_open.find("}}") else {
            // No closing braces; emit the rest literally and stop.
            out.push_str(&rest[start..]);
            return out;
        };
        let key = after_open[..end].trim();
        match vars.get(key) {
            Some(val) => out.push_str(val),
            None => {
                tracing::warn!(
                    target: "kernex.install.template",
                    "unknown template key '{{{{{key}}}}}' left literal in output"
                );
                out.push_str(&rest[start..start + end + 4]);
            }
        }
        rest = &after_open[end + 2..];
    }
    out.push_str(rest);
    out
}

fn locate_claude() -> Option<PathBuf> {
    let output = Command::new("which").arg("claude").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn read_claude_version() -> Option<String> {
    let output = Command::new("claude").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}
