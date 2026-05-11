//! Structured CLI errors for `kx mem *` subcommands.
//!
//! Each variant maps onto the exit-code taxonomy from ADR-005:
//!
//! | Code | Variant |
//! |------|---------|
//! | 2    | `Usage`, `NotImplemented` |
//! | 3    | `NotFound` |
//! | 4    | `Sandbox` |
//! | 5    | `Runtime` |
//!
//! In JSON mode (auto-JSON when stdout is not a TTY, or `--json` forced),
//! the error is emitted as a one-line JSON object on stderr:
//!
//! ```json
//! {"error":{"code":3,"message":"...","hint":"..."}}
//! ```

/// Structured CLI errors emitted by `kx mem *` subcommands.
///
/// The `Display` impl renders only the message; the renderer in
/// [`super::render`] reassembles the hint into the structured stderr
/// shape (CC-6). Variant naming maps directly to ADR-005 exit codes via
/// [`CliError::exit_code`]; the variant identifier itself is also surfaced
/// as the `kernex.error_kind` tracing field by [`CliError::kind_name`].
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Subcommand is recognized by the parser but not yet wired to a
    /// handler. Returned by every stub handler until follow-up commits
    /// land the trait calls.
    #[error("{subcommand} is not yet implemented")]
    NotImplemented { subcommand: &'static str },
    /// Operator-facing usage error: unknown flag value, malformed
    /// argument, conflicting flag combination.
    #[error("{message}")]
    Usage { message: String, hint: String },
    /// Lookup miss: requested id / key absent from the store. Soft-deleted
    /// rows surface here per CC-9. Wired by Step 2.4 (`kx mem get`).
    #[allow(dead_code)]
    #[error("{message}")]
    NotFound { message: String, hint: String },
    /// Sandbox / authorization refusal. Reserved for `kx mem save` (Step
    /// 2.8) and any future write surface that crosses a policy boundary.
    #[allow(dead_code)]
    #[error("{message}")]
    Sandbox { message: String, hint: String },
    /// Runtime fault: DB locked, IO failure, schema mismatch, or any
    /// underlying error surfaced from `kernex_memory::MemoryError`.
    #[error("{message}")]
    Runtime { message: String, hint: String },
}

impl CliError {
    /// OS exit code returned by `main` when a `kx mem *` subcommand exits
    /// with this error. `main`'s top-level handler downcasts via
    /// [`crate::exit_code_for`]; every other error path defaults to 1.
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::NotImplemented { .. } | CliError::Usage { .. } => 2,
            CliError::NotFound { .. } => 3,
            CliError::Sandbox { .. } => 4,
            CliError::Runtime { .. } => 5,
        }
    }

    /// Stable, lowercase variant identifier suitable for the
    /// `kernex.error_kind` tracing field. Never contains operator-supplied
    /// content; safe to emit alongside the structured stderr line.
    pub fn kind_name(&self) -> &'static str {
        match self {
            CliError::NotImplemented { .. } => "not_implemented",
            CliError::Usage { .. } => "usage",
            CliError::NotFound { .. } => "not_found",
            CliError::Sandbox { .. } => "sandbox",
            CliError::Runtime { .. } => "runtime",
        }
    }

    /// Hint string suitable for the `Try:` line on a TTY or the `hint`
    /// field in JSON mode. `NotImplemented` carries an inline default;
    /// every other variant returns the explicit field.
    pub fn hint(&self) -> &str {
        match self {
            CliError::NotImplemented { .. } => "follow-up commits land the handler",
            CliError::Usage { hint, .. }
            | CliError::NotFound { hint, .. }
            | CliError::Sandbox { hint, .. }
            | CliError::Runtime { hint, .. } => hint.as_str(),
        }
    }
}
