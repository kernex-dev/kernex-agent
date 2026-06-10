//! MCP registration seam for the configurator.
//!
//! Registering kernex's MCP server with a host (Claude Code) is a *command*,
//! not a file write: we shell out to the host's own CLI (`claude mcp add-json`)
//! so the host owns the config format and location — the path the audit found
//! wrong twice. The trait abstracts that command so the apply/rollback stages
//! can be exercised in tests without spawning the real `claude` binary or
//! touching the user's real config.

use super::InstallError;

/// Result of an `add`: either the host registered the server, or no host CLI
/// was found so registration was skipped (there is nothing to register into).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterOutcome {
    Registered,
    SkippedNoClaude,
}

/// Abstracts registering/unregistering an MCP server with a host CLI.
/// Production uses [`ClaudeCliRegistrar`]; tests inject a recorder.
pub trait McpRegistrar: Send + Sync {
    /// Register `name` with the host using a stdio server JSON object
    /// (e.g. `{"command":"kx","args":["mcp"]}`) at the given scope.
    fn add(
        &self,
        name: &str,
        server_json: &str,
        scope: &str,
    ) -> Result<RegisterOutcome, InstallError>;

    /// Unregister `name` at `scope`. A missing host CLI is a no-op (nothing
    /// to undo). Used by rollback.
    fn remove(&self, name: &str, scope: &str) -> Result<(), InstallError>;
}

/// Production registrar: shells out to `claude mcp add-json` / `claude mcp
/// remove`. A missing `claude` binary is treated as "skip" (add) / "no-op"
/// (remove) rather than a hard failure, since there is no MCP runtime to
/// register into.
pub struct ClaudeCliRegistrar;

impl McpRegistrar for ClaudeCliRegistrar {
    fn add(
        &self,
        name: &str,
        server_json: &str,
        scope: &str,
    ) -> Result<RegisterOutcome, InstallError> {
        let output = std::process::Command::new("claude")
            .args(["mcp", "add-json", name, server_json, "--scope", scope])
            .output();
        match output {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(RegisterOutcome::SkippedNoClaude)
            }
            Err(e) => Err(InstallError::Permanent(format!(
                "failed to run `claude mcp add-json`: {e}"
            ))),
            Ok(o) if o.status.success() => Ok(RegisterOutcome::Registered),
            Ok(o) => Err(InstallError::Permanent(format!(
                "`claude mcp add-json {name}` failed: {}",
                String::from_utf8_lossy(&o.stderr).trim()
            ))),
        }
    }

    fn remove(&self, name: &str, scope: &str) -> Result<(), InstallError> {
        let output = std::process::Command::new("claude")
            .args(["mcp", "remove", name, "--scope", scope])
            .output();
        match output {
            // No claude binary (or the entry was never added): nothing to undo.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(InstallError::Permanent(format!(
                "failed to run `claude mcp remove`: {e}"
            ))),
            // A non-zero exit on remove is best-effort during rollback; the
            // caller logs it but does not abort the rest of the rollback.
            Ok(_) => Ok(()),
        }
    }
}
