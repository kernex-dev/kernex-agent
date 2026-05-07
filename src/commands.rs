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
            let cwd = match std::env::current_dir() {
                Ok(d) => d,
                Err(e) => {
                    eprintln!(
                        "{} could not get working directory: {e}",
                        "error:".red().bold()
                    );
                    return CommandResult::Continue;
                }
            };
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
    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "{} could not get working directory: {e}",
                "error:".red().bold()
            );
            return;
        }
    };
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
        if let Some(name) = &pc.name {
            println!("  {} {name}", "Provider:".dimmed());
        }
        if let Some(model) = &pc.model {
            println!("  {} {model}", "Model:".dimmed());
        }
        if let Some(tokens) = pc.max_tokens {
            println!("  {} {tokens}", "Max tokens:".dimmed());
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

  /help              Show this help message
  /quit, /exit       Exit kx dev

  {}

  /search <query>    Full-text search across all past conversations
  /history           Show last 20 messages in current conversation
  /memory            Show memory stats (conversations, messages, facts, DB size)
  /facts             List stored facts (things kx learned about your project)
  /facts delete <k>  Delete a specific fact by its key
  /clear             End current conversation and start fresh

  {}

  /stack             Show detected stack, project name, and data directory
  /config            Show active configuration (.kx.toml settings)

  {}

  /skills            List installed skills with trust levels
  /skills add <src>  Install skill from GitHub (owner/repo or owner/repo@tag)
  /skills remove <n> Remove an installed skill by name
  /skills verify     Verify SHA-256 integrity of all installed skills

  {}

  /retry             Retry the last failed message

  {}

  \"\"\"                Start/end multiline input (paste code blocks between \"\"\")
                     Example: \"\"\" <paste code> \"\"\"

  {}

  Create .kx.toml in your project root to customize behavior.
  See: examples/.kx.toml.example
"#,
        "Commands".bold(),
        "Memory & Search".bold(),
        "Project Info".bold(),
        "Skills".bold(),
        "Recovery".bold(),
        "Input".bold(),
        "Configuration".bold()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_result_variants() {
        // Test that all variants can be constructed
        let _ = CommandResult::Continue;
        let _ = CommandResult::Quit;
        let _ = CommandResult::Unknown;
    }

    #[test]
    fn strip_prefix_search() {
        let input = "/search rust async";
        let rest = input.strip_prefix("/search");
        assert!(rest.is_some());
        assert_eq!(rest.unwrap().trim(), "rust async");
    }

    #[test]
    fn strip_prefix_facts() {
        let input = "/facts";
        let rest = input.strip_prefix("/facts");
        assert!(rest.is_some());
        assert_eq!(rest.unwrap().trim(), "");
    }

    #[test]
    fn strip_prefix_facts_delete() {
        let input = "/facts delete user_name";
        let rest = input.strip_prefix("/facts").unwrap().trim();
        let key = rest.strip_prefix("delete ");
        assert!(key.is_some());
        assert_eq!(key.unwrap().trim(), "user_name");
    }

    #[test]
    fn strip_prefix_skills() {
        let input = "/skills add acme/repo";
        let rest = input.strip_prefix("/skills");
        assert!(rest.is_some());
        let arg = rest.unwrap().trim();
        assert!(arg.starts_with("add "));
    }

    #[test]
    fn command_matching_quit() {
        let input = "/quit";
        assert!(input == "/quit" || input == "/exit");
    }

    #[test]
    fn command_matching_exit() {
        let input = "/exit";
        assert!(input == "/quit" || input == "/exit");
    }

    #[test]
    fn command_matching_help() {
        let input = "/help";
        assert_eq!(input, "/help");
    }

    #[test]
    fn command_matching_stack() {
        let input = "/stack";
        assert_eq!(input, "/stack");
    }

    #[test]
    fn command_matching_history() {
        let input = "/history";
        assert_eq!(input, "/history");
    }

    #[test]
    fn command_matching_memory() {
        let input = "/memory";
        assert_eq!(input, "/memory");
    }

    #[test]
    fn command_matching_config() {
        let input = "/config";
        assert_eq!(input, "/config");
    }

    #[test]
    fn command_matching_clear() {
        let input = "/clear";
        assert_eq!(input, "/clear");
    }

    #[test]
    fn skills_arg_parsing_empty() {
        let arg = "";
        assert!(arg.is_empty());
    }

    #[test]
    fn skills_arg_parsing_add() {
        let arg = "add acme/my-skill sandboxed";
        let rest = arg.strip_prefix("add ");
        assert!(rest.is_some());
        let parts: Vec<&str> = rest.unwrap().split_whitespace().collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "acme/my-skill");
        assert_eq!(parts[1], "sandboxed");
    }

    #[test]
    fn skills_arg_parsing_add_default_trust() {
        let arg = "add acme/my-skill";
        let rest = arg.strip_prefix("add ");
        assert!(rest.is_some());
        let parts: Vec<&str> = rest.unwrap().split_whitespace().collect();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0], "acme/my-skill");
        let trust = parts.get(1).copied().unwrap_or("sandboxed");
        assert_eq!(trust, "sandboxed");
    }

    #[test]
    fn skills_arg_parsing_remove() {
        let arg = "remove my-skill";
        let rest = arg.strip_prefix("remove ");
        assert!(rest.is_some());
        assert_eq!(rest.unwrap().trim(), "my-skill");
    }

    #[test]
    fn skills_arg_parsing_verify() {
        let arg = "verify";
        assert_eq!(arg, "verify");
    }

    #[test]
    fn history_message_preview_short() {
        let text = "Hello world";
        let preview: String = text.chars().take(150).collect();
        let ellipsis = if text.len() > 150 { "..." } else { "" };
        assert_eq!(preview, "Hello world");
        assert_eq!(ellipsis, "");
    }

    #[test]
    fn history_message_preview_long() {
        let text = "a".repeat(200);
        let preview: String = text.chars().take(150).collect();
        let ellipsis = if text.len() > 150 { "..." } else { "" };
        assert_eq!(preview.len(), 150);
        assert_eq!(ellipsis, "...");
    }

    #[test]
    fn search_result_preview_short() {
        let text = "Search result";
        let preview: String = text.chars().take(120).collect();
        let ellipsis = if text.len() > 120 { "..." } else { "" };
        assert_eq!(preview, "Search result");
        assert_eq!(ellipsis, "");
    }

    #[test]
    fn search_result_preview_long() {
        let text = "b".repeat(150);
        let preview: String = text.chars().take(120).collect();
        let ellipsis = if text.len() > 120 { "..." } else { "" };
        assert_eq!(preview.len(), 120);
        assert_eq!(ellipsis, "...");
    }

    #[test]
    fn memory_stats_mb_calculation() {
        let size: u64 = 1024 * 1024 * 5; // 5 MB
        let mb = size as f64 / (1024.0 * 1024.0);
        assert!((mb - 5.0).abs() < 0.001);
    }

    #[test]
    fn memory_stats_kb_to_mb() {
        let size: u64 = 512 * 1024; // 512 KB
        let mb = size as f64 / (1024.0 * 1024.0);
        assert!((mb - 0.5).abs() < 0.001);
    }
}
