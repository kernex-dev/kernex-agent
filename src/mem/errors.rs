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

use std::fmt;

#[derive(Debug)]
pub enum CliError {
    /// Subcommand is recognized by the parser but not yet wired to a handler.
    /// Returned by every handler in the scaffold commit until follow-up
    /// commits land the trait calls.
    NotImplemented { subcommand: &'static str },
    /// Reserved for future use.
    #[allow(dead_code)]
    Usage { message: String, hint: String },
    /// Reserved for future use.
    #[allow(dead_code)]
    NotFound { message: String, hint: String },
    /// Reserved for future use.
    #[allow(dead_code)]
    Sandbox { message: String, hint: String },
    /// Reserved for future use.
    #[allow(dead_code)]
    Runtime { message: String, hint: String },
}

impl CliError {
    /// Returned to the OS via `std::process::ExitCode` once the dispatcher
    /// is wired to honor it. Not yet consumed in the scaffold commit; the
    /// top-level `main` still maps every error to exit 1.
    #[allow(dead_code)]
    pub fn exit_code(&self) -> u8 {
        match self {
            CliError::NotImplemented { .. } | CliError::Usage { .. } => 2,
            CliError::NotFound { .. } => 3,
            CliError::Sandbox { .. } => 4,
            CliError::Runtime { .. } => 5,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::NotImplemented { subcommand } => write!(
                f,
                "{subcommand} is not yet implemented; follow-up commits land the handler"
            ),
            CliError::Usage { message, .. }
            | CliError::NotFound { message, .. }
            | CliError::Sandbox { message, .. }
            | CliError::Runtime { message, .. } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for CliError {}
