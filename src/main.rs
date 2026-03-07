mod cli;
mod commands;
mod prompts;
mod stack;

use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use colored::Colorize;
use kernex_core::context::ContextNeeds;
use kernex_core::message::Request;
use kernex_providers::claude_code::ClaudeCodeProvider;
use kernex_runtime::{Runtime, RuntimeBuilder};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

use crate::cli::{Cli, Command};
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
        Command::Dev { message } => cmd_dev(message).await,
        Command::Audit => {
            eprintln!("{}", "kx audit is not yet implemented.".yellow());
            Ok(())
        }
        Command::Docs => {
            eprintln!("{}", "kx docs is not yet implemented.".yellow());
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
    let detected_stack = stack::detect(&cwd);
    let project_name = stack::project_name(&cwd);

    let data_dir = data_dir_for(&project_name);
    let system_prompt = prompts::dev_system_prompt(detected_stack, &project_name);

    let provider = ClaudeCodeProvider::new();

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

    println!(
        "{} {} ({})",
        "kx dev".green().bold(),
        project_name.bold(),
        detected_stack
    );
    println!("{}", "Type /help for commands, /quit to exit.\n".dimmed());

    let history_path = data_dir.join("history.txt");
    let editor = Arc::new(tokio::sync::Mutex::new(create_editor(&history_path)?));

    loop {
        let input = {
            let ed = editor.clone();
            match tokio::task::spawn_blocking(move || {
                ed.blocking_lock().readline("> ")
            })
            .await?
            {
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
                    send_message(&runtime, &provider, &needs, &text).await;
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
                send_message(&runtime, &provider, &needs, &full).await;
            }
            continue;
        }

        if trimmed.starts_with('/') {
            match commands::handle(trimmed, &runtime, detected_stack).await {
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

        send_message(&runtime, &provider, &needs, trimmed).await;
    }

    Ok(())
}

async fn send_message(
    runtime: &Runtime,
    provider: &ClaudeCodeProvider,
    needs: &ContextNeeds,
    input: &str,
) {
    let request = Request::text("user", input);
    match runtime
        .complete_with_needs(provider, &request, needs)
        .await
    {
        Ok(response) => {
            println!("\n{}\n", response.text);
        }
        Err(e) => {
            eprintln!("{} {e}\n", "error:".red().bold());
        }
    }
}

async fn read_multiline(editor: &Arc<tokio::sync::Mutex<DefaultEditor>>) -> Option<String> {
    println!(
        "{}",
        "  Multiline mode. Type \"\"\" to finish.".dimmed()
    );
    let mut lines = Vec::new();
    loop {
        let ed = editor.clone();
        match tokio::task::spawn_blocking(move || ed.blocking_lock().readline(".. "))
            .await
            .ok()?
        {
            Ok(line) => {
                if line.trim() == "\"\"\"" {
                    break;
                }
                lines.push(line);
            }
            Err(_) => return None,
        }
    }
    Some(lines.join("\n"))
}

fn create_editor(history_path: &PathBuf) -> Result<DefaultEditor, Box<dyn std::error::Error>> {
    let mut rl = DefaultEditor::new()?;
    let _ = rl.load_history(history_path);
    Ok(rl)
}

async fn save_history(
    editor: &Arc<tokio::sync::Mutex<DefaultEditor>>,
    history_path: &PathBuf,
) {
    if let Some(parent) = history_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = editor.lock().await.save_history(history_path);
}

async fn graceful_shutdown(runtime: &Runtime) {
    commands::close_conversation(runtime, "User exited session.").await;
    println!("{}", "Bye.".dimmed());
}

fn data_dir_for(project_name: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".kx")
        .join("projects")
        .join(project_name)
}
