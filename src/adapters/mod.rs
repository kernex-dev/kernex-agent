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

#[cfg(feature = "agent-claude")]
use std::sync::Arc;

#[cfg(feature = "agent-claude")]
use kernex_runtime::Adapter;

/// Name-keyed adapter lookup. Stays minimal because the first adapter
/// change wires only one adapter; later changes extend the match arm
/// without changing the call sites in `src/configurator/stage_*.rs`.
#[cfg(feature = "agent-claude")]
pub struct AgentRegistry;

#[cfg(feature = "agent-claude")]
impl AgentRegistry {
    pub fn lookup(&self, name: &str) -> Option<Arc<dyn Adapter>> {
        match name {
            "claude-code" => Some(Arc::new(claude::ClaudeAdapter)),
            _ => None,
        }
    }
}

/// Build the default registry. Cheap (zero allocations); the adapter
/// instances are created on each `lookup()` call so the registry itself
/// is a stateless marker.
#[cfg(feature = "agent-claude")]
pub fn default_registry() -> AgentRegistry {
    AgentRegistry
}

// Reserved for future adapters. Each will be added behind its own
// `agent-*` feature flag:
//   #[cfg(feature = "agent-codex")]    pub mod codex;
//   #[cfg(feature = "agent-opencode")] pub mod opencode;
//   #[cfg(feature = "agent-cursor")]   pub mod cursor;
//   #[cfg(feature = "agent-cline")]    pub mod cline;
//   #[cfg(feature = "agent-windsurf")] pub mod windsurf;
