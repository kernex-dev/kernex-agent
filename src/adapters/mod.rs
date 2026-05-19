//! Configurator adapters.
//!
//! Each adapter writes the per-tool config file that maps an external
//! coding assistant (Claude, Codex, OpenCode, Cursor, Cline, Windsurf)
//! onto a kernex runtime. Adapters are gated behind their own Cargo
//! features so a slim build can ship only the targets a given user
//! actually runs locally.
//!
//! `default_registry()` returns the name -> adapter lookup the
//! configurator pipeline consumes from §5 (DETECT) onward.

#[cfg(feature = "agent-claude")]
pub mod claude;

#[cfg(feature = "agent-codex")]
pub mod codex;

// Cross-adapter helpers (e.g. marker-block merge). Gated on the same
// `any(agent-*)` set as the registry so dead-code analysis stays quiet
// when no Tier 1 adapter is enabled. Codex is the first consumer; the
// Tier 1 OpenCode, Cursor, and Cline sprints will extend this cfg as
// they land.
#[cfg(feature = "agent-codex")]
pub mod shared;

#[cfg(any(feature = "agent-claude", feature = "agent-codex"))]
use std::sync::Arc;

#[cfg(any(feature = "agent-claude", feature = "agent-codex"))]
use kernex_runtime::Adapter;

/// Name-keyed adapter lookup. Each adapter feature contributes one match
/// arm; the registry surface stays uniform so call sites in
/// `src/configurator/stage_*.rs` do not change as adapters land.
#[cfg(any(feature = "agent-claude", feature = "agent-codex"))]
pub struct AgentRegistry;

#[cfg(any(feature = "agent-claude", feature = "agent-codex"))]
impl AgentRegistry {
    pub fn lookup(&self, name: &str) -> Option<Arc<dyn Adapter>> {
        match name {
            #[cfg(feature = "agent-claude")]
            "claude-code" => Some(Arc::new(claude::ClaudeAdapter)),
            #[cfg(feature = "agent-codex")]
            "codex" => Some(Arc::new(codex::CodexAdapter)),
            _ => None,
        }
    }
}

/// Build the default registry. Cheap (zero allocations); the adapter
/// instances are created on each `lookup()` call so the registry itself
/// is a stateless marker.
#[cfg(any(feature = "agent-claude", feature = "agent-codex"))]
pub fn default_registry() -> AgentRegistry {
    AgentRegistry
}

// Reserved for future Phase F adapters. Each is added behind its own
// `agent-*` feature flag mirroring the Codex pattern above:
//   #[cfg(feature = "agent-opencode")] pub mod opencode;
//   #[cfg(feature = "agent-cursor")]   pub mod cursor;
//   #[cfg(feature = "agent-cline")]    pub mod cline;
//   #[cfg(feature = "agent-windsurf")] pub mod windsurf;
