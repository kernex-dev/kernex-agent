mod cli;
mod prompts;
mod stack;

use std::io::{self, Write};
use std::path::PathBuf;

use clap::Parser;
use colored::Colorize;
use kernex_core::message::Request;
use kernex_providers::claude_code::ClaudeCodeProvider;
use kernex_runtime::RuntimeBuilder;

use crate::cli::{Cli, Command};

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
        Command::Dev => cmd_dev().await,
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

async fn cmd_dev() -> Result<(), Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir()?;
    let detected_stack = stack::detect(&cwd);
    let project_name = stack::project_name(&cwd);

    let data_dir = data_dir_for(&project_name);
    let system_prompt = prompts::dev_system_prompt(detected_stack, &project_name);

    println!(
        "{} {} ({})",
        "kx dev".green().bold(),
        project_name.bold(),
        detected_stack
    );
    println!("{}", "Type /quit to exit.\n".dimmed());

    let provider = ClaudeCodeProvider::new();

    let runtime = RuntimeBuilder::new()
        .data_dir(data_dir.to_str().unwrap_or("~/.kx"))
        .system_prompt(&system_prompt)
        .channel("cli")
        .project(&project_name)
        .build()
        .await?;

    loop {
        print!("{} ", ">".cyan().bold());
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input == "/quit" || input == "/exit" {
            println!("{}", "Bye.".dimmed());
            break;
        }

        let request = Request::text("user", input);
        match runtime.complete(&provider, &request).await {
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

fn data_dir_for(project_name: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".kx")
        .join("projects")
        .join(project_name)
}
