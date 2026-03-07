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
}
