use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "kx", version, about = "CLI dev assistant powered by Kernex")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// AI provider to use (claude-code, anthropic, openai, ollama, gemini, openrouter, groq, mistral, deepseek, fireworks, xai)
    #[arg(
        short = 'p',
        long,
        global = true,
        default_value = "claude-code",
        env = "KERNEX_PROVIDER"
    )]
    pub provider: String,

    /// Model override (provider-specific, e.g. gpt-4o, llama3.2)
    #[arg(short = 'm', long, global = true, env = "KERNEX_MODEL")]
    pub model: Option<String>,

    /// API key for providers that require one
    #[arg(long, global = true, env = "KERNEX_API_KEY")]
    pub api_key: Option<String>,

    /// Base URL override (e.g. http://localhost:11434 for Ollama)
    #[arg(long, global = true, env = "KERNEX_BASE_URL")]
    pub base_url: Option<String>,

    /// Override the active project scope (default: current directory name)
    #[arg(long, global = true)]
    pub project: Option<String>,

    /// Channel identifier for memory scoping (default: cli)
    #[arg(long, global = true)]
    pub channel: Option<String>,

    /// Max response tokens per provider request (default: provider-specific)
    #[arg(long, global = true)]
    pub max_tokens: Option<u32>,

    /// Disable persistent memory for this session
    #[arg(long, global = true)]
    pub no_memory: bool,

    /// Print tool calls and hook events to stderr
    #[arg(long, global = true)]
    pub verbose: bool,

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
    /// Diagnose the local install: provider availability, env vars, data dir
    Doctor,
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
    /// Manage scheduled tasks (cron-style self-scheduling)
    Cron {
        #[command(subcommand)]
        action: CronAction,
    },
    /// Run kx as a headless HTTP server for remote agent execution
    Serve {
        /// Host address to bind
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to listen on
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Bearer auth token (or set KERNEX_AUTH_TOKEN env var)
        #[arg(long, env = "KERNEX_AUTH_TOKEN")]
        auth_token: Option<String>,
        /// Max concurrent agent jobs
        #[arg(long, default_value_t = 4)]
        workers: usize,
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
    /// Validate a skill directory against the Agent Skills spec (content, format, anti-patterns)
    Lint {
        /// Path to skill directory to validate (defaults to current directory)
        #[arg(default_value = ".")]
        path: String,
    },
}

#[derive(Subcommand)]
pub enum CronAction {
    /// Schedule a new task for autonomous execution
    Create {
        /// What the agent should do when the task runs
        description: String,
        /// When to run (ISO 8601 datetime, e.g. "2026-04-03T09:00:00")
        #[arg(long)]
        at: String,
        /// Repeat interval: daily, weekly, monthly, weekdays
        #[arg(long)]
        repeat: Option<String>,
    },
    /// List all pending scheduled tasks
    List,
    /// Cancel a scheduled task by ID prefix
    Delete {
        /// Task ID prefix (first 8+ characters shown by cron list)
        id: String,
    },
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
    fn cli_parses_cron_list() {
        let cli = Cli::try_parse_from(["kx", "cron", "list"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Cron { action }) = cli.command {
            assert!(matches!(action, CronAction::List));
        } else {
            panic!("Expected Cron command");
        }
    }

    #[test]
    fn cli_parses_cron_create() {
        let cli = Cli::try_parse_from([
            "kx",
            "cron",
            "create",
            "run the test suite",
            "--at",
            "2026-04-03T09:00:00",
        ]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Cron { action }) = cli.command {
            if let CronAction::Create {
                description,
                at,
                repeat,
            } = action
            {
                assert_eq!(description, "run the test suite");
                assert_eq!(at, "2026-04-03T09:00:00");
                assert!(repeat.is_none());
            } else {
                panic!("Expected Create action");
            }
        } else {
            panic!("Expected Cron command");
        }
    }

    #[test]
    fn cli_parses_cron_create_with_repeat() {
        let cli = Cli::try_parse_from([
            "kx",
            "cron",
            "create",
            "run lints",
            "--at",
            "2026-04-03T08:00:00",
            "--repeat",
            "daily",
        ]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Cron { action }) = cli.command {
            if let CronAction::Create { repeat, .. } = action {
                assert_eq!(repeat, Some("daily".to_string()));
            } else {
                panic!("Expected Create action");
            }
        } else {
            panic!("Expected Cron command");
        }
    }

    #[test]
    fn cli_parses_cron_delete() {
        let cli = Cli::try_parse_from(["kx", "cron", "delete", "abc12345"]);
        assert!(cli.is_ok());
        let cli = cli.unwrap();
        if let Some(Command::Cron { action }) = cli.command {
            if let CronAction::Delete { id } = action {
                assert_eq!(id, "abc12345");
            } else {
                panic!("Expected Delete action");
            }
        } else {
            panic!("Expected Cron command");
        }
    }

    #[test]
    fn cli_parses_serve_defaults() {
        let cli = Cli::try_parse_from(["kx", "serve", "--auth-token", "secret"]).unwrap();
        if let Some(Command::Serve {
            host,
            port,
            auth_token,
            workers,
        }) = cli.command
        {
            assert_eq!(host, "127.0.0.1");
            assert_eq!(port, 8080);
            assert_eq!(auth_token, Some("secret".to_string()));
            assert_eq!(workers, 4);
        } else {
            panic!("Expected Serve command");
        }
    }

    #[test]
    fn cli_parses_serve_custom_host_port() {
        let cli = Cli::try_parse_from([
            "kx",
            "serve",
            "--host",
            "0.0.0.0",
            "--port",
            "9000",
            "--workers",
            "8",
        ])
        .unwrap();
        if let Some(Command::Serve {
            host,
            port,
            workers,
            ..
        }) = cli.command
        {
            assert_eq!(host, "0.0.0.0");
            assert_eq!(port, 9000);
            assert_eq!(workers, 8);
        } else {
            panic!("Expected Serve command");
        }
    }

    #[test]
    fn cli_parses_serve_no_auth_token() {
        let cli = Cli::try_parse_from(["kx", "serve"]).unwrap();
        if let Some(Command::Serve { auth_token, .. }) = cli.command {
            assert!(auth_token.is_none());
        } else {
            panic!("Expected Serve command");
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

    #[test]
    fn cli_parses_provider_short_flag() {
        let cli = Cli::try_parse_from(["kx", "-p", "ollama", "dev"]).unwrap();
        assert_eq!(cli.provider, "ollama");
    }

    #[test]
    fn cli_parses_model_short_flag() {
        let cli = Cli::try_parse_from(["kx", "-m", "llama3.2", "dev"]).unwrap();
        assert_eq!(cli.model, Some("llama3.2".to_string()));
    }

    #[test]
    fn cli_parses_project_flag() {
        let cli = Cli::try_parse_from(["kx", "--project", "my-app", "dev"]).unwrap();
        assert_eq!(cli.project, Some("my-app".to_string()));
    }

    #[test]
    fn cli_parses_channel_flag() {
        let cli = Cli::try_parse_from(["kx", "--channel", "ci", "dev"]).unwrap();
        assert_eq!(cli.channel, Some("ci".to_string()));
    }

    #[test]
    fn cli_parses_max_tokens_flag() {
        let cli = Cli::try_parse_from(["kx", "--max-tokens", "2048", "dev"]).unwrap();
        assert_eq!(cli.max_tokens, Some(2048));
    }

    #[test]
    fn cli_parses_no_memory_flag() {
        let cli = Cli::try_parse_from(["kx", "--no-memory", "dev"]).unwrap();
        assert!(cli.no_memory);
    }

    #[test]
    fn cli_parses_verbose_flag() {
        let cli = Cli::try_parse_from(["kx", "--verbose", "dev"]).unwrap();
        assert!(cli.verbose);
    }

    #[test]
    fn cli_flags_default_false() {
        let cli = Cli::try_parse_from(["kx", "dev"]).unwrap();
        assert!(!cli.no_memory);
        assert!(!cli.verbose);
        assert!(cli.project.is_none());
        assert!(cli.channel.is_none());
        assert!(cli.max_tokens.is_none());
    }
}
