#![deny(clippy::unwrap_used, clippy::expect_used)]
#![deny(warnings)]

mod cli;
mod commands;
mod config;
mod prompts;
mod skills;
mod stack;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use kernex_core::context::ContextNeeds;
use kernex_core::message::Request;
use kernex_providers::claude_code::ClaudeCodeProvider;
use kernex_runtime::{Runtime, RuntimeBuilder};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use crate::cli::{Cli, Command, SkillsAction};
use crate::commands::CommandResult;

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("{} {e}", "error:".red().bold());
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Dev { message }) => cmd_dev(message).await,
        Some(Command::Audit) => {
            eprintln!("{}", "kx audit is not yet implemented.".yellow());
            Ok(())
        }
        Some(Command::Docs) => {
            eprintln!("{}", "kx docs is not yet implemented.".yellow());
            Ok(())
        }
        Some(Command::Skills { action }) => cmd_skills(action).await,
        None => cmd_dev(cli.message).await,
    }
}

async fn cmd_skills(action: SkillsAction) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let project_config = config::ProjectConfig::load(&cwd);
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
                .map_err(|e| e.into())
        }
        SkillsAction::Remove { name } => skills::cli_handler::remove_skill(&data_dir, &name)
            .await
            .map_err(|e| e.into()),
        SkillsAction::Verify => {
            skills::cli_handler::verify_skills(&data_dir).await;
            Ok(())
        }
    }
}

fn context_needs() -> ContextNeeds {
    ContextNeeds {
        recall: true,
        summaries: true,
        profile: true,
        pending_tasks: false,
        outcomes: false,
    }
}

async fn cmd_dev(one_shot: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let project_config = config::ProjectConfig::load(&cwd);
    let detected_stack = project_config.resolve_stack(stack::detect(&cwd));
    let project_name = stack::project_name(&cwd);

    let data_dir = data_dir_for(&project_name);

    let mut system_prompt = prompts::dev_system_prompt(detected_stack, &project_name);
    if let Some(extra) = &project_config.system_prompt {
        system_prompt.push_str("\n\n## Project-specific instructions\n");
        system_prompt.push_str(extra);
    }

    let skills_manifest = skills::manifest::SkillsManifest::load(&data_dir);
    let loaded_skills = skills::prompt::load_skills(&data_dir, skills_manifest.list());
    let skills_section = skills::prompt::build_skills_prompt(&loaded_skills);
    system_prompt.push_str(&skills_section);

    let provider = build_provider(&project_config);

    if !ClaudeCodeProvider::check_cli().await {
        eprintln!(
            "{} Claude CLI not found. Install it: https://docs.anthropic.com/en/docs/claude-code",
            "error:".red().bold()
        );
        return Err("claude CLI not available".into());
    }

    check_claude_version();

    let runtime = RuntimeBuilder::new()
        .data_dir(data_dir.to_str().unwrap_or("~/.kx"))
        .system_prompt(&system_prompt)
        .channel("cli")
        .project(&project_name)
        .build()
        .await?;

    let needs = context_needs();

    if let Some(msg) = one_shot {
        let request = Request::text("user", &msg);
        let response = runtime
            .complete_with_needs(&provider, &request, &needs)
            .await?;
        println!("{}", response.text);
        commands::close_conversation(&runtime, "One-shot command completed.").await;
        return Ok(());
    }

    let is_first_run = !data_dir.exists();
    if is_first_run {
        show_first_run_welcome(&detected_stack.to_string());
    }

    println!(
        "{} {} ({})",
        "kx dev".green().bold(),
        project_name.bold(),
        detected_stack
    );
    println!("{}", "Type /help for commands, /quit to exit.\n".dimmed());

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
                    let ok = send_message(&runtime, &provider, &needs, &text).await;
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
                let ok = send_message(&runtime, &provider, &needs, &full).await;
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
                    let ok = send_message(&runtime, &provider, &needs, msg).await;
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

        let ok = send_message(&runtime, &provider, &needs, trimmed).await;
        if !ok {
            last_input = Some(trimmed.to_string());
        } else {
            last_input = None;
        }
    }

    Ok(())
}

async fn send_message(
    runtime: &Runtime,
    provider: &ClaudeCodeProvider,
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

fn create_editor(history_path: &PathBuf) -> Result<DefaultEditor, Box<dyn std::error::Error>> {
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

fn build_provider(config: &config::ProjectConfig) -> ClaudeCodeProvider {
    match &config.provider {
        Some(pc) if pc.max_turns.is_some() || pc.timeout_secs.is_some() || pc.model.is_some() => {
            ClaudeCodeProvider::from_config(
                pc.max_turns.unwrap_or(10),
                vec![],
                pc.timeout_secs.unwrap_or(300),
                None,
                3,
                pc.model.clone().unwrap_or_default(),
                None,
            )
        }
        _ => ClaudeCodeProvider::new(),
    }
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

fn check_claude_version() {
    let output = std::process::Command::new("claude")
        .arg("--version")
        .output();

    let version_str = match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        Err(_) => return,
    };

    // Parse "2.1.70 (Claude Code)" -> (2, 1)
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
}

fn data_dir_for(project_name: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".kx")
        .join("projects")
        .join(project_name)
}
