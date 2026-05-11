//! Handler stubs for `kx mem *` subcommands.
//!
//! Each handler returns `CliError::NotImplemented` in this scaffold commit.
//! Follow-up commits per `openspec/changes/kx-mem-cli-promotion/tasks.md`
//! Step 2 fill in the trait calls into `kernex_memory::MemoryStore`.

use crate::mem::errors::CliError;

pub async fn search() -> Result<(), CliError> {
    Err(CliError::NotImplemented {
        subcommand: "kx mem search",
    })
}

pub async fn get() -> Result<(), CliError> {
    Err(CliError::NotImplemented {
        subcommand: "kx mem get",
    })
}

pub async fn history() -> Result<(), CliError> {
    Err(CliError::NotImplemented {
        subcommand: "kx mem history",
    })
}

pub async fn stats() -> Result<(), CliError> {
    Err(CliError::NotImplemented {
        subcommand: "kx mem stats",
    })
}

pub async fn facts() -> Result<(), CliError> {
    Err(CliError::NotImplemented {
        subcommand: "kx mem facts",
    })
}

pub async fn save() -> Result<(), CliError> {
    Err(CliError::NotImplemented {
        subcommand: "kx mem save",
    })
}
