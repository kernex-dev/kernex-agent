//! Auto-JSON renderer for `kx mem *` output.
//!
//! Per ADR-004, every `kx mem *` subcommand checks
//! `std::io::IsTerminal::is_terminal` on stdout. When false, output is JSON;
//! ANSI color codes are suppressed; help and error text route to stderr.
//!
//! `--compact` projects to high-gravity fields (`id`, `type`, `title`,
//! `updated_at`, `score`). `--select fld1,fld2` projects arbitrary fields;
//! unknown fields exit `2`.
//!
//! Renderer functions land in follow-up commits per `tasks.md` Step 2.12.

use std::io::IsTerminal;

/// True when stdout is a real terminal (CLI is being read by a human), false
/// when piped or redirected (CLI is being read by another agent).
#[allow(dead_code)]
pub fn is_tty() -> bool {
    std::io::stdout().is_terminal()
}
