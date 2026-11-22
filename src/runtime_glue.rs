//! Runtime glue shared between the binary entry point (`src/main.rs`)
//! and library modules (notably `serve`) that need to build a provider
//! or compose a `Runtime`. Moved out of `main.rs` when `src/lib.rs` was
//! extracted, so the items here are reachable from `crate::` paths
//! inside the library and from `kernex_agent::runtime_glue::*` outside
//! it.
//!
//! Public items:
//! - [`CliHookRunner`] — simple verbose-aware `HookRunner` impl used by
//!   the interactive REPL and the headless serve loop.
//! - [`ProviderFlags`] — operator-facing flag bundle that
//!   [`build_provider`] resolves against `ProjectConfig`.
//! - [`context_needs`] — defaulted [`ContextNeeds`] policy honoring the
//!   `--no-memory` flag.
//! - [`build_provider`] — resolves a [`Provider`] from CLI flags +
//!   project config, returning the boxed provider plus a display label.
//! - [`data_dir_for`] — per-project state directory (`~/.kx/projects/<name>/`).
//!
//! Provider metadata lives in the private [`PROVIDERS`] table; updating
//! the list adds a new `--provider` choice plus its default model and
//! API-key env var.

use std::path::PathBuf;

use async_trait::async_trait;
use kernex_core::context::ContextNeeds;
use kernex_core::hooks::{HookOutcome, HookRunner};
use kernex_core::traits::Provider;
use kernex_providers::factory::{ProviderConfig as KxProviderConfig, ProviderFactory};
use serde_json::Value;

use crate::config;

pub struct CliHookRunner {
    pub verbose: bool,
}

#[async_trait]
impl HookRunner for CliHookRunner {
    async fn pre_tool(&self, tool_name: &str, _input: &Value) -> HookOutcome {
        if self.verbose {
            eprintln!("[tool] {tool_name}");
        }
        HookOutcome::Allow
    }

    async fn post_tool(&self, _tool_name: &str, _result: &str, is_error: bool) {
        if self.verbose && is_error {
            eprintln!("[tool error] {_tool_name}");
        }
    }

    async fn on_stop(&self, _final_text: &str) {}
}

#[derive(Clone)]
pub struct ProviderFlags {
    pub name: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub project: Option<String>,
    pub channel: Option<String>,
    pub max_tokens: Option<u32>,
    pub no_memory: bool,
    /// When true, the runtime summarizes overflow context instead of
    /// silently dropping it. Defaults to true; user can disable with
    /// `--no-auto-compact`.
    pub auto_compact: bool,
    pub verbose: bool,
}

/// Sandbox posture for every provider kx builds: OS-level enforcement is
/// required, never best-effort. A host that cannot apply Seatbelt (macOS)
/// or Landlock (Linux 5.13+) refuses to spawn agent tool subprocesses
/// instead of silently running them unsandboxed. Embedders of the kernex
/// crates choose their own posture; kx as a product chooses fail-closed.
pub fn sandbox_profile() -> kernex_sandbox::SandboxProfile {
    kernex_sandbox::SandboxProfile {
        require_os_enforcement: true,
        ..Default::default()
    }
}

pub fn context_needs(no_memory: bool) -> ContextNeeds {
    if no_memory {
        ContextNeeds::default()
    } else {
        ContextNeeds {
            recall: true,
            summaries: true,
            profile: true,
            ..Default::default()
        }
    }
}

pub fn build_provider(
    flags: &ProviderFlags,
    config: &config::ProjectConfig,
) -> anyhow::Result<(Box<dyn Provider>, String)> {
    // The provider and its endpoint come from the user (`--provider`,
    // `--base-url`, or their env vars), never from the project's `.kx.toml`.
    // That file arrives with a cloned repository: honoring `provider.name`
    // or `provider.base_url` from it let a repo point kx at a host of its
    // choosing, which then received the user's API key and answered as the
    // model.
    let provider_name = flags.name.clone();
    warn_ignored_project_provider_fields(config);

    let model = flags
        .model
        .clone()
        .or_else(|| config.provider.as_ref().and_then(|pc| pc.model.clone()));

    // API keys come from `--api-key` or per-provider env vars only. We
    // intentionally do not honor an `api_key` field in `.kx.toml` so secrets
    // never get committed alongside repo configuration.
    let api_key = flags
        .api_key
        .clone()
        .or_else(|| env_api_key(&provider_name));

    let base_url = flags.base_url.clone();

    // current_dir() failing is unusual (parent dir deleted, permissions
    // changed under us) but real. We let it fall through to None because
    // most providers can still operate without a workspace path; the
    // claude-code provider degrades to no --working-dir flag. Log so
    // diagnostic context isn't lost when something downstream complains.
    let cwd = match std::env::current_dir() {
        Ok(d) => Some(d),
        Err(e) => {
            tracing::warn!("current_dir() failed: {e}; provider workspace will be unset");
            None
        }
    };

    let label = display_model(&provider_name, model.as_deref());

    let kx_config = base_provider_config(
        base_url,
        api_key,
        model,
        flags
            .max_tokens
            .or_else(|| config.provider.as_ref().and_then(|pc| pc.max_tokens)),
        cwd,
    );

    let provider = ProviderFactory::create(&provider_name, kx_config).map_err(|e| {
        let msg = e.to_string();
        if msg.contains("unknown") || msg.contains("unsupported") || msg.contains("not found") {
            let names: Vec<&'static str> = PROVIDERS.iter().map(|p| p.name).collect();
            anyhow::anyhow!(
                "unknown provider '{}'. Valid choices: {}",
                provider_name,
                names.join(", ")
            )
        } else {
            anyhow::anyhow!(msg)
        }
    })?;
    Ok((provider, label))
}

/// Fields of `[provider]` in a project `.kx.toml` that kx parses but does
/// not act on. See [`build_provider`].
fn ignored_project_provider_fields(config: &config::ProjectConfig) -> Vec<&'static str> {
    let Some(pc) = config.provider.as_ref() else {
        return Vec::new();
    };
    let mut ignored = Vec::new();
    if pc.name.is_some() {
        ignored.push("provider.name");
    }
    if pc.base_url.is_some() {
        ignored.push("provider.base_url");
    }
    ignored
}

/// Tell the user, once per process, that `.kx.toml` asked for a provider
/// or endpoint and kx did not follow it.
fn warn_ignored_project_provider_fields(config: &config::ProjectConfig) {
    static WARNED: std::sync::Once = std::sync::Once::new();
    let ignored = ignored_project_provider_fields(config);
    if ignored.is_empty() {
        return;
    }
    WARNED.call_once(|| {
        eprintln!(
            "warning: .kx.toml sets {}, which kx ignores: a project file cannot choose \
             where your API key is sent. Use --provider / KERNEX_PROVIDER and \
             --base-url / KERNEX_BASE_URL instead.",
            ignored.join(" and ")
        );
    });
}

/// Assemble the provider config every kx entry point shares. REPL,
/// one-shot, and serve all construct providers through [`build_provider`],
/// so this is the single place the fail-closed sandbox posture is
/// attached; the test below pins that it cannot be dropped silently.
fn base_provider_config(
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
    max_tokens: Option<u32>,
    workspace_path: Option<PathBuf>,
) -> KxProviderConfig {
    KxProviderConfig {
        base_url,
        api_key,
        model,
        max_tokens,
        workspace_path,
        sandbox_profile: Some(sandbox_profile()),
        ..Default::default()
    }
}

fn display_model(provider: &str, model: Option<&str>) -> String {
    let m = model.unwrap_or_else(|| default_model(provider));
    format!("{provider}/{m}")
}

/// Single source of truth for provider metadata. Each entry pairs the
/// provider name (`--provider <name>`) with its API-key env var (or `None`
/// if the provider does not need one). Default models are not kept here:
/// they come from `ProviderFactory::default_model` in the runtime.
pub struct ProviderSpec {
    pub name: &'static str,
    pub api_key_env: Option<&'static str>,
}

pub const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        name: "claude-code",
        api_key_env: None,
    },
    ProviderSpec {
        name: "opencode",
        api_key_env: None,
    },
    ProviderSpec {
        name: "anthropic",
        api_key_env: Some("ANTHROPIC_API_KEY"),
    },
    ProviderSpec {
        name: "openai",
        api_key_env: Some("OPENAI_API_KEY"),
    },
    ProviderSpec {
        name: "ollama",
        api_key_env: None,
    },
    ProviderSpec {
        name: "gemini",
        api_key_env: Some("GEMINI_API_KEY"),
    },
    ProviderSpec {
        name: "openrouter",
        api_key_env: Some("OPENROUTER_API_KEY"),
    },
    ProviderSpec {
        name: "groq",
        api_key_env: Some("GROQ_API_KEY"),
    },
    ProviderSpec {
        name: "mistral",
        api_key_env: Some("MISTRAL_API_KEY"),
    },
    ProviderSpec {
        name: "deepseek",
        api_key_env: Some("DEEPSEEK_API_KEY"),
    },
    ProviderSpec {
        name: "fireworks",
        api_key_env: Some("FIREWORKS_API_KEY"),
    },
    ProviderSpec {
        name: "xai",
        api_key_env: Some("XAI_API_KEY"),
    },
];

fn provider_spec(name: &str) -> Option<&'static ProviderSpec> {
    PROVIDERS.iter().find(|p| p.name == name)
}

/// The model shown in the banner when the user sets none. It comes from the
/// runtime's own table, so the label always names what the factory will
/// send. CLI-backed providers (`claude-code`, `opencode`) choose their own
/// model and have no entry; the label repeats the provider name for them.
fn default_model(provider: &str) -> &'static str {
    ProviderFactory::default_model(provider).unwrap_or_else(|| {
        provider_spec(provider)
            .map(|p| p.name)
            .unwrap_or("claude-code")
    })
}

pub fn api_key_var(provider: &str) -> &'static str {
    provider_spec(provider)
        .and_then(|p| p.api_key_env)
        .unwrap_or("API_KEY")
}

fn env_api_key(provider: &str) -> Option<String> {
    let var = provider_spec(provider).and_then(|p| p.api_key_env)?;
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

pub fn data_dir_for(project_name: &str) -> PathBuf {
    let base = match dirs::home_dir() {
        Some(home) => home,
        None => {
            // No HOME (stripped containers, init scripts running as a
            // service account, etc.). Falling back to "." would scatter
            // memory.db / skills.toml / jobs.db into the user's project
            // directory and risk committing them. Use /tmp instead and
            // warn so the operator notices.
            tracing::warn!(
                "dirs::home_dir() returned None; falling back to /tmp/.kx (set HOME explicitly)"
            );
            PathBuf::from("/tmp")
        }
    };
    base.join(".kx").join("projects").join(project_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The banner must name the model the factory will send. kx once kept
    /// its own table, and it drifted: the label said one model while the
    /// runtime sent another.
    #[test]
    fn display_label_matches_the_runtime_default() {
        for spec in PROVIDERS {
            let label = display_model(spec.name, None);
            match ProviderFactory::default_model(spec.name) {
                Some(model) => assert_eq!(label, format!("{}/{model}", spec.name)),
                None => assert_eq!(label, format!("{0}/{0}", spec.name)),
            }
        }
        assert_eq!(display_model("deepseek", Some("custom")), "deepseek/custom");
    }

    fn test_flags(name: &str) -> ProviderFlags {
        ProviderFlags {
            name: name.to_string(),
            model: None,
            api_key: None,
            base_url: None,
            project: None,
            channel: None,
            max_tokens: None,
            no_memory: false,
            auto_compact: true,
            verbose: false,
        }
    }

    /// A cloned repository must not be able to pick the provider or the
    /// endpoint: either one decides where the user's API key goes.
    #[test]
    fn project_config_cannot_choose_provider_or_endpoint() {
        let config: config::ProjectConfig = basic_toml::from_str(
            "[provider]\nname = \"ollama\"\nbase_url = \"http://attacker.example\"\nmodel = \"m\"\n",
        )
        .unwrap();
        assert_eq!(
            ignored_project_provider_fields(&config),
            vec!["provider.name", "provider.base_url"]
        );

        // The flag's provider is the one built; the file's model is still
        // honored because it cannot redirect anything.
        let (_provider, label) = build_provider(&test_flags("claude-code"), &config).unwrap();
        assert_eq!(label, "claude-code/m");

        // With a keyed provider the factory would reject the http URL if it
        // were passed through; building succeeds because it is not.
        let mut flags = test_flags("openai");
        flags.api_key = Some("test-key".to_string());
        let (_provider, label) = build_provider(&flags, &config).unwrap();
        assert_eq!(label, "openai/m");
    }

    #[test]
    fn project_config_without_provider_overrides_is_silent() {
        let config: config::ProjectConfig =
            basic_toml::from_str("[provider]\nmodel = \"m\"\n").unwrap();
        assert!(ignored_project_provider_fields(&config).is_empty());
        assert!(ignored_project_provider_fields(&config::ProjectConfig::default()).is_empty());
    }

    #[test]
    fn sandbox_profile_requires_os_enforcement() {
        // kx's fail-closed posture: every provider gets a profile that
        // refuses unsandboxed tool subprocesses. If this flag ever flips
        // back to best-effort it must be a deliberate, reviewed change.
        assert!(sandbox_profile().require_os_enforcement);
    }

    #[test]
    fn every_provider_config_carries_the_fail_closed_profile() {
        // REPL, one-shot, AND serve construct providers through
        // build_provider -> base_provider_config, so this seam is the
        // serve path's sandbox threading. The profile must be present
        // and fail-closed; egress stays deny-by-default.
        let cfg = base_provider_config(None, None, None, None, None);
        let profile = cfg.sandbox_profile.expect("sandbox profile missing");
        assert!(profile.require_os_enforcement);
        assert!(!profile.allow_network, "egress must default to deny");
    }
}
