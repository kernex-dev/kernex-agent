use colored::Colorize;
use kernex_runtime::Runtime;

use crate::stack::{self, Stack};

pub enum CommandResult {
    Continue,
    Quit,
    Unknown,
}

pub async fn handle(input: &str, runtime: &Runtime, detected_stack: Stack) -> CommandResult {
    match input {
        "/quit" | "/exit" => CommandResult::Quit,
        "/help" => {
            print_help();
            CommandResult::Continue
        }
        "/stack" => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let name = stack::project_name(&cwd);
            println!(
                "\n  {} {}\n  {} {}\n  {} {}\n",
                "Project:".dimmed(),
                name.bold(),
                "Stack:".dimmed(),
                detected_stack,
                "Data:".dimmed(),
                runtime.data_dir,
            );
            CommandResult::Continue
        }
        "/memory" => {
            print_memory_stats(runtime).await;
            CommandResult::Continue
        }
        "/clear" => {
            close_conversation(runtime, "Conversation cleared by user.").await;
            println!("{}", "Conversation cleared.\n".dimmed());
            CommandResult::Continue
        }
        _ => CommandResult::Unknown,
    }
}

pub async fn close_conversation(runtime: &Runtime, summary: &str) {
    let project = runtime.project.as_deref().unwrap_or("default");
    if let Err(e) = runtime
        .store
        .close_current_conversation("user", project, summary)
        .await
    {
        if !e.to_string().contains("no active") {
            eprintln!("{} closing conversation: {e}", "warn:".yellow().bold());
        }
    }
}

async fn print_memory_stats(runtime: &Runtime) {
    match runtime.store.get_memory_stats("user").await {
        Ok((conversations, messages, facts)) => {
            println!("\n{}", "  Memory stats".bold());
            println!("  {} {conversations}", "Conversations:".dimmed());
            println!("  {} {messages}", "Messages:".dimmed());
            println!("  {} {facts}\n", "Facts:".dimmed());
        }
        Err(e) => {
            eprintln!("{} fetching memory stats: {e}\n", "error:".red().bold());
        }
    }

    match runtime.store.db_size().await {
        Ok(size) => {
            let mb = size as f64 / (1024.0 * 1024.0);
            println!("  {} {:.2} MB\n", "DB size:".dimmed(), mb);
        }
        Err(e) => {
            eprintln!("{} fetching db size: {e}\n", "error:".red().bold());
        }
    }
}

fn print_help() {
    println!(
        r#"
  {}
  /help     Show this help
  /stack    Show detected stack and project info
  /memory   Show memory stats and DB size
  /clear    Close current conversation
  /quit     Exit kx dev
"#,
        "Commands".bold()
    );
}
