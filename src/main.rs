mod cli;
mod commands;
mod prompts;
mod stack;

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use colored::Colorize;
use kernex_core::context::ContextNeeds;
use kernex_core::message::Request;
use kernex_providers::claude_code::ClaudeCodeProvider;
use kernex_runtime::{Runtime, RuntimeBuilder};
use tokio::sync::Notify;

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

    let shutdown = setup_ctrlc_handler(&runtime).await;

    loop {
        print!("{} ", ">".cyan().bold());
        io::stdout().flush()?;

        let input = match read_input(&shutdown).await {
            Some(line) => line,
            None => break, // EOF or Ctrl+C
        };

        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input.starts_with('/') {
            match commands::handle(input, &runtime, detected_stack).await {
                CommandResult::Quit => {
                    graceful_shutdown(&runtime).await;
                    break;
                }
                CommandResult::Continue => continue,
                CommandResult::Unknown => {
                    eprintln!("{} Unknown command: {input}\n", "warn:".yellow().bold());
                    continue;
                }
            }
        }

        let request = Request::text("user", input);
        match runtime
            .complete_with_needs(&provider, &request, &needs)
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

    Ok(())
}

async fn read_line_async(shutdown: &Arc<Notify>) -> Option<String> {
    let shutdown = shutdown.clone();
    tokio::select! {
        result = tokio::task::spawn_blocking(|| {
            let mut buf = String::new();
            let n = io::stdin().read_line(&mut buf)?;
            Ok::<_, io::Error>((buf, n))
        }) => {
            match result.ok()? {
                Ok((_, 0)) => None,
                Ok((buf, _)) => Some(buf),
                Err(_) => None,
            }
        }
        _ = shutdown.notified() => None,
    }
}

async fn read_input(shutdown: &Arc<Notify>) -> Option<String> {
    let line = read_line_async(shutdown).await?;

    if line.trim() == "\"\"\"" {
        println!("{}", "  Multiline mode. Type \"\"\" on its own line to finish.".dimmed());
        let mut lines = Vec::new();
        loop {
            print!("{} ", "..".dimmed());
            io::stdout().flush().ok();
            let next = read_line_async(shutdown).await?;
            if next.trim() == "\"\"\"" {
                break;
            }
            lines.push(next);
        }
        Some(lines.join(""))
    } else if line.trim().starts_with("\"\"\"") {
        let first = line.trim().trim_start_matches("\"\"\"");
        let mut lines = vec![first.to_string(), "\n".to_string()];
        loop {
            print!("{} ", "..".dimmed());
            io::stdout().flush().ok();
            let next = read_line_async(shutdown).await?;
            if next.trim() == "\"\"\"" {
                break;
            }
            lines.push(next);
        }
        Some(lines.join(""))
    } else {
        Some(line)
    }
}

async fn setup_ctrlc_handler(runtime: &Runtime) -> Arc<Notify> {
    let notify = Arc::new(Notify::new());
    let notify_clone = notify.clone();
    let store = runtime.store.clone();
    let project = runtime.project.clone();

    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("\n{}", "Caught Ctrl+C, closing conversation...".dimmed());
            let proj = project.as_deref().unwrap_or("default");
            let _ = store
                .close_current_conversation("user", proj, "Session interrupted by Ctrl+C.")
                .await;
            eprintln!("{}", "Bye.".dimmed());
            notify_clone.notify_one();
        }
    });

    notify
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
