use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "kx", version, about = "CLI dev assistant powered by Kernex")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

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
    /// Manage installed skills
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
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
