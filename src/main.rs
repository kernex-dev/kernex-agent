#![deny(clippy::unwrap_used, clippy::expect_used)]
#![deny(warnings)]

mod builtins;
mod cli;
mod commands;
mod config;
mod loader;
mod prompts;
mod scheduler;
mod serve;
mod skills;
mod stack;
mod utils;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use async_trait::async_trait;
use clap::Parser;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use kernex_core::context::ContextNeeds;
use kernex_core::hooks::{HookOutcome, HookRunner};
use kernex_core::message::Request;
use kernex_core::traits::Provider;
use kernex_providers::factory::{ProviderConfig as KxProviderConfig, ProviderFactory};
use kernex_runtime::{Runtime, RuntimeBuilder};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use serde_json::Value;

use crate::cli::{Cli, Command, CronAction, PipelineAction, SkillsAction};
use crate::commands::CommandResult;
use crate::serve::cmd_serve;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{} {e}", "error:".red().bold());
        std::process::exit(1);
    }
}

async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize a global tracing subscriber once, before subcommand dispatch.
    // Previously this was wired only inside `kx serve`, so every other command
    // (dev REPL, scheduler tick, pipeline runner, audit, docs) silently dropped
    // its `tracing::info!` / `tracing::warn!` output. `try_init` is used so
    // multiple subcommands within the same process (tests, or future
    // composition) don't panic on a re-init attempt.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .try_init();

    let provider_flags = ProviderFlags {
        name: cli.provider.clone(),
        model: cli.model.clone(),
        api_key: cli.api_key.clone(),
        base_url: cli.base_url.clone(),
        project: cli.project.clone(),
        channel: cli.channel.clone(),
        max_tokens: cli.max_tokens,
        no_memory: cli.no_memory,
        verbose: cli.verbose,
    };

    match cli.command {
        Some(Command::Dev { message }) => cmd_dev(message, &provider_flags).await,
        Some(Command::Audit) => cmd_audit(&provider_flags).await,
        Some(Command::Docs) => cmd_docs(&provider_flags).await,
        Some(Command::Init) => cmd_init().await,
        Some(Command::Pipeline { action }) => cmd_pipeline(action, &provider_flags).await,
        Some(Command::Skills { action }) => cmd_skills(action).await,
        Some(Command::Cron { action }) => cmd_cron(action).await,
        Some(Command::Serve {
            host,
            port,
            auth_token,
            workers,
        }) => cmd_serve(host, port, auth_token, workers, &provider_flags).await,
        None => cmd_dev(cli.message, &provider_flags).await,
    }
}

#[derive(Debug)]
pub(crate) struct CliHookRunner {
    pub(crate) verbose: bool,
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
pub(crate) struct ProviderFlags {
    pub(crate) name: String,
    pub(crate) model: Option<String>,
    pub(crate) api_key: Option<String>,
    pub(crate) base_url: Option<String>,
    pub(crate) project: Option<String>,
    pub(crate) channel: Option<String>,
    pub(crate) max_tokens: Option<u32>,
    pub(crate) no_memory: bool,
    pub(crate) verbose: bool,
}

async fn cmd_skills(action: SkillsAction) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let project_config = config::ProjectConfig::load(&cwd)?;
    let project_name = stack::project_name(&cwd);
    let data_dir = data_dir_for(&project_name);
    let policy = project_config.skills_policy();

    match action {
        SkillsAction::List => {
            skills::cli_handler::list_skills(&data_dir).await;
            Ok(())
        }
        SkillsAction::Add { source, trust } => {
            skills::cli_handler::add_skill(&data_dir, &source, &trust, &policy)
                .await
                .map_err(anyhow::Error::msg)
        }
        SkillsAction::Remove { name } => skills::cli_handler::remove_skill(&data_dir, &name)
            .await
            .map_err(anyhow::Error::msg),
        SkillsAction::Verify => {
            skills::cli_handler::verify_skills(&data_dir).await;
            Ok(())
        }
        SkillsAction::Lint { path } => {
            let lint_path = std::path::Path::new(&path);
            if !skills::cli_handler::lint_skill_dir(lint_path) {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

pub(crate) fn context_needs(no_memory: bool) -> ContextNeeds {
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

async fn cmd_dev(one_shot: Option<String>, flags: &ProviderFlags) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let project_config = config::ProjectConfig::load(&cwd)?;
    let detected_stack = project_config.resolve_stack(stack::detect(&cwd));
    let project_name = flags
        .project
        .clone()
        .unwrap_or_else(|| stack::project_name(&cwd));

    let data_dir = data_dir_for(&project_name);

    let mut system_prompt = prompts::dev_system_prompt(detected_stack, &project_name);

    let claude_md = loader::SystemPromptLoader::new(&cwd).load();
    if !claude_md.is_empty() {
        system_prompt.push_str("\n\n");
        system_prompt.push_str(&claude_md);
    }

    if let Some(extra) = &project_config.system_prompt {
        system_prompt.push_str("\n\n## Project-specific instructions\n");
        system_prompt.push_str(extra);
    }

    let skills_manifest = skills::manifest::SkillsManifest::load(&data_dir);
    let loaded_skills = skills::prompt::load_skills(&data_dir, skills_manifest.list());
    let skills_section = skills::prompt::build_skills_prompt(&loaded_skills);
    system_prompt.push_str(&skills_section);

    let (raw_provider, model_label) = build_provider(flags, &project_config)?;
    let provider: Arc<dyn Provider> = Arc::from(raw_provider);

    check_provider(provider.as_ref()).await?;

    let runtime = Arc::new(
        RuntimeBuilder::new()
            .data_dir(data_dir.to_str().unwrap_or("~/.kx"))
            .system_prompt(&system_prompt)
            .channel(flags.channel.as_deref().unwrap_or("cli"))
            .project(&project_name)
            .hook_runner(Arc::new(CliHookRunner {
                verbose: flags.verbose,
            }))
            .build()
            .await
            .with_context(|| {
                format!(
                    "initializing runtime for project '{project_name}' (data_dir = {})",
                    data_dir.display()
                )
            })?,
    );

    let needs = context_needs(flags.no_memory);

    if let Some(msg) = one_shot {
        // One-shot mode does not need the scheduler running.
        let request = Request::text("user", &msg);
        let response = runtime
            .complete_with_needs(provider.as_ref(), &request, &needs)
            .await?;
        println!("{}", response.text);
        commands::close_conversation(&runtime, "One-shot command completed.").await;
        return Ok(());
    }

    let scheduler_handle =
        scheduler::spawn(runtime.clone(), provider.clone(), context_needs(false), 60);

    let is_first_run = !data_dir.exists();
    if is_first_run {
        show_first_run_welcome(&detected_stack.to_string());
    }

    println!(
        "{} {} ({}) [{}]",
        "kx dev".green().bold(),
        project_name.bold(),
        detected_stack,
        model_label.cyan()
    );
    println!("{}", "Type /help for commands, /quit to exit.\n".dimmed());

    if skills_manifest.list().is_empty() {
        println!(
            "  {} No skills installed. Run {} to set up builtin skills.\n",
            "tip:".yellow(),
            "kx init".cyan().bold()
        );
    }

    let history_path = data_dir.join("history.txt");
    let editor = Arc::new(tokio::sync::Mutex::new(create_editor(&history_path)?));
    let mut last_input: Option<String> = None;

    loop {
        let input = {
            let ed = editor.clone();
            match tokio::task::spawn_blocking(move || ed.blocking_lock().readline("> ")).await? {
                Ok(line) => line,
                Err(ReadlineError::Interrupted) => {
                    graceful_shutdown(&runtime).await;
                    save_history(&editor, &history_path).await;
                    break;
                }
                Err(ReadlineError::Eof) => {
                    graceful_shutdown(&runtime).await;
                    save_history(&editor, &history_path).await;
                    break;
                }
                Err(e) => {
                    eprintln!("{} readline: {e}", "error:".red().bold());
                    break;
                }
            }
        };

        let trimmed = input.trim();

        if trimmed.is_empty() {
            continue;
        }

        editor.lock().await.add_history_entry(&input).ok();

        if trimmed == "\"\"\"" {
            let multiline = read_multiline(&editor).await;
            match multiline {
                Some(text) if !text.trim().is_empty() => {
                    let ok = send_message(&runtime, provider.as_ref(), &needs, &text).await;
                    if !ok {
                        last_input = Some(text);
                    } else {
                        last_input = None;
                    }
                }
                _ => continue,
            }
            continue;
        }

        if trimmed.starts_with("\"\"\"") {
            let first = trimmed.trim_start_matches("\"\"\"");
            let rest = read_multiline(&editor).await.unwrap_or_default();
            let full = format!("{first}\n{rest}");
            if !full.trim().is_empty() {
                let ok = send_message(&runtime, provider.as_ref(), &needs, &full).await;
                if !ok {
                    last_input = Some(full);
                } else {
                    last_input = None;
                }
            }
            continue;
        }

        if trimmed == "/retry" {
            match &last_input {
                Some(msg) => {
                    println!("{}", "  Retrying last message...".dimmed());
                    let ok = send_message(&runtime, provider.as_ref(), &needs, msg).await;
                    if ok {
                        last_input = None;
                    }
                }
                None => {
                    eprintln!("{}", "  Nothing to retry.\n".dimmed());
                }
            }
            continue;
        }

        if trimmed.starts_with('/') {
            match commands::handle(trimmed, &runtime, detected_stack, &project_config).await {
                CommandResult::Quit => {
                    graceful_shutdown(&runtime).await;
                    save_history(&editor, &history_path).await;
                    break;
                }
                CommandResult::Continue => continue,
                CommandResult::Unknown => {
                    eprintln!("{} Unknown command: {trimmed}\n", "warn:".yellow().bold());
                    continue;
                }
            }
        }

        let ok = send_message(&runtime, provider.as_ref(), &needs, trimmed).await;
        if !ok {
            last_input = Some(trimmed.to_string());
        } else {
            last_input = None;
        }
    }

    // Stop the scheduler so any in-flight task batch settles before the
    // tokio runtime drops; without this, the loop runs until the runtime
    // tears down and KAIROS tasks may be cancelled mid-completion.
    scheduler_handle.shutdown().await;

    Ok(())
}

async fn send_message(
    runtime: &Runtime,
    provider: &dyn Provider,
    needs: &ContextNeeds,
    input: &str,
) -> bool {
    let spinner = create_spinner("Thinking...");

    let request = Request::text("user", input);
    let result = runtime.complete_with_needs(provider, &request, needs).await;

    spinner.finish_and_clear();

    match result {
        Ok(response) => {
            println!("\n{}\n", response.text);
            true
        }
        Err(e) => {
            eprintln!("{} {e}\n", "error:".red().bold());
            false
        }
    }
}

fn create_spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    let style = ProgressStyle::default_spinner()
        .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]);
    if let Ok(s) = style.template("{spinner:.cyan} {msg}") {
        pb.set_style(s);
    }
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(80));
    pb
}

async fn read_multiline(editor: &Arc<tokio::sync::Mutex<DefaultEditor>>) -> Option<String> {
    println!(
        "{}",
        "  Multiline mode (\"\"\" to finish, Ctrl+C to cancel)".dimmed()
    );
    let mut lines = Vec::new();
    let mut line_num: usize = 1;
    loop {
        let prompt = format!("{} {} ", format!("{line_num:>3}").cyan(), "|".dimmed());
        let ed = editor.clone();
        match tokio::task::spawn_blocking(move || ed.blocking_lock().readline(&prompt))
            .await
            .ok()?
        {
            Ok(line) => {
                if line.trim() == "\"\"\"" {
                    break;
                }
                lines.push(line);
                line_num += 1;
            }
            Err(_) => return None,
        }
    }
    let count = lines.len();
    println!("{}", format!("  ({count} lines captured)").dimmed());
    Some(lines.join("\n"))
}

fn create_editor(history_path: &PathBuf) -> anyhow::Result<DefaultEditor> {
    let mut rl = DefaultEditor::new()?;
    let _ = rl.load_history(history_path);
    Ok(rl)
}

async fn save_history(editor: &Arc<tokio::sync::Mutex<DefaultEditor>>, history_path: &PathBuf) {
    if let Some(parent) = history_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = editor.lock().await.save_history(history_path);
}

pub(crate) fn build_provider(
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

    let cwd = std::env::current_dir().ok();

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
struct ProviderSpec {
    name: &'static str,
    api_key_env: Option<&'static str>,
    default_model: &'static str,
}

const PROVIDERS: &[ProviderSpec] = &[
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

async fn check_provider(provider: &dyn Provider) -> anyhow::Result<()> {
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

fn api_key_var(provider: &str) -> &'static str {
    provider_spec(provider)
        .and_then(|p| p.api_key_env)
        .unwrap_or("API_KEY")
}

fn env_api_key(provider: &str) -> Option<String> {
    let var = provider_spec(provider).and_then(|p| p.api_key_env)?;
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

async fn graceful_shutdown(runtime: &Runtime) {
    commands::close_conversation(runtime, "User exited session.").await;
    println!("{}", "Bye.".dimmed());
}

fn show_first_run_welcome(stack: &str) {
    println!();
    println!("{}", "Welcome to kx!".green().bold());
    println!("Your AI-powered coding assistant.\n");

    println!("Detected: {} project", stack);
    println!();
    println!("I can help you:");
    println!("  {} Answer questions about your code", "•".dimmed());
    println!("  {} Explain errors and suggest fixes", "•".dimmed());
    println!("  {} Review and refactor code", "•".dimmed());
    println!("  {} Remember context across sessions", "•".dimmed());
    println!();
    println!("Type {} for all commands.", "/help".cyan());
    println!();
}

const MIN_CLAUDE_VERSION: (u32, u32) = (2, 0);

async fn cmd_init() -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let project_name = stack::project_name(&cwd);
    let data_dir = data_dir_for(&project_name);

    std::fs::create_dir_all(&data_dir)?;

    println!(
        "  {} builtin skills from kernex-dev...",
        "Fetching".dimmed()
    );
    let installed = builtins::install_builtin_skills(&data_dir)?;

    println!("{}", "kx init complete.".green().bold());
    println!("  {} {}", "Project:".dimmed(), project_name.bold());
    println!("  {} {}", "Data dir:".dimmed(), data_dir.display());
    println!(
        "  {} {installed} builtin skills installed",
        "Skills:".dimmed()
    );
    println!("\n  Run {} to start coding.\n", "kx dev".cyan());
    Ok(())
}

async fn cmd_pipeline(action: PipelineAction, flags: &ProviderFlags) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let project_config = config::ProjectConfig::load(&cwd)?;
    let project_name = stack::project_name(&cwd);
    let data_dir = data_dir_for(&project_name);

    match action {
        PipelineAction::Run { name } => {
            let data_str = data_dir.to_str().unwrap_or("~/.kx");
            let loaded = kernex_pipelines::load_topology(data_str, &name)?;

            println!(
                "{} {} (v{})",
                "pipeline:".green().bold(),
                loaded.topology.topology.name.bold(),
                loaded.topology.topology.version
            );
            println!(
                "  {} {}",
                "Description:".dimmed(),
                loaded.topology.topology.description
            );
            println!(
                "  {} {} phases, {} agents\n",
                "Topology:".dimmed(),
                loaded.topology.phases.len(),
                loaded.agents.len()
            );

            let (provider, _model_label) = build_provider(flags, &project_config)?;
            check_provider(provider.as_ref()).await?;

            // Pre-build one Runtime per unique agent so each runs with its own system prompt.
            let mut agent_runtimes: std::collections::HashMap<String, Runtime> =
                std::collections::HashMap::new();
            for phase in &loaded.topology.phases {
                build_agent_runtime(
                    data_str,
                    &loaded,
                    &phase.agent,
                    &project_name,
                    &mut agent_runtimes,
                )
                .await?;
                if phase.phase_type == kernex_pipelines::PhaseType::CorrectiveLoop {
                    if let Some(ref retry) = phase.retry {
                        build_agent_runtime(
                            data_str,
                            &loaded,
                            &retry.fix_agent,
                            &project_name,
                            &mut agent_runtimes,
                        )
                        .await?;
                    }
                }
            }

            let needs = context_needs(flags.no_memory);

            let handoff_dir = cwd.join(".kx-pipeline").join(&name);
            std::fs::create_dir_all(&handoff_dir)?;

            let mut prev_output: Option<String> = None;

            for (i, phase) in loaded.topology.phases.iter().enumerate() {
                let phase_num = i + 1;
                let total = loaded.topology.phases.len();

                println!(
                    "{} [{phase_num}/{total}] {} (agent: {})",
                    "phase:".cyan().bold(),
                    phase.name.bold(),
                    phase.agent
                );

                if let Some(ref pre_val) = phase.pre_validation {
                    check_pre_validation(pre_val, &cwd)?;
                }

                let prompt = build_phase_prompt(
                    &phase.name,
                    &loaded.topology.topology.name,
                    prev_output.as_deref(),
                );

                let output = run_phase_with_retry(
                    &agent_runtimes,
                    provider.as_ref(),
                    &needs,
                    phase,
                    &cwd,
                    &prompt,
                )
                .await?;

                let handoff_file = handoff_dir.join(format!("{}.md", phase.name));
                std::fs::write(&handoff_file, output.as_bytes())?;

                println!("{}\n", output);
                prev_output = Some(output);
            }

            println!("{}", "Pipeline complete.".green().bold());
            Ok(())
        }
        PipelineAction::List => {
            let topologies_dir = data_dir.join("topologies");
            if !topologies_dir.exists() {
                println!(
                    "{}",
                    "  No pipelines found. Add topologies to ~/.kx/projects/<project>/topologies/\n"
                        .dimmed()
                );
                return Ok(());
            }

            println!("\n  {}\n", "Available pipelines".bold());
            let entries = std::fs::read_dir(&topologies_dir)?;
            let mut found = false;
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let toml_path = entry.path().join("TOPOLOGY.toml");
                    if toml_path.exists() {
                        println!("  {}", name.cyan());
                        found = true;
                    }
                }
            }
            if !found {
                println!("{}", "  No pipelines found.\n".dimmed());
            } else {
                println!();
            }
            Ok(())
        }
    }
}

async fn cmd_audit(flags: &ProviderFlags) -> anyhow::Result<()> {
    run_oneshot_command(flags, "audit", "kx audit", |stack, name| {
        format!(
            "Perform a comprehensive repository health audit for this {} project '{}'.\n\n\
             Check and report on:\n\
             1. **Dependencies** — outdated, vulnerable, or unused deps\n\
             2. **Tests** — test coverage, missing tests, flaky tests\n\
             3. **Code quality** — linting issues, dead code, complexity hotspots\n\
             4. **Documentation** — missing or outdated docs, README completeness\n\
             5. **Project structure** — file organization, naming conventions\n\
             6. **Security** — hardcoded secrets, insecure patterns\n\
             7. **CI/CD** — build config, missing checks\n\n\
             Provide a structured report with severity levels (critical/warning/info) \
             and actionable recommendations.",
            stack, name
        )
    })
    .await
}

async fn cmd_docs(flags: &ProviderFlags) -> anyhow::Result<()> {
    run_oneshot_command(flags, "docs", "kx docs", |stack, name| {
        format!(
            "Perform a documentation audit for this {} project '{}'.\n\n\
             Analyze and report on:\n\
             1. **README** — completeness, accuracy, setup instructions\n\
             2. **API docs** — missing or outdated function/module documentation\n\
             3. **Inline comments** — misleading or stale comments\n\
             4. **Examples** — missing usage examples, broken code snippets\n\
             5. **Changelog** — whether changes are tracked\n\
             6. **Architecture docs** — missing high-level design documentation\n\n\
             For each issue found, suggest specific fixes. \
             Flag any docs that reference code that no longer exists.",
            stack, name
        )
    })
    .await
}

async fn run_oneshot_command(
    flags: &ProviderFlags,
    channel: &str,
    label: &str,
    build_prompt: impl FnOnce(stack::Stack, &str) -> String,
) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let project_config = config::ProjectConfig::load(&cwd)?;
    let detected_stack = project_config.resolve_stack(stack::detect(&cwd));
    let project_name = stack::project_name(&cwd);
    let data_dir = data_dir_for(&project_name);

    let (provider, _model_label) = build_provider(flags, &project_config)?;
    check_provider(provider.as_ref()).await?;

    let system_prompt = prompts::dev_system_prompt(detected_stack, &project_name);

    let runtime = RuntimeBuilder::new()
        .data_dir(data_dir.to_str().unwrap_or("~/.kx"))
        .system_prompt(&system_prompt)
        .channel(channel)
        .project(&project_name)
        .hook_runner(Arc::new(CliHookRunner {
            verbose: flags.verbose,
        }))
        .build()
        .await?;

    let needs = context_needs(flags.no_memory);
    let prompt = build_prompt(detected_stack, &project_name);

    println!(
        "{} {} ({})\n",
        label.green().bold(),
        project_name.bold(),
        detected_stack
    );

    let spinner = create_spinner(&format!("Running {channel}..."));
    let request = Request::text("user", &prompt);
    let result = runtime
        .complete_with_needs(provider.as_ref(), &request, &needs)
        .await;
    spinner.finish_and_clear();

    match result {
        Ok(response) => {
            println!("{}\n", response.text);
        }
        Err(e) => {
            eprintln!("{} {channel} failed: {e}", "error:".red().bold());
        }
    }

    commands::close_conversation(&runtime, &format!("{label} completed.")).await;
    Ok(())
}

async fn cmd_cron(action: CronAction) -> anyhow::Result<()> {
    let cwd = std::env::current_dir()?;
    let project_name = stack::project_name(&cwd);
    let data_dir = data_dir_for(&project_name);

    let runtime = RuntimeBuilder::new()
        .data_dir(data_dir.to_str().unwrap_or("~/.kx"))
        .system_prompt("")
        .channel("cron")
        .project(&project_name)
        .build()
        .await?;

    match action {
        CronAction::Create {
            description,
            at,
            repeat,
        } => {
            let id = runtime
                .store
                .create_task(
                    "cli",
                    "user",
                    "cli",
                    &description,
                    &at,
                    repeat.as_deref(),
                    "scheduled",
                    &project_name,
                )
                .await
                .map_err(anyhow::Error::msg)?;
            println!("{} {}", "scheduled:".green().bold(), &id[..8.min(id.len())]);
            println!("  {} {}", "task:".dimmed(), description);
            println!("  {} {}", "at:".dimmed(), at);
            if let Some(r) = repeat {
                println!("  {} {}", "repeat:".dimmed(), r);
            }
        }
        CronAction::List => {
            let tasks = runtime
                .store
                .get_tasks_for_sender("user")
                .await
                .map_err(anyhow::Error::msg)?;
            if tasks.is_empty() {
                println!("{}", "  No scheduled tasks.\n".dimmed());
            } else {
                println!("\n  {}\n", "Scheduled tasks".bold());
                for (id, description, due_at, repeat, _task_type, _project) in &tasks {
                    let short = &id[..8.min(id.len())];
                    let repeat_label = repeat
                        .as_deref()
                        .map(|r| format!(" [{r}]"))
                        .unwrap_or_default();
                    println!("  {} {}{}", short.cyan(), description, repeat_label);
                    println!("       {} {}", "due:".dimmed(), due_at);
                }
                println!();
            }
        }
        CronAction::Delete { id } => {
            let cancelled = runtime
                .store
                .cancel_task(&id, "user")
                .await
                .map_err(anyhow::Error::msg)?;
            if cancelled {
                println!("{} {}", "cancelled:".green().bold(), id);
            } else {
                eprintln!(
                    "{} No pending task found with ID prefix: {id}",
                    "error:".red().bold()
                );
            }
        }
    }

    Ok(())
}

fn build_phase_prompt(phase_name: &str, pipeline_name: &str, prev_output: Option<&str>) -> String {
    let mut prompt = format!(
        "Execute phase '{}' of pipeline '{}'. Work in the current directory.",
        phase_name, pipeline_name
    );
    if let Some(prev) = prev_output {
        prompt.push_str("\n\n## Previous phase output\n");
        prompt.push_str(prev);
    }
    prompt
}

fn check_pre_validation(
    validation: &kernex_pipelines::ValidationConfig,
    cwd: &std::path::Path,
) -> anyhow::Result<()> {
    match &validation.validation_type {
        kernex_pipelines::ValidationType::FileExists => {
            for path in &validation.paths {
                if !cwd.join(path).exists() {
                    anyhow::bail!("Pre-validation failed: required file '{}' not found", path);
                }
            }
        }
        kernex_pipelines::ValidationType::FilePatterns => {
            for pattern in &validation.patterns {
                if !dir_contains_pattern(cwd, pattern) {
                    anyhow::bail!(
                        "Pre-validation failed: no file matching '{}' found",
                        pattern
                    );
                }
            }
        }
    }
    Ok(())
}

fn missing_post_validation_paths(paths: &[String], cwd: &std::path::Path) -> Vec<String> {
    paths
        .iter()
        .filter(|p| !cwd.join(p.as_str()).exists())
        .cloned()
        .collect()
}

fn dir_contains_pattern(dir: &std::path::Path, pattern: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            if dir_contains_pattern(&path, pattern) {
                return true;
            }
        } else if matches_glob_pattern(&name, pattern) {
            return true;
        }
    }
    false
}

fn matches_glob_pattern(name: &str, pattern: &str) -> bool {
    if !pattern.contains('*') {
        return name == pattern;
    }
    let parts: Vec<&str> = pattern.splitn(2, '*').collect();
    match parts.as_slice() {
        [prefix, suffix] => name.starts_with(prefix) && name.ends_with(suffix),
        _ => name == pattern,
    }
}

async fn build_agent_runtime(
    data_str: &str,
    loaded: &kernex_pipelines::LoadedTopology,
    agent_name: &str,
    project_name: &str,
    runtimes: &mut std::collections::HashMap<String, Runtime>,
) -> anyhow::Result<()> {
    if runtimes.contains_key(agent_name) {
        return Ok(());
    }
    let content = loaded.agent_content(agent_name)?;
    let runtime = RuntimeBuilder::new()
        .data_dir(data_str)
        .system_prompt(content)
        .channel("pipeline")
        .project(project_name)
        .build()
        .await?;
    runtimes.insert(agent_name.to_string(), runtime);
    Ok(())
}

async fn run_phase_with_retry(
    runtimes: &std::collections::HashMap<String, Runtime>,
    provider: &dyn Provider,
    needs: &ContextNeeds,
    phase: &kernex_pipelines::Phase,
    cwd: &std::path::Path,
    prompt: &str,
) -> anyhow::Result<String> {
    let runtime = runtimes
        .get(phase.agent.as_str())
        .ok_or_else(|| anyhow::anyhow!("No runtime for agent '{}'", phase.agent))?;

    let mut output = execute_single_phase(runtime, provider, needs, &phase.name, prompt).await?;

    match &phase.phase_type {
        kernex_pipelines::PhaseType::CorrectiveLoop => {}
        _ => return Ok(output),
    }

    let retry = match &phase.retry {
        Some(r) => r,
        None => return Ok(output),
    };

    let post_paths = match &phase.post_validation {
        Some(p) => p.clone(),
        None => return Ok(output),
    };

    let fix_runtime = runtimes
        .get(retry.fix_agent.as_str())
        .ok_or_else(|| anyhow::anyhow!("No runtime for fix agent '{}'", retry.fix_agent))?;

    for attempt in 0..retry.max {
        let missing = missing_post_validation_paths(&post_paths, cwd);
        if missing.is_empty() {
            return Ok(output);
        }

        eprintln!(
            "{} post-validation: {} path(s) missing (attempt {}/{}): {}",
            "warn:".yellow().bold(),
            missing.len(),
            attempt + 1,
            retry.max,
            missing.join(", ")
        );

        let fix_prompt = format!(
            "The following outputs from phase '{}' are missing:\n{}\n\n\
             Correct the issue so all required outputs are produced.",
            phase.name,
            missing.join("\n"),
        );

        if let Err(e) =
            execute_single_phase(fix_runtime, provider, needs, &retry.fix_agent, &fix_prompt).await
        {
            eprintln!(
                "{} fix agent '{}' failed: {e}",
                "warn:".yellow().bold(),
                retry.fix_agent
            );
        }

        output = execute_single_phase(runtime, provider, needs, &phase.name, prompt).await?;
    }

    let missing = missing_post_validation_paths(&post_paths, cwd);
    if !missing.is_empty() {
        anyhow::bail!(
            "Phase '{}' post-validation failed after {} retries. Missing: {}",
            phase.name,
            retry.max,
            missing.join(", ")
        );
    }

    Ok(output)
}

async fn execute_single_phase(
    runtime: &Runtime,
    provider: &dyn Provider,
    needs: &ContextNeeds,
    label: &str,
    prompt: &str,
) -> anyhow::Result<String> {
    let spinner = create_spinner(&format!("Running {label}..."));
    let request = Request::text("pipeline", prompt);
    let result = runtime.complete_with_needs(provider, &request, needs).await;
    spinner.finish_and_clear();
    Ok(result?.text)
}

pub(crate) fn data_dir_for(project_name: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".kx")
        .join("projects")
        .join(project_name)
}
