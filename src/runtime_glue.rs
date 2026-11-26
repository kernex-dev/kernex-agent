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

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use kernex_core::context::ContextNeeds;
use kernex_core::hooks::{HookOutcome, HookRunner};
use kernex_core::traits::Provider;
use kernex_providers::factory::{ProviderConfig as KxProviderConfig, ProviderFactory};
use serde_json::Value;

use crate::agent_sandbox::{sandbox_profile, AccessArgs, ToolAccess, UserConfig};
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
    /// `--web-fetch` and `--read-path`, resolved against config by
    /// [`build_provider`].
    pub access: AccessArgs,
}

/// What the runtime loads from the memory store into the prompt, and
/// whether it writes to it.
///
/// `no_memory` turns every load off and stops the runtime from storing
/// the exchange, so the session leaves the store as it found it.
/// `ContextNeeds::default()` is the opposite (everything on), so the
/// fields are spelled out here.
pub fn context_needs(no_memory: bool) -> ContextNeeds {
    if no_memory {
        ContextNeeds {
            recall: false,
            pending_tasks: false,
            profile: false,
            summaries: false,
            outcomes: false,
            store: false,
            ..Default::default()
        }
    } else {
        ContextNeeds {
            recall: true,
            summaries: true,
            profile: true,
            store: true,
            ..Default::default()
        }
    }
}

/// Context needs for a `kx serve` job: nothing is loaded from memory. An
/// isolated job also stores nothing, since no later job can read its
/// conversation; a job on a named channel stores its exchange so the next
/// job on that channel continues it.
pub fn job_context_needs(isolated: bool) -> ContextNeeds {
    ContextNeeds {
        store: !isolated,
        ..context_needs(true)
    }
}

/// Where a provider's tools run and what they may reach.
pub struct ProviderSetup {
    /// The kx data directory of the run (`~/.kx/projects/<name>`). Tools
    /// cannot write it; its memory database stays unreadable.
    pub data_dir: PathBuf,
    /// `web_fetch` hosts and extra read-only directories.
    pub access: ToolAccess,
    /// Tool allowlist for providers that run their own tools in a child
    /// process. `None` leaves them unrestricted.
    pub cli_tools: Option<Vec<String>>,
}

/// Build the provider for a run in the current directory that keeps its
/// state in `data_dir`, reading `--web-fetch` / `--read-path`, the
/// project's `.kx.toml` and `~/.kx/config.toml` for the tool access.
pub fn build_provider(
    flags: &ProviderFlags,
    config: &config::ProjectConfig,
    data_dir: &Path,
) -> anyhow::Result<(Box<dyn Provider>, String)> {
    let cwd = std::env::current_dir()?;
    let setup = provider_setup(flags, config, &UserConfig::load()?, &cwd, data_dir)?;
    build_provider_with_tools(flags, config, setup)
}

/// The setup [`build_provider`] builds with: the tool access and, for an
/// agent CLI, its tool list.
fn provider_setup(
    flags: &ProviderFlags,
    config: &config::ProjectConfig,
    user: &UserConfig,
    cwd: &Path,
    data_dir: &Path,
) -> anyhow::Result<ProviderSetup> {
    let access = ToolAccess::resolve(&flags.access, Some(config), user, cwd)?;
    let cli_tools =
        crate::cli_tools::for_provider(&flags.name, &flags.access, user, &access.web_fetch)?
            .map(|list| crate::cli_tools::without_blocked_fetch(list, &access.web_fetch))
            .transpose()?;
    Ok(ProviderSetup {
        data_dir: data_dir.to_path_buf(),
        access,
        cli_tools,
    })
}

/// [`build_provider`] with the setup given in full.
pub fn build_provider_with_tools(
    flags: &ProviderFlags,
    config: &config::ProjectConfig,
    setup: ProviderSetup,
) -> anyhow::Result<(Box<dyn Provider>, String)> {
    // The provider and its endpoint come from the user (`--provider`,
    // `--base-url`, or their env vars), never from the project's `.kx.toml`.
    // That file arrives with a cloned repository: honoring `provider.name`
    // or `provider.base_url` from it let a repo point kx at a host of its
    // choosing, which then received the user's API key and answered as the
    // model. The factory ignores case, so the name is folded here too: the
    // agent CLI arm below must catch `Claude-Code` as well.
    let provider_name = flags.name.trim().to_ascii_lowercase();
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

    let kx_config = KxProviderConfig {
        base_url,
        api_key,
        model,
        max_tokens: flags
            .max_tokens
            .or_else(|| config.provider.as_ref().and_then(|pc| pc.max_tokens)),
        workspace_path: cwd,
        ..base_provider_config(&setup.data_dir, &setup.access)
    };

    if matches!(provider_name.as_str(), "claude-code" | "opencode") {
        return cli_provider(&provider_name, kx_config, setup.cli_tools.as_deref())
            .map(|p| (p, label));
    }

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

/// Build a provider that is an agent CLI (`claude-code`, `opencode`).
///
/// Providers that call an HTTP API run tools inside kx, where the caller's
/// hook sees every call. An agent CLI runs its own tools in a child process
/// inside the OS sandbox: the only other control is the allowlist it is
/// started with. With `tools` set, the CLI may use exactly those (and never
/// runs with its permission checks off). Without, it runs with them off,
/// as an interactive coding assistant needs, still inside the sandbox.
/// `opencode` has no allowlist at all, so a restricted profile refuses it
/// rather than run it unrestricted.
///
/// claude-code is built here rather than by the runtime's factory so that
/// `CLAUDE_CODE_OAUTH_TOKEN` reaches the CLI: the sandbox clears the
/// environment, and the CLI no longer reads the login in `~/.claude`.
fn cli_provider(
    provider_name: &str,
    config: KxProviderConfig,
    tools: Option<&[String]>,
) -> anyhow::Result<Box<dyn Provider>> {
    use kernex_providers::claude_code::ClaudeCodeProvider;
    match (provider_name, tools) {
        ("claude-code", _) => {
            let dirs = sandbox_dirs(&config);
            let oauth_token = std::env::var(CLAUDE_OAUTH_TOKEN_ENV)
                .ok()
                .filter(|t| !t.is_empty());
            Ok(Box::new(
                ClaudeCodeProvider::from_config(
                    25,
                    tools.map(<[String]>::to_vec),
                    3600,
                    config.workspace_path,
                    5,
                    config.model.unwrap_or_default(),
                    oauth_token,
                )
                .with_sandbox_profile(config.sandbox_profile.unwrap_or_default())
                .with_sandbox_dirs(dirs)
                .with_permission_bypass(config.cli_permission_bypass && tools.is_none())
                .with_web_fetch_policy(config.web_fetch),
            ))
        }
        ("opencode", Some(_)) => anyhow::bail!(
            "the opencode provider cannot run with a restricted tool list: it has no tool \
             allowlist. Use another provider, or allow every tool: `--cli-tools all` (or \
             `cli_tools = \"all\"` in ~/.kx/config.toml), or `--tools full` for `kx serve`."
        ),
        _ => ProviderFactory::create(provider_name, config)
            .map_err(|e| anyhow::anyhow!(e.to_string())),
    }
}

/// The environment variable the Claude CLI reads its OAuth token from
/// (`claude setup-token` prints one).
pub const CLAUDE_OAUTH_TOKEN_ENV: &str = "CLAUDE_CODE_OAUTH_TOKEN";

/// The directories `config` asks the sandbox to protect.
fn sandbox_dirs(config: &KxProviderConfig) -> kernex_providers::SandboxDirs {
    let mut dirs = kernex_providers::SandboxDirs::new();
    if let Some(dir) = &config.data_dir {
        dirs = dirs.with_data_dir(dir);
    }
    if let Some(path) = &config.config_path {
        dirs = dirs.with_config_path(path);
    }
    if let Some(path) = &config.db_path {
        dirs = dirs.with_db_path(path);
    }
    dirs
}

/// Where the Claude CLI keeps its login when kx runs it with `data_dir`.
/// Logging in once with `CLAUDE_CONFIG_DIR` set to this directory makes
/// the claude-code provider work for that project.
pub fn claude_login_dir(data_dir: &Path) -> PathBuf {
    kernex_providers::claude_code::ClaudeCodeProvider::new()
        .with_sandbox_dirs(kernex_providers::SandboxDirs::new().with_data_dir(data_dir))
        .cli_config_dir()
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

/// The sandbox half of the provider config every kx entry point shares.
/// REPL, one-shot, pipelines and serve all construct providers through
/// [`build_provider_with_tools`], so this is the single place the
/// fail-closed sandbox posture and the protected directories are attached;
/// the tests below pin that they cannot be dropped silently.
///
/// The memory database and `~/.kx/config.toml` are named explicitly: the
/// first is where the runtime keeps it (`memory.db` in the data
/// directory), the second lives outside the data directory.
fn base_provider_config(data_dir: &Path, access: &ToolAccess) -> KxProviderConfig {
    KxProviderConfig {
        sandbox_profile: Some(sandbox_profile(access.read_paths.clone(), data_dir)),
        data_dir: Some(data_dir.to_path_buf()),
        db_path: Some(data_dir.join("memory.db")),
        config_path: Some(UserConfig::path()),
        web_fetch: access.web_fetch.clone(),
        cli_permission_bypass: true,
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
    kx_root().join("projects").join(project_name)
}

/// The directory kx keeps its state in: `~/.kx`.
pub fn kx_root() -> PathBuf {
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
    base.join(".kx")
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
            access: AccessArgs::default(),
        }
    }

    fn setup(cli_tools: Option<&[String]>) -> ProviderSetup {
        ProviderSetup {
            data_dir: PathBuf::from("/home/someone/.kx/projects/demo"),
            access: ToolAccess::default(),
            cli_tools: cli_tools.map(<[String]>::to_vec),
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
        let (_provider, label) =
            build_provider_with_tools(&test_flags("claude-code"), &config, setup(None)).unwrap();
        assert_eq!(label, "claude-code/m");

        // With a keyed provider the factory would reject the http URL if it
        // were passed through; building succeeds because it is not.
        let mut flags = test_flags("openai");
        flags.api_key = Some("test-key".to_string());
        let (_provider, label) = build_provider_with_tools(&flags, &config, setup(None)).unwrap();
        assert_eq!(label, "openai/m");
    }

    /// A restricted tool profile must never fall back to an unrestricted
    /// provider: `opencode` has no allowlist, so it is refused.
    #[test]
    fn restricted_tools_refuse_a_provider_without_an_allowlist() {
        let config = config::ProjectConfig::default();
        let tools = vec!["Read".to_string()];

        let Err(err) =
            build_provider_with_tools(&test_flags("opencode"), &config, setup(Some(&tools)))
        else {
            panic!("opencode must be refused under a restricted profile");
        };
        assert!(err.to_string().contains("--tools full"), "{err}");
        assert!(err.to_string().contains("--cli-tools all"), "{err}");

        // The factory ignores case, so the refusal must too.
        assert!(
            build_provider_with_tools(&test_flags("OpenCode"), &config, setup(Some(&tools)))
                .is_err(),
            "OpenCode skipped the refusal"
        );

        // The list kx dev uses when nothing is configured refuses it too.
        let default = crate::cli_tools::resolve(&[], None, &Default::default()).unwrap();
        assert!(default.is_some());
        assert!(build_provider_with_tools(
            &test_flags("opencode"),
            &config,
            setup(default.as_deref())
        )
        .is_err());

        // Unrestricted, it still builds.
        assert!(build_provider_with_tools(&test_flags("opencode"), &config, setup(None)).is_ok());
    }

    /// `build_provider` must hand an agent CLI its tool list: without one,
    /// claude-code runs every tool with its permission checks off.
    #[test]
    fn build_provider_gives_the_agent_cli_its_tool_list() {
        let config = config::ProjectConfig::default();
        let cwd = tempfile::tempdir().unwrap();
        let data_dir = cwd.path().join(".kx/projects/demo");
        let user = UserConfig::default();
        let default: Vec<String> = crate::cli_tools::DEFAULT_CLI_TOOLS
            .iter()
            .map(|t| t.to_string())
            .collect();

        let setup = provider_setup(
            &test_flags("claude-code"),
            &config,
            &user,
            cwd.path(),
            &data_dir,
        )
        .unwrap();
        assert_eq!(setup.cli_tools, Some(default));
        let setup = provider_setup(
            &test_flags("anthropic"),
            &config,
            &user,
            cwd.path(),
            &data_dir,
        )
        .unwrap();
        assert_eq!(setup.cli_tools, None, "API providers run their tools in kx");

        let all = UserConfig {
            cli_tools: Some(crate::cli_tools::CliToolsSetting::Keyword("all".into())),
            ..Default::default()
        };
        let setup = provider_setup(
            &test_flags("claude-code"),
            &config,
            &all,
            cwd.path(),
            &data_dir,
        )
        .unwrap();
        assert_eq!(setup.cli_tools, None, "\"all\" is the user's choice");
    }

    #[test]
    fn restricted_tools_still_build_claude_code_and_api_providers() {
        let config = config::ProjectConfig::default();
        let tools = vec!["Read".to_string(), "Grep".to_string()];

        let (provider, _) =
            build_provider_with_tools(&test_flags("claude-code"), &config, setup(Some(&tools)))
                .unwrap();
        assert_eq!(provider.name(), "claude-code");

        let mut flags = test_flags("openai");
        flags.api_key = Some("test-key".to_string());
        assert!(build_provider_with_tools(&flags, &config, setup(Some(&tools))).is_ok());
    }

    /// `--no-memory` once returned the default needs, which load everything,
    /// and then still stored every exchange.
    #[test]
    fn no_memory_loads_and_stores_nothing() {
        let needs = context_needs(true);
        assert!(!needs.recall);
        assert!(!needs.pending_tasks);
        assert!(!needs.profile);
        assert!(!needs.summaries);
        assert!(!needs.outcomes);
        assert!(!needs.store);

        let needs = context_needs(false);
        assert!(needs.recall && needs.profile && needs.summaries);
        assert!(needs.store);
    }

    /// An isolated serve job leaves memory as it found it; a job on a named
    /// channel stores its exchange so the next job on it can continue.
    #[test]
    fn serve_jobs_store_only_on_a_named_channel() {
        let isolated = job_context_needs(true);
        assert!(!isolated.store);
        assert!(!isolated.recall && !isolated.profile && !isolated.summaries);

        let shared = job_context_needs(false);
        assert!(shared.store);
        assert!(!shared.recall && !shared.profile && !shared.summaries);
    }

    /// A provider that answers without calling anything.
    struct FakeProvider;

    #[async_trait]
    impl Provider for FakeProvider {
        fn name(&self) -> &str {
            "fake"
        }
        fn requires_api_key(&self) -> bool {
            false
        }
        async fn complete(
            &self,
            _ctx: &kernex_core::context::Context,
        ) -> Result<kernex_core::message::Response, kernex_core::error::KernexError> {
            Ok(kernex_core::message::Response {
                text: "done".to_string(),
                metadata: kernex_core::message::CompletionMeta {
                    tokens_used: Some(42),
                    ..Default::default()
                },
            })
        }
        async fn is_available(&self) -> bool {
            true
        }
    }

    /// Conversations, messages and facts stored for `sender`, plus the
    /// number of recorded requests.
    async fn stored_rows(runtime: &kernex_runtime::Runtime, sender: &str) -> (i64, i64, i64, u64) {
        let (conversations, messages, _, facts) =
            runtime.store.get_memory_stats(sender).await.unwrap();
        let requests = runtime.store.get_total_usage().await.unwrap().request_count;
        (conversations, messages, facts, requests as u64)
    }

    /// `--no-memory` runs a real runtime against a real memory database and
    /// leaves it as it found it; the same call without the flag stores.
    #[tokio::test]
    async fn no_memory_session_adds_no_rows_to_the_memory_database() {
        let dir = tempfile::tempdir().unwrap();
        let runtime = kernex_runtime::RuntimeBuilder::new()
            .data_dir(dir.path().to_str().unwrap())
            .project("demo")
            .channel("cli")
            .build()
            .await
            .unwrap();
        let request = kernex_core::message::Request::text("user", "remember this");

        let before = stored_rows(&runtime, "user").await;
        runtime
            .complete_with_needs(&FakeProvider, &request, &context_needs(true))
            .await
            .unwrap();
        assert_eq!(stored_rows(&runtime, "user").await, before);

        runtime
            .complete_with_needs(&FakeProvider, &request, &context_needs(false))
            .await
            .unwrap();
        let after = stored_rows(&runtime, "user").await;
        assert!(after.1 > before.1, "a normal session stores its messages");
        assert!(after.3 > before.3, "a normal session records usage");
    }

    #[test]
    fn claude_login_dir_is_inside_the_project_data_dir() {
        let data_dir = Path::new("/home/someone/.kx/projects/demo");
        assert_eq!(
            claude_login_dir(data_dir),
            PathBuf::from("/home/someone/.kx/projects/demo/cli/claude")
        );
    }

    #[test]
    fn project_config_without_provider_overrides_is_silent() {
        let config: config::ProjectConfig =
            basic_toml::from_str("[provider]\nmodel = \"m\"\n").unwrap();
        assert!(ignored_project_provider_fields(&config).is_empty());
        assert!(ignored_project_provider_fields(&config::ProjectConfig::default()).is_empty());
    }

    #[test]
    fn every_provider_config_carries_the_fail_closed_profile() {
        // REPL, one-shot, pipelines AND serve construct providers through
        // build_provider_with_tools -> base_provider_config, so this seam is
        // the sandbox threading for all of them. The profile must be present
        // and fail-closed; egress stays deny-by-default.
        let data_dir = Path::new("/home/someone/.kx/projects/demo");
        let cfg = base_provider_config(data_dir, &ToolAccess::default());
        let profile = cfg.sandbox_profile.expect("sandbox profile missing");
        assert!(profile.require_os_enforcement);
        assert!(!profile.allow_network, "egress must default to deny");
        assert!(profile.read_paths.is_empty());
    }

    /// The sandbox must protect the directories kx really uses, not the
    /// runtime's default `~/.kernex`.
    #[test]
    fn every_provider_config_names_the_kx_data_dir_database_and_config() {
        let data_dir = Path::new("/home/someone/.kx/projects/demo");
        let cfg = base_provider_config(data_dir, &ToolAccess::default());
        assert_eq!(cfg.data_dir.as_deref(), Some(data_dir));
        assert_eq!(cfg.db_path, Some(data_dir.join("memory.db")));
        assert_eq!(cfg.config_path, Some(UserConfig::path()));
        assert_eq!(cfg.web_fetch, kernex_providers::WebFetchPolicy::Disabled);
    }

    #[test]
    fn provider_config_carries_the_resolved_access() {
        let access = ToolAccess {
            web_fetch: kernex_providers::WebFetchPolicy::AllowHosts(vec!["docs.rs".to_string()]),
            read_paths: vec![PathBuf::from("/opt/docs")],
        };
        let cfg = base_provider_config(Path::new("/d"), &access);
        assert_eq!(cfg.web_fetch, access.web_fetch);
        assert_eq!(
            cfg.sandbox_profile.unwrap().read_paths,
            vec![PathBuf::from("/opt/docs")]
        );
    }
}
