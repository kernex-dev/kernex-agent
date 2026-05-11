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

    /// Disable automatic conversation summarization on overflow.
    /// kx defaults to summarizing the oldest messages when a session
    /// exceeds the runtime's context cap; pass --no-auto-compact to
    /// fall back to the kernex-runtime default behavior (silent drop)
    /// for cost reasons or when running against a provider where the
    /// extra summarization round-trip is undesirable.
    #[arg(long, global = true)]
    pub no_auto_compact: bool,

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
    #[cfg(feature = "serve")]
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
    /// Read, write, and search the local memory store from outside the REPL
    #[cfg(feature = "memory-cli")]
    Mem {
        #[command(subcommand)]
        action: MemAction,
        /// Force JSON output even on a TTY
        #[arg(long, global = true)]
        json: bool,
        /// Project only `id`, `type`, `title`, `updated_at`, `score`
        #[arg(long, global = true)]
        compact: bool,
        /// Project arbitrary fields (comma-separated)
        #[arg(long, global = true, value_delimiter = ',')]
        select: Vec<String>,
    },
}

// Per CC-8, every `kx mem *` subcommand's `--help` output includes
// Examples, an Exit codes section, and a `Try:` line. Clap's derive
// macros require each `after_help` value to be a compile-time string
// literal, so the shared exit-codes block is inlined in every variant
// below rather than referenced through a const (concat! and the derive
// parser both reject non-literal args). A small `cli_help_contract`
// test below asserts every variant carries the required sections so
// future variants cannot drift away from the contract silently.

#[cfg(feature = "memory-cli")]
#[derive(Subcommand)]
pub enum MemAction {
    /// Full-text search across observations and messages
    #[command(after_help = "Examples:
  kx mem search \"N+1 query\" --limit 5
  kx mem search auth --type bugfix --since 30d
  kx mem search hello --json | jq '.[].title'

Exit codes:
  0  Success
  2  Usage error (unknown flag, malformed argument)
  3  Not found (id, key, or required record absent)
  4  Authorization or sandbox refusal
  5  Runtime (DB locked, IO failure, schema mismatch)
  7  Rate / capacity (reserved for future provider-backed commands)

Try: kx mem search \"hello\" --limit 3 --json
")]
    Search {
        /// Query string (FTS5 syntax)
        query: String,
        /// Max results to return (default 10)
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Filter to records within a recency window (e.g., 30d, 12h, 90m)
        #[arg(long)]
        since: Option<String>,
        /// Filter by observation type
        #[arg(long)]
        r#type: Option<String>,
    },
    /// Fetch a single observation by id
    #[command(after_help = "Examples:
  kx mem get 42
  kx mem get 42 --json

Exit codes:
  0  Success
  2  Usage error (unknown flag, malformed argument)
  3  Not found (id, key, or required record absent)
  4  Authorization or sandbox refusal
  5  Runtime (DB locked, IO failure, schema mismatch)
  7  Rate / capacity (reserved for future provider-backed commands)

Try: kx mem search hello | jq '.[0].id' # find an id, then kx mem get <id>
")]
    Get {
        /// Message UUID (returned by `kx mem search`)
        id: String,
    },
    /// Recent observations for a project, newest first
    #[command(after_help = "Examples:
  kx mem history
  kx mem history --last 5
  kx --project my-project mem history

Exit codes:
  0  Success
  2  Usage error (unknown flag, malformed argument)
  3  Not found (id, key, or required record absent)
  4  Authorization or sandbox refusal
  5  Runtime (DB locked, IO failure, schema mismatch)
  7  Rate / capacity (reserved for future provider-backed commands)

Try: kx mem history --last 5 --json | jq '.[].title'
")]
    History {
        /// Override the default count (default 20)
        #[arg(long)]
        last: Option<usize>,
    },
    /// Counts and last-write timestamp for a project
    #[command(after_help = "Examples:
  kx mem stats
  kx mem stats --json

Exit codes:
  0  Success
  2  Usage error (unknown flag, malformed argument)
  3  Not found (id, key, or required record absent)
  4  Authorization or sandbox refusal
  5  Runtime (DB locked, IO failure, schema mismatch)
  7  Rate / capacity (reserved for future provider-backed commands)

Try: kx mem stats --json | jq '.observations'
")]
    Stats {},
    /// Read or write the project-scoped facts table
    #[command(after_help = "Examples:
  kx mem facts list
  kx mem facts add auth-pattern \"OIDC + PKCE\"
  kx mem facts get auth-pattern
  kx mem facts delete auth-pattern

Exit codes:
  0  Success
  2  Usage error (unknown flag, malformed argument)
  3  Not found (id, key, or required record absent)
  4  Authorization or sandbox refusal
  5  Runtime (DB locked, IO failure, schema mismatch)
  7  Rate / capacity (reserved for future provider-backed commands)

Try: kx mem facts --help # see all four subcommands
")]
    Facts {
        #[command(subcommand)]
        action: FactsAction,
    },
    /// Record a new observation with structured What / Why / Where / Learned fields
    #[command(after_help = "Examples:
  kx mem save --type bugfix \"Fixed N+1 in UserList\" \\
      --what \"added eager loading\" \\
      --why \"5k-user lists were 12s slow\" \\
      --where src/users/list.rs
  echo '{\"type\":\"decision\",\"title\":\"...\"}' | kx mem save --stdin

Exit codes:
  0  Success
  2  Usage error (unknown flag, malformed argument)
  3  Not found (id, key, or required record absent)
  4  Authorization or sandbox refusal
  5  Runtime (DB locked, IO failure, schema mismatch)
  7  Rate / capacity (reserved for future provider-backed commands)

Try: kx mem save --help # see all required fields
")]
    Save(SaveArgs),
}

#[cfg(feature = "memory-cli")]
#[derive(Subcommand)]
pub enum FactsAction {
    /// List every fact for the current project
    #[command(after_help = "Examples:
  kx mem facts list
  kx mem facts list --json | jq '.[].key'

Exit codes:
  0  Success
  2  Usage error (unknown flag, malformed argument)
  3  Not found (id, key, or required record absent)
  4  Authorization or sandbox refusal
  5  Runtime (DB locked, IO failure, schema mismatch)
  7  Rate / capacity (reserved for future provider-backed commands)

Try: kx mem facts list --json | jq '.[] | {key, value}'
")]
    List,
    /// Read a single fact by key
    #[command(after_help = "Examples:
  kx mem facts get auth-pattern
  kx mem facts get auth-pattern --json

Exit codes:
  0  Success
  2  Usage error (unknown flag, malformed argument)
  3  Not found (id, key, or required record absent)
  4  Authorization or sandbox refusal
  5  Runtime (DB locked, IO failure, schema mismatch)
  7  Rate / capacity (reserved for future provider-backed commands)

Try: kx mem facts list # see known keys, then kx mem facts get <key>
")]
    Get {
        /// Fact key
        key: String,
    },
    /// Write a fact; upserts on existing key
    #[command(after_help = "Examples:
  kx mem facts add auth-pattern \"OIDC + PKCE\"
  printf \"OIDC + PKCE\" | kx mem facts add auth-pattern --stdin

Exit codes:
  0  Success
  2  Usage error (unknown flag, malformed argument)
  3  Not found (id, key, or required record absent)
  4  Authorization or sandbox refusal
  5  Runtime (DB locked, IO failure, schema mismatch)
  7  Rate / capacity (reserved for future provider-backed commands)

Try: kx mem facts add my-key \"my value\" && kx mem facts get my-key
")]
    Add {
        /// Fact key
        key: String,
        /// Inline value (mutually exclusive with --stdin)
        value: Option<String>,
        /// Read the value from stdin
        #[arg(long)]
        stdin: bool,
    },
    /// Soft-delete a fact (recoverable; reads exclude soft-deleted by default)
    #[command(after_help = "Examples:
  kx mem facts delete auth-pattern

Exit codes:
  0  Success
  2  Usage error (unknown flag, malformed argument)
  3  Not found (id, key, or required record absent)
  4  Authorization or sandbox refusal
  5  Runtime (DB locked, IO failure, schema mismatch)
  7  Rate / capacity (reserved for future provider-backed commands)

Try: kx mem facts list # confirm key exists, then kx mem facts delete <key>
")]
    Delete {
        /// Fact key
        key: String,
    },
}

#[cfg(feature = "memory-cli")]
#[derive(clap::Args)]
pub struct SaveArgs {
    /// Observation type (bugfix, decision, pattern, config, discovery, learning, architecture)
    #[arg(long)]
    pub r#type: Option<String>,
    /// Title (positional, required unless --stdin)
    pub title: Option<String>,
    /// What changed
    #[arg(long)]
    pub what: Option<String>,
    /// Why it changed
    #[arg(long)]
    pub why: Option<String>,
    /// Where the change applied (file path)
    #[arg(long)]
    pub r#where: Option<String>,
    /// What was learned
    #[arg(long)]
    pub learned: Option<String>,
    /// Read the full SaveEntry JSON from stdin (mutually exclusive with inline fields)
    #[arg(long)]
    pub stdin: bool,
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
        /// Replace an existing skill of the same name even if it came from
        /// a different source. Required to shadow a trusted builtin.
        #[arg(long)]
        force: bool,
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
            if let SkillsAction::Add {
                source,
                trust,
                force: _,
            } = action
            {
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
            if let SkillsAction::Add {
                source,
                trust,
                force: _,
            } = action
            {
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
    #[cfg(feature = "serve")]
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
    #[cfg(feature = "serve")]
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
    #[cfg(feature = "serve")]
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

    /// CC-8 help text contract: every `kx mem *` subcommand's `--help`
    /// output must contain an Examples section, an Exit codes block,
    /// and a `Try:` line with a runnable example. This test walks the
    /// clap command tree and asserts each `mem` subcommand carries the
    /// required substrings so new variants cannot drift away from the
    /// contract without breaking the test.
    #[cfg(feature = "memory-cli")]
    #[test]
    fn cli_help_contract_cc8_mem_subcommands() {
        fn check(subcmd: &clap::Command, full_name: &str) {
            let help = subcmd.clone().render_long_help().to_string();
            assert!(
                help.contains("Examples:"),
                "{full_name} --help missing 'Examples:' section"
            );
            assert!(
                help.contains("Exit codes:"),
                "{full_name} --help missing 'Exit codes:' section"
            );
            assert!(
                help.contains("Try:"),
                "{full_name} --help missing 'Try:' line"
            );
        }

        let cmd = Cli::command();
        let mem = cmd
            .find_subcommand("mem")
            .expect("`kx mem` subcommand must exist when memory-cli feature is on");

        // Direct children of `kx mem`. `facts` is a container; its leaves
        // get checked separately below.
        for name in ["search", "get", "history", "stats", "facts", "save"] {
            let sub = mem
                .find_subcommand(name)
                .unwrap_or_else(|| panic!("`kx mem {name}` subcommand missing"));
            check(sub, &format!("kx mem {name}"));
        }

        // Leaves of `kx mem facts`.
        let facts = mem.find_subcommand("facts").expect("facts subcommand");
        for name in ["list", "get", "add", "delete"] {
            let sub = facts
                .find_subcommand(name)
                .unwrap_or_else(|| panic!("`kx mem facts {name}` subcommand missing"));
            check(sub, &format!("kx mem facts {name}"));
        }
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
