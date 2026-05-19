//! Internal `Preset` type and inline preset resolver per ADR-006.
//!
//! kernex-agent does NOT depend on an external preset crate; this module
//! is the seam a future change uses if preset management eventually moves
//! to a published crate. The type is intentionally crate-internal so
//! consumers outside this crate cannot pin to it as a stable API surface.

use kernex_runtime::AdapterId;
use serde::{Deserialize, Serialize};

use crate::configurator::InstallError;

/// Resolved preset shape consumed by RESOLVE (§6).
///
/// `adapters` lists the `AdapterId`s the preset turns on; `components`
/// names the install components each adapter should render (e.g.
/// `"claude-md"`, `"mcp-json"`, `"output-style"`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Preset {
    pub adapters: Vec<AdapterId>,
    pub components: Vec<String>,
}

/// Resolve a preset name + agent to its typed shape.
///
/// Component identifiers are agent-specific so the per-tool file
/// layout stays explicit: `claude-md`/`mcp-json`/`output-style` for
/// Claude Code; `agents-md`/`config-toml`/`output-style` for Codex.
/// The downstream `component_path` resolver dispatches on the same
/// `(agent, component)` pair to compute the target path.
///
/// Unknown preset names return `InstallError::UnknownPreset(name)`;
/// unknown agents return `InstallError::UnknownAgent(agent)` so the
/// CLI dispatcher can surface a clean exit-2 usage error per
/// E-install-5. A future change replaces this inline table with a
/// real preset catalog (likely TOML-backed).
pub fn resolve_preset(name: &str, agent: &str) -> Result<Preset, InstallError> {
    match (name, agent) {
        ("solo-dev", "claude-code") => Ok(Preset {
            adapters: vec![AdapterId::ClaudeCode],
            components: vec![
                "claude-md".to_string(),
                "mcp-json".to_string(),
                "output-style".to_string(),
            ],
        }),
        ("solo-dev", "codex") => Ok(Preset {
            adapters: vec![AdapterId::CodexCli],
            components: vec![
                "agents-md".to_string(),
                "config-toml".to_string(),
                "output-style".to_string(),
            ],
        }),
        ("solo-dev", other) => Err(InstallError::UnknownAgent(other.to_string())),
        (other, _) => Err(InstallError::UnknownPreset(other.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solo_dev_claude_returns_expected_preset() {
        let preset =
            resolve_preset("solo-dev", "claude-code").expect("solo-dev/claude-code resolves");
        assert_eq!(preset.adapters, vec![AdapterId::ClaudeCode]);
        assert_eq!(
            preset.components,
            vec!["claude-md", "mcp-json", "output-style"]
        );
    }

    #[test]
    fn solo_dev_codex_returns_expected_preset() {
        let preset = resolve_preset("solo-dev", "codex").expect("solo-dev/codex resolves");
        assert_eq!(preset.adapters, vec![AdapterId::CodexCli]);
        assert_eq!(
            preset.components,
            vec!["agents-md", "config-toml", "output-style"]
        );
    }

    #[test]
    fn unknown_preset_errors_with_unknown_preset_variant() {
        let err = resolve_preset("not-a-preset", "claude-code").expect_err("must error");
        match err {
            InstallError::UnknownPreset(name) => assert_eq!(name, "not-a-preset"),
            other => panic!("expected UnknownPreset, got {other:?}"),
        }
    }

    #[test]
    fn unknown_agent_under_known_preset_errors_with_unknown_agent() {
        let err = resolve_preset("solo-dev", "not-an-agent").expect_err("must error");
        match err {
            InstallError::UnknownAgent(name) => assert_eq!(name, "not-an-agent"),
            other => panic!("expected UnknownAgent, got {other:?}"),
        }
    }

    #[test]
    fn preset_is_clone_and_serialize() {
        // Forward-compat: the struct must stay trivially serializable so
        // RESOLVE can include the resolved preset in the audit log
        // payload, and a future preset catalog can round-trip through
        // disk without API churn.
        let preset = resolve_preset("solo-dev", "claude-code").unwrap();
        let cloned = preset.clone();
        assert_eq!(preset, cloned);
        let json = serde_json::to_string(&preset).expect("serialize");
        let back: Preset = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, preset);
    }
}
