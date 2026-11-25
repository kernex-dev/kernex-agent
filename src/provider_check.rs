//! Is the chosen provider usable, and if not, what should the user do?
//!
//! The claude-code provider runs the Claude CLI with its own config
//! directory inside the kx data directory, so a login made in a normal
//! terminal (`~/.claude`) does not carry over. [`login_hint`] turns the
//! CLI's "not logged in" failure into the three ways to fix it.

use std::path::Path;

use colored::Colorize;
use kernex_core::traits::Provider;

use crate::runtime_glue::{api_key_var, claude_login_dir, CLAUDE_OAUTH_TOKEN_ENV};

/// Oldest Claude CLI release kx is tested against.
const MIN_CLAUDE_VERSION: (u32, u32) = (2, 0);

/// Fail with a readable message when `provider` cannot be used.
pub async fn check_provider(provider: &dyn Provider) -> anyhow::Result<()> {
    if provider.name() == "claude-code" {
        return check_claude_cli();
    }

    if !provider.is_available().await {
        let msg = if provider.name() == "ollama" {
            "Ollama server not reachable. Start it with: ollama serve".to_string()
        } else if provider.requires_api_key() {
            let var = api_key_var(provider.name());
            format!(
                "Provider '{}' unavailable. Set {var} or pass --api-key.",
                provider.name()
            )
        } else {
            format!("Provider '{}' is not available.", provider.name())
        };
        return Err(anyhow::anyhow!(msg));
    }
    Ok(())
}

fn check_claude_cli() -> anyhow::Result<()> {
    let output = std::process::Command::new("claude")
        .arg("--version")
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let version_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let parts: Vec<&str> = version_str.split(|c: char| !c.is_ascii_digit()).collect();
            let major = parts.first().and_then(|s| s.parse::<u32>().ok());
            let minor = parts.get(1).and_then(|s| s.parse::<u32>().ok());

            if let (Some(maj), Some(min)) = (major, minor) {
                if (maj, min) < MIN_CLAUDE_VERSION {
                    eprintln!(
                        "{} Claude CLI {version_str} is below minimum {}.{}. Please update.",
                        "warn:".yellow().bold(),
                        MIN_CLAUDE_VERSION.0,
                        MIN_CLAUDE_VERSION.1,
                    );
                }
            }
            Ok(())
        }
        _ => {
            eprintln!("{}", "error: Claude CLI not found".red().bold());
            eprintln!();
            eprintln!("  To fix this:");
            eprintln!("    1. Install Claude Code: https://claude.ai/download");
            eprintln!("    2. Verify installation: which claude");
            eprintln!("    3. If installed, add to PATH: export PATH=\"$PATH:/path/to/claude\"");
            eprintln!();
            anyhow::bail!("claude CLI not available")
        }
    }
}

/// Whether an error from the Claude CLI says it has no usable login.
fn reports_missing_login(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "not logged in",
        "please run /login",
        "invalid api key",
        "oauth token",
        "authentication_error",
    ]
    .iter()
    .any(|needle| error.contains(needle))
}

/// How to log the claude-code provider in for the project whose data
/// directory is `data_dir`.
pub fn login_instructions(data_dir: &Path) -> String {
    format!(
        "kx runs the Claude CLI with its own login, not the one in ~/.claude. Either:\n  \
         - export {CLAUDE_OAUTH_TOKEN_ENV} (create one with `claude setup-token`),\n  \
         - export ANTHROPIC_API_KEY, or\n  \
         - log in once for this project: CLAUDE_CONFIG_DIR={} claude",
        claude_login_dir(data_dir).display()
    )
}

/// The login instructions, when `error` from `provider` is a missing login.
pub fn login_hint(provider: &str, data_dir: &Path, error: &str) -> Option<String> {
    (provider == "claude-code" && reports_missing_login(error))
        .then(|| login_instructions(data_dir))
}

/// `error` as text, followed by the login instructions when it is a
/// missing claude-code login.
pub fn explain_error(provider: &str, data_dir: &Path, error: &dyn std::fmt::Display) -> String {
    let error = error.to_string();
    match login_hint(provider, data_dir, &error) {
        Some(hint) => format!("{error}\n{hint}"),
        None => error,
    }
}

/// One line for `kx doctor`: which claude-code login kx will use.
pub fn claude_login_status(data_dir: &Path) -> String {
    let set = |var: &str| std::env::var(var).is_ok_and(|v| !v.is_empty());
    if set(CLAUDE_OAUTH_TOKEN_ENV) {
        format!("login: {CLAUDE_OAUTH_TOKEN_ENV} is set")
    } else if set("ANTHROPIC_API_KEY") {
        "login: ANTHROPIC_API_KEY is set".to_string()
    } else {
        format!(
            "login: none in the environment; kx uses the login in {} (not ~/.claude)",
            claude_login_dir(data_dir).display()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_login_errors_get_the_instructions() {
        let data_dir = Path::new("/home/someone/.kx/projects/demo");
        let error = "claude CLI exited with exit status: 1: Invalid API key - Please run /login";
        let hint = login_hint("claude-code", data_dir, error).unwrap();
        assert!(hint.contains(CLAUDE_OAUTH_TOKEN_ENV));
        assert!(hint.contains("ANTHROPIC_API_KEY"));
        assert!(hint.contains("CLAUDE_CONFIG_DIR=/home/someone/.kx/projects/demo/cli/claude"));

        assert!(login_hint("claude-code", data_dir, "Not logged in").is_some());
    }

    #[test]
    fn other_errors_and_providers_get_no_login_hint() {
        let data_dir = Path::new("/d");
        assert!(login_hint("claude-code", data_dir, "timed out after 3600s").is_none());
        assert!(login_hint("openai", data_dir, "Invalid API key").is_none());
    }
}
