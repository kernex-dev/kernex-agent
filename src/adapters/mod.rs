//! Configurator adapters.
//!
//! Each adapter writes the per-tool config file that maps an external
//! coding assistant (Claude, Codex, OpenCode, Cursor, Cline, Windsurf)
//! onto a kernex runtime. Adapters are gated behind their own Cargo
//! features so a slim build can ship only the targets a given user
//! actually runs locally.
//!
//! This module is a placeholder. Submodules will land alongside the
//! corresponding adapter implementations under the `cargo-feature-graph`
//! openspec change.

#[cfg(feature = "agent-claude")]
pub mod claude;

// Reserved for future adapters. Each will be added behind its own
// `agent-*` feature flag:
//   #[cfg(feature = "agent-codex")]    pub mod codex;
//   #[cfg(feature = "agent-opencode")] pub mod opencode;
//   #[cfg(feature = "agent-cursor")]   pub mod cursor;
//   #[cfg(feature = "agent-cline")]    pub mod cline;
//   #[cfg(feature = "agent-windsurf")] pub mod windsurf;
