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
    let provider_name = config
        .provider
        .as_ref()
        .and_then(|pc| pc.name.clone())
        .unwrap_or_else(|| flags.name.clone());

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

    let base_url = flags
        .base_url
        .clone()
        .or_else(|| config.provider.as_ref().and_then(|pc| pc.base_url.clone()));

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

    let kx_config = KxProviderConfig {
        base_url,
        api_key,
        model,
        max_tokens: flags
            .max_tokens
            .or_else(|| config.provider.as_ref().and_then(|pc| pc.max_tokens)),
        workspace_path: cwd,
        ..Default::default()
    };

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

fn display_model(provider: &str, model: Option<&str>) -> String {
    let m = model.unwrap_or_else(|| default_model(provider));
    format!("{provider}/{m}")
}

/// Single source of truth for provider metadata. Each entry pairs the
/// provider name (`--provider <name>`) with its API-key env var (or `None`
/// if the provider does not need one) and a sensible default model.
pub struct ProviderSpec {
    pub name: &'static str,
    pub api_key_env: Option<&'static str>,
    pub default_model: &'static str,
}

pub const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        name: "claude-code",
        api_key_env: None,
        default_model: "claude-code",
    },
    ProviderSpec {
        name: "anthropic",
        api_key_env: Some("ANTHROPIC_API_KEY"),
        default_model: "claude-3-7-sonnet-20250219",
    },
    ProviderSpec {
        name: "openai",
        api_key_env: Some("OPENAI_API_KEY"),
        default_model: "gpt-4o",
    },
    ProviderSpec {
        name: "ollama",
        api_key_env: None,
        default_model: "llama3.2",
    },
    ProviderSpec {
        name: "gemini",
        api_key_env: Some("GEMINI_API_KEY"),
        default_model: "gemini-2.0-flash",
    },
    ProviderSpec {
        name: "openrouter",
        api_key_env: Some("OPENROUTER_API_KEY"),
        default_model: "anthropic/claude-sonnet-4-5",
    },
    ProviderSpec {
        name: "groq",
        api_key_env: Some("GROQ_API_KEY"),
        default_model: "llama-3.3-70b-versatile",
    },
    ProviderSpec {
        name: "mistral",
        api_key_env: Some("MISTRAL_API_KEY"),
        default_model: "mistral-large-latest",
    },
    ProviderSpec {
        name: "deepseek",
        api_key_env: Some("DEEPSEEK_API_KEY"),
        default_model: "deepseek-chat",
    },
    ProviderSpec {
        name: "fireworks",
        api_key_env: Some("FIREWORKS_API_KEY"),
        default_model: "accounts/fireworks/models/llama-v3p1-70b-instruct",
    },
    ProviderSpec {
        name: "xai",
        api_key_env: Some("XAI_API_KEY"),
        default_model: "grok-2-latest",
    },
];

fn provider_spec(name: &str) -> Option<&'static ProviderSpec> {
    PROVIDERS.iter().find(|p| p.name == name)
}

fn default_model(provider: &str) -> &'static str {
    provider_spec(provider)
        .map(|p| p.default_model)
        .unwrap_or("claude-code")
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
