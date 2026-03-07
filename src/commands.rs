use std::path::PathBuf;

use colored::Colorize;
use kernex_runtime::Runtime;

use crate::config::ProjectConfig;
use crate::skills;
use crate::stack::{self, Stack};

pub enum CommandResult {
    Continue,
    Quit,
    Unknown,
}

pub async fn handle(
    input: &str,
    runtime: &Runtime,
    detected_stack: Stack,
    project_config: &ProjectConfig,
) -> CommandResult {
    if let Some(rest) = input.strip_prefix("/search") {
        let query = rest.trim();
        if query.is_empty() {
            eprintln!("{} Usage: /search <query>\n", "warn:".yellow().bold());
        } else {
            search_memory(runtime, query).await;
        }
        return CommandResult::Continue;
    }

    if let Some(rest) = input.strip_prefix("/facts") {
        let arg = rest.trim();
        if arg.is_empty() {
            print_facts(runtime).await;
        } else if let Some(key) = arg.strip_prefix("delete ") {
            delete_fact(runtime, key.trim()).await;
        } else {
            eprintln!(
                "{} Usage: /facts or /facts delete <key>\n",
                "warn:".yellow().bold()
            );
        }
        return CommandResult::Continue;
    }

    if let Some(rest) = input.strip_prefix("/skills") {
        let arg = rest.trim();
        handle_skills_command(arg).await;
        return CommandResult::Continue;
    }

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
        "/history" => {
            print_history(runtime).await;
            CommandResult::Continue
        }
        "/memory" => {
            print_memory_stats(runtime).await;
            CommandResult::Continue
        }
        "/config" => {
            print_config(runtime, detected_stack, project_config);
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

async fn print_facts(runtime: &Runtime) {
    match runtime.store.get_facts("user").await {
        Ok(facts) if facts.is_empty() => {
            println!("{}", "  No facts stored.\n".dimmed());
        }
        Ok(facts) => {
            println!("\n{}", "  Stored facts".bold());
            for (key, value) in &facts {
                println!("  {} {}", format!("{key}:").dimmed(), value);
            }
            println!();
        }
        Err(e) => {
            eprintln!("{} fetching facts: {e}\n", "error:".red().bold());
        }
    }
}

async fn delete_fact(runtime: &Runtime, key: &str) {
    match runtime.store.delete_fact("user", key).await {
        Ok(true) => println!("{}", format!("  Deleted fact: {key}\n").dimmed()),
        Ok(false) => println!("{}", format!("  Fact not found: {key}\n").yellow()),
        Err(e) => eprintln!("{} deleting fact: {e}\n", "error:".red().bold()),
    }
}

async fn print_history(runtime: &Runtime) {
    let channel = &runtime.channel;
    match runtime.store.get_history(channel, "user", 20).await {
        Ok(messages) if messages.is_empty() => {
            println!("{}", "  No history in current session.\n".dimmed());
        }
        Ok(messages) => {
            println!("\n  {}\n", "Conversation history (last 20)".bold());
            for (role, text) in &messages {
                let label = if role == "user" {
                    "you:".cyan()
                } else {
                    "kx:".green()
                };
                let preview: String = text.chars().take(150).collect();
                let ellipsis = if text.len() > 150 { "..." } else { "" };
                println!("  {label} {preview}{ellipsis}");
            }
            println!();
        }
        Err(e) => {
            eprintln!("{} fetching history: {e}\n", "error:".red().bold());
        }
    }
}

fn print_config(runtime: &Runtime, detected_stack: Stack, config: &ProjectConfig) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let has_config = cwd.join(".kx.toml").exists();

    println!("\n  {}", "Active configuration".bold());
    println!("  {} {}", "Project:".dimmed(), stack::project_name(&cwd));
    println!("  {} {detected_stack}", "Stack:".dimmed());
    println!("  {} {}", "Data dir:".dimmed(), runtime.data_dir);
    println!("  {} {}", "Channel:".dimmed(), runtime.channel);
    println!(
        "  {} {}",
        ".kx.toml:".dimmed(),
        if has_config { "found" } else { "not found" }
    );

    if let Some(override_stack) = &config.stack {
        println!("  {} {override_stack}", "Stack override:".dimmed());
    }
    if config.system_prompt.is_some() {
        println!("  {} yes", "Custom prompt:".dimmed());
    }
    if let Some(pc) = &config.provider {
        if let Some(model) = &pc.model {
            println!("  {} {model}", "Model:".dimmed());
        }
        if let Some(turns) = pc.max_turns {
            println!("  {} {turns}", "Max turns:".dimmed());
        }
        if let Some(timeout) = pc.timeout_secs {
            println!("  {} {timeout}s", "Timeout:".dimmed());
        }
    }

    println!();
}

async fn search_memory(runtime: &Runtime, query: &str) {
    match runtime.store.search_messages(query, "", "user", 10).await {
        Ok(results) if results.is_empty() => {
            println!("{}", "  No results found.\n".dimmed());
        }
        Ok(results) => {
            println!("\n  {} \"{query}\"\n", "Search results for".bold());
            for (role, text, _conv_id) in &results {
                let label = if role == "user" {
                    "you:".cyan()
                } else {
                    "kx:".green()
                };
                let preview: String = text.chars().take(120).collect();
                let ellipsis = if text.len() > 120 { "..." } else { "" };
                println!("  {label} {preview}{ellipsis}");
            }
            println!();
        }
        Err(e) => {
            eprintln!("{} searching memory: {e}\n", "error:".red().bold());
        }
    }
}

fn print_help() {
    println!(
        r#"
  {}
  /help     Show this help
  /search <query>  Search past conversations (FTS5)
  /history  Show recent conversation history
  /stack    Show detected stack and project info
  /memory   Show memory stats and DB size
  /config   Show active configuration
  /facts    List stored facts
  /facts delete <key>  Delete a specific fact
  /skills   List installed skills
  /skills add <source>  Install a skill (owner/repo)
  /skills remove <name>  Remove a skill
  /skills verify  Verify skill integrity
  /retry    Retry last failed message
  /clear    Close current conversation
  /quit     Exit kx dev

  {}
  \"\"\"       Start/end multiline input (for pasting code blocks)
"#,
        "Commands".bold(),
        "Input".bold()
    );
}

async fn handle_skills_command(arg: &str) {
    let data_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".kx");

    if arg.is_empty() {
        skills::cli_handler::list_skills(&data_dir).await;
        return;
    }

    if let Some(rest) = arg.strip_prefix("add ") {
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.is_empty() {
            eprintln!(
                "{} Usage: /skills add <owner/repo>\n",
                "warn:".yellow().bold()
            );
            return;
        }
        let source = parts[0];
        let trust = parts.get(1).copied().unwrap_or("sandboxed");

        let policy = crate::skills::permissions::PermissionPolicy::default();
        match skills::cli_handler::add_skill(&data_dir, source, trust, &policy).await {
            Ok(()) => {}
            Err(e) => eprintln!("{} {e}\n", "error:".red().bold()),
        }
        return;
    }

    if let Some(rest) = arg.strip_prefix("remove ") {
        let name = rest.trim();
        if name.is_empty() {
            eprintln!("{} Usage: /skills remove <name>\n", "warn:".yellow().bold());
            return;
        }

        match skills::cli_handler::remove_skill(&data_dir, name).await {
            Ok(()) => {}
            Err(e) => eprintln!("{} {e}\n", "error:".red().bold()),
        }
        return;
    }

    if arg == "verify" {
        skills::cli_handler::verify_skills(&data_dir).await;
        return;
    }

    eprintln!(
        "{} Unknown skills command. Use: /skills, /skills add <source>, /skills remove <name>, /skills verify\n",
        "warn:".yellow().bold()
    );
}
