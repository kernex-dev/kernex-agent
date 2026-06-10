//! Unit tests for the Claude Code adapter (§4).
//!
//! Covers E-claude-1..7.

#![cfg(feature = "agent-claude")]

use std::collections::HashMap;

use kernex_agent::adapters::claude::{render, ClaudeAdapter};
use kernex_runtime::{Adapter, AdapterId, Capability};

#[test]
fn id_returns_claude_code() {
    let adapter = ClaudeAdapter;
    assert_eq!(adapter.id(), AdapterId::ClaudeCode);
}

#[test]
fn supports_reports_skills_memory_mcp_output_style() {
    let adapter = ClaudeAdapter;
    assert!(adapter.supports(Capability::Skills));
    assert!(adapter.supports(Capability::Memory));
    assert!(adapter.supports(Capability::Mcp));
    assert!(adapter.supports(Capability::OutputStyle));
}

#[tokio::test]
async fn detect_returns_installed_false_when_no_claude_on_path() {
    // Set PATH to a directory that surely contains no `claude` binary.
    let saved = std::env::var_os("PATH");
    // SAFETY: tests run single-threaded by default for cargo test --test;
    // even if multi-threaded, restoring PATH below keeps the env consistent.
    unsafe {
        std::env::set_var("PATH", "/nonexistent-kernex-test-dir");
    }
    let detection = ClaudeAdapter.detect().await.expect("detect ok");
    if let Some(prior) = saved {
        unsafe { std::env::set_var("PATH", prior) };
    } else {
        unsafe { std::env::remove_var("PATH") };
    }
    assert!(!detection.installed);
    assert!(detection.version.is_none());
}

#[test]
fn render_substitutes_known_keys() {
    let mut vars = HashMap::new();
    vars.insert("name", "Jose".to_string());
    vars.insert("project", "kernex-agent".to_string());
    let out = render("Hello {{name}}, welcome to {{project}}.", &vars);
    assert_eq!(out, "Hello Jose, welcome to kernex-agent.");
}

#[test]
fn render_leaves_unknown_keys_literal() {
    let mut vars = HashMap::new();
    vars.insert("known", "yes".to_string());
    let out = render("known={{known}} unknown={{unknown}}", &vars);
    assert_eq!(out, "known=yes unknown={{unknown}}");
}
