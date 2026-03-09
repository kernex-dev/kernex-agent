use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "kx", version, about = "CLI dev assistant powered by Kernex")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// AI provider to use (claude-code, ollama, openai, anthropic, gemini, openrouter)
    #[arg(long, global = true, default_value = "claude-code")]
    pub provider: String,

    /// Model override (provider-specific, e.g. gpt-4o, llama3.2)
    #[arg(long, global = true)]
    pub model: Option<String>,

    /// API key for providers that require one
    #[arg(long, global = true)]
    pub api_key: Option<String>,

    /// Base URL override (e.g. http://localhost:11434 for Ollama)
    #[arg(long, global = true)]
    pub base_url: Option<String>,

    /// One-shot message when no subcommand is given (kx "fix the bug")
    pub message: Option<String>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Interactive coding assistant with persistent memory
    Dev {
        /// One-shot message (skip interactive loop)
        message: Option<String>,
    },
    /// Repository health audit (deps, tests, docs, structure)
    Audit,
    /// Documentation audit (detect outdated docs, archive)
    Docs,
    /// Initialize kx for current project (installs builtin skills)
    Init,
    /// Run a multi-agent pipeline
    Pipeline {
        #[command(subcommand)]
        action: PipelineAction,
    },
    /// Manage installed skills
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
}

#[derive(Subcommand)]
pub enum PipelineAction {
    /// Run a named pipeline/topology
    Run {
        /// Pipeline name (matches topology directory name)
        name: String,
    },
    /// List available pipelines
    List,
}

#[derive(Subcommand)]
pub enum SkillsAction {
    /// List installed skills
    List,
    /// Add a skill from GitHub (owner/repo or owner/repo/path)
    Add {
        /// Skill source (e.g., acme/my-skill or acme/repo/skills/rust)
        source: String,
        /// Trust level to assign (sandboxed, standard, trusted)
        #[arg(short, long, default_value = "sandboxed")]
        trust: String,
    },
    /// Remove an installed skill
    Remove {
        /// Name of the skill to remove
        name: String,
    },
    /// Verify integrity of installed skills
    Verify,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_parses_no_args() {
        let cli = Cli::try_parse_from(["kx"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert!(cli.command.is_none());
        assert!(cli.message.is_none());
        assert_eq!(cli.provider, "claude-code");
    }

    #[test]
    fn cli_parses_oneshot_message() {
        let cli = Cli::try_parse_from(["kx", "fix the bug"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.message, Some("fix the bug".to_string()));
    }

    #[test]
    fn cli_parses_dev_subcommand() {
        let cli = Cli::try_parse_from(["kx", "dev"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert!(matches!(cli.command, Some(Command::Dev { message: None })));
    }

    #[test]
    fn cli_parses_dev_with_message() {
        let cli = Cli::try_parse_from(["kx", "dev", "write tests"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Dev { message }) = cli.command {
            assert_eq!(message, Some("write tests".to_string()));
        } else {
            panic!("Expected Dev command");
        }
    }

    #[test]
    fn cli_parses_audit() {
        let cli = Cli::try_parse_from(["kx", "audit"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert!(matches!(cli.command, Some(Command::Audit)));
    }

    #[test]
    fn cli_parses_docs() {
        let cli = Cli::try_parse_from(["kx", "docs"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert!(matches!(cli.command, Some(Command::Docs)));
    }

    #[test]
    fn cli_parses_init() {
        let cli = Cli::try_parse_from(["kx", "init"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert!(matches!(cli.command, Some(Command::Init)));
    }

    #[test]
    fn cli_parses_pipeline_run() {
        let cli = Cli::try_parse_from(["kx", "pipeline", "run", "code-review"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Pipeline { action }) = cli.command {
            if let PipelineAction::Run { name } = action {
                assert_eq!(name, "code-review");
            } else {
                panic!("Expected Run action");
            }
        } else {
            panic!("Expected Pipeline command");
        }
    }

    #[test]
    fn cli_parses_pipeline_list() {
        let cli = Cli::try_parse_from(["kx", "pipeline", "list"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Pipeline { action }) = cli.command {
            assert!(matches!(action, PipelineAction::List));
        } else {
            panic!("Expected Pipeline command");
        }
    }

    #[test]
    fn cli_parses_provider_flag() {
        let cli = Cli::try_parse_from(["kx", "--provider", "ollama", "dev"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert_eq!(cli.provider, "ollama");
    }

    #[test]
    fn cli_parses_model_flag() {
        let cli = Cli::try_parse_from(["kx", "--model", "gpt-4o", "dev"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert_eq!(cli.model, Some("gpt-4o".to_string()));
    }

    #[test]
    fn cli_parses_api_key_flag() {
        let cli = Cli::try_parse_from(["kx", "--api-key", "sk-test", "dev"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert_eq!(cli.api_key, Some("sk-test".to_string()));
    }

    #[test]
    fn cli_parses_base_url_flag() {
        let cli = Cli::try_parse_from(["kx", "--base-url", "http://localhost:11434", "dev"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        assert_eq!(cli.base_url, Some("http://localhost:11434".to_string()));
    }

    #[test]
    fn cli_provider_default_is_claude_code() {
        let cli = Cli::try_parse_from(["kx", "dev"]);
        assert!(cli.is_ok());
        assert_eq!(cli.unwrap().provider, "claude-code");
    }

    #[test]
    fn cli_parses_skills_list() {
        let cli = Cli::try_parse_from(["kx", "skills", "list"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Skills { action }) = cli.command {
            assert!(matches!(action, SkillsAction::List));
        } else {
            panic!("Expected Skills command");
        }
    }

    #[test]
    fn cli_parses_skills_add() {
        let cli = Cli::try_parse_from(["kx", "skills", "add", "acme/repo"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Skills { action }) = cli.command {
            if let SkillsAction::Add { source, trust } = action {
                assert_eq!(source, "acme/repo");
                assert_eq!(trust, "sandboxed");
            } else {
                panic!("Expected Add action");
            }
        } else {
            panic!("Expected Skills command");
        }
    }

    #[test]
    fn cli_parses_skills_add_with_trust() {
        let cli = Cli::try_parse_from(["kx", "skills", "add", "acme/repo", "-t", "trusted"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Skills { action }) = cli.command {
            if let SkillsAction::Add { source, trust } = action {
                assert_eq!(source, "acme/repo");
                assert_eq!(trust, "trusted");
            } else {
                panic!("Expected Add action");
            }
        } else {
            panic!("Expected Skills command");
        }
    }

    #[test]
    fn cli_parses_skills_remove() {
        let cli = Cli::try_parse_from(["kx", "skills", "remove", "my-skill"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Skills { action }) = cli.command {
            if let SkillsAction::Remove { name } = action {
                assert_eq!(name, "my-skill");
            } else {
                panic!("Expected Remove action");
            }
        } else {
            panic!("Expected Skills command");
        }
    }

    #[test]
    fn cli_parses_skills_verify() {
        let cli = Cli::try_parse_from(["kx", "skills", "verify"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Skills { action }) = cli.command {
            assert!(matches!(action, SkillsAction::Verify));
        } else {
            panic!("Expected Skills command");
        }
    }

    #[test]
    fn cli_has_valid_structure() {
        // Verifies the CLI definition doesn't have conflicts
        Cli::command().debug_assert();
    }

    #[test]
    fn cli_version_flag() {
        let result = Cli::try_parse_from(["kx", "--version"]);
        // --version causes early exit, so it's an error from clap's perspective
        assert!(result.is_err());
    }

    #[test]
    fn cli_help_flag() {
        let result = Cli::try_parse_from(["kx", "--help"]);
        // --help causes early exit
        assert!(result.is_err());
    }
}
