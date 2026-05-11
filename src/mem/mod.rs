//! `kx mem *` subcommand surface.
//!
//! See [openspec/changes/kx-mem-cli-promotion/](../../openspec/changes/kx-mem-cli-promotion)
//! for the change spec. This module owns the CLI subcommand handlers, the
//! auto-JSON renderer, and the structured error type that maps to the
//! exit-code taxonomy (ADR-005).
//!
//! The dispatcher resolves each invocation's project-scoped data dir
//! (honoring per-subcommand `--project` overrides), opens a
//! `kernex_memory::Store` against it, hands a `MemoryStore` trait handle
//! to the pure handler in [`cli`], then routes the typed record set
//! through [`render`] using the global `--json` / `--compact` / `--select`
//! flags.
//!
//! Tracing: each public boundary carries `#[tracing::instrument(name = "kernex.mem.*", skip_all, ...)]`
//! per the workspace tracing convention. Operator-supplied content (query,
//! select fields, file paths) is never recorded as a default field; only
//! shape (`query_len`, `result_count`) and typed `kernex.error_kind` cross
//! the span boundary.

#![cfg(feature = "memory-cli")]

pub mod cli;
pub mod errors;
pub mod render;
pub mod types;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use kernex_core::config::MemoryConfig;
use kernex_memory::{into_handle, MemoryStore, Store};

use crate::cli::{FactsAction, MemAction};
use crate::data_dir_for;
use crate::mem::cli::{HistoryOpts, SearchOpts, StatsOpts};
use crate::mem::errors::CliError;

/// Render flags forwarded from the top-level `Command::Mem` variant.
/// Every `kx mem *` subcommand inherits these.
#[derive(Debug, Clone)]
pub struct RenderFlags {
    /// Force JSON output even when stdout is a TTY (CC-2).
    pub json: bool,
    /// Project records to the compact field set (CC-3).
    pub compact: bool,
    /// Project records to the named field set (CC-4). Validated against
    /// the per-subcommand field allowlist in [`render`]; unknown names
    /// produce `CliError::Usage`.
    pub select: Vec<String>,
}

/// Dispatch a `kx mem ...` invocation. Resolves the project-scoped data
/// dir (per-subcommand `--project` wins over `default_project`), opens the
/// memory store, runs the pure handler, renders the result.
#[tracing::instrument(
    name = "kernex.mem.dispatch",
    skip_all,
    fields(
        sender_id = %crate::mem::cli::CLI_SENDER_ID,
        default_project = %default_project,
        project_explicit = explicit_project.is_some(),
        json = flags.json,
        compact = flags.compact,
    ),
)]
pub async fn dispatch(
    action: MemAction,
    default_project: &str,
    explicit_project: Option<&str>,
    flags: RenderFlags,
) -> anyhow::Result<()> {
    let json_mode = render::json_mode(flags.json);
    match dispatch_inner(action, default_project, explicit_project, &flags, json_mode).await {
        Ok(()) => Ok(()),
        Err(err) => {
            tracing::warn!(
                kernex.error_kind = err.kind_name(),
                exit_code = err.exit_code(),
                "kx mem dispatch failed",
            );
            eprintln!("{}", render::render_error(&err, json_mode));
            Err(anyhow::Error::from(err))
        }
    }
}

async fn dispatch_inner(
    action: MemAction,
    default_project: &str,
    explicit_project: Option<&str>,
    flags: &RenderFlags,
    json_mode: bool,
) -> Result<(), CliError> {
    match action {
        MemAction::Search {
            query,
            limit,
            since,
            r#type,
        } => {
            // Search auto-creates the data dir on first use (matches
            // `kx dev`); explicit `--project` does not gate it.
            let data_dir = data_dir_for(default_project);
            let store = open_store(&data_dir).await?;
            let records = cli::search(
                store.as_ref(),
                SearchOpts {
                    query,
                    limit,
                    since,
                    kind: r#type,
                },
            )
            .await?;
            // CC-6 invariant: stdout stays empty on error. `render_search_json`
            // returns `Result<String, CliError>` (it may reject `--select`),
            // so it must run BEFORE any stdout write. Do not refactor the
            // render-then-print order without re-reading this comment.
            if json_mode {
                let out = render::render_search_json(&records, flags.compact, &flags.select)?;
                println!("{out}");
            } else {
                print!("{}", render::render_search_table(&records));
            }
            Ok(())
        }
        MemAction::History { last } => {
            // S-history-4: an explicit `--project bar` for a missing
            // project exits 3. An implicit fallback (the cwd-derived
            // default) auto-creates the data dir on first use, matching
            // `kx mem search` and `kx dev`. The existence check fires
            // only when the operator named the project on the CLI via
            // the global `--project` flag.
            let data_dir = if explicit_project.is_some() {
                resolve_project_data_dir(default_project)?
            } else {
                data_dir_for(default_project)
            };
            let store = open_store(&data_dir).await?;
            let records = cli::history(
                store.as_ref(),
                HistoryOpts {
                    last: last.unwrap_or(cli::DEFAULT_HISTORY_LIMIT),
                    project: default_project.to_string(),
                },
            )
            .await?;
            if json_mode {
                let out = render::render_history_json(&records, flags.compact, &flags.select)?;
                println!("{out}");
            } else {
                print!("{}", render::render_history_table(&records));
            }
            Ok(())
        }
        MemAction::Get { id } => {
            // Same explicit/implicit project handling as History.
            let data_dir = if explicit_project.is_some() {
                resolve_project_data_dir(default_project)?
            } else {
                data_dir_for(default_project)
            };
            let store = open_store(&data_dir).await?;
            let record = cli::get(store.as_ref(), &id).await?;
            if json_mode {
                let out = render::render_search_json(
                    std::slice::from_ref(&record),
                    flags.compact,
                    &flags.select,
                )?;
                println!("{out}");
            } else {
                print!(
                    "{}",
                    render::render_search_table(std::slice::from_ref(&record))
                );
            }
            Ok(())
        }
        MemAction::Stats {} => {
            // Same explicit/implicit project handling as History
            // (S-stats-2 explicitly allows an empty project as a
            // VALID project; the existence check only fires when
            // `--project` was named on the CLI).
            let data_dir = if explicit_project.is_some() {
                resolve_project_data_dir(default_project)?
            } else {
                data_dir_for(default_project)
            };
            let store = open_store(&data_dir).await?;
            let record = cli::stats(
                store.as_ref(),
                StatsOpts {
                    project: default_project.to_string(),
                },
            )
            .await?;
            if json_mode {
                let out = render::render_stats_json(&record, flags.compact, &flags.select)?;
                println!("{out}");
            } else {
                print!("{}", render::render_stats_table(&record));
            }
            Ok(())
        }
        MemAction::Facts { action } => {
            // Same explicit/implicit project gating as History/Stats.
            let data_dir = if explicit_project.is_some() {
                resolve_project_data_dir(default_project)?
            } else {
                data_dir_for(default_project)
            };
            let store = open_store(&data_dir).await?;
            dispatch_facts(store.as_ref(), action, flags, json_mode).await
        }
        MemAction::Save(_) => Err(CliError::NotImplemented {
            subcommand: "kx mem save",
        }),
    }
}

/// Dispatch the four `kx mem facts *` subcommands. Pulled out of
/// `dispatch_inner` so `--stdin` handling for `facts add` stays close
/// to its single use site without bloating the outer match.
async fn dispatch_facts(
    store: &dyn MemoryStore,
    action: FactsAction,
    flags: &RenderFlags,
    json_mode: bool,
) -> Result<(), CliError> {
    match action {
        FactsAction::List => {
            let records = cli::facts_list(store).await?;
            if json_mode {
                let out = render::render_facts_list_json(&records, flags.compact, &flags.select)?;
                println!("{out}");
            } else {
                print!("{}", render::render_facts_list_table(&records));
            }
            Ok(())
        }
        FactsAction::Get { key } => {
            let record = cli::facts_get(store, &key).await?;
            if json_mode {
                let out = render::render_facts_record_json(&record, flags.compact, &flags.select)?;
                println!("{out}");
            } else {
                print!("{}", render::render_facts_record_table(&record));
            }
            Ok(())
        }
        FactsAction::Add { key, value, stdin } => {
            // `--stdin` and an inline positional value are mutually
            // exclusive: the spec wants exactly one value source.
            let resolved_value = match (value, stdin) {
                (Some(_), true) => {
                    return Err(CliError::Usage {
                        message: "cannot combine inline value with --stdin".to_string(),
                        hint:
                            "Pass the value as a positional argument OR pipe via --stdin, not both."
                                .to_string(),
                    });
                }
                (None, false) => {
                    return Err(CliError::Usage {
                        message: "fact value is required".to_string(),
                        hint: "Provide the value as a positional argument or pipe via --stdin."
                            .to_string(),
                    });
                }
                (Some(v), false) => v,
                (None, true) => read_stdin_value()?,
            };
            let record = cli::facts_add(store, &key, &resolved_value).await?;
            if json_mode {
                let out = render::render_facts_record_json(&record, flags.compact, &flags.select)?;
                println!("{out}");
            } else {
                print!("{}", render::render_facts_record_table(&record));
            }
            Ok(())
        }
        FactsAction::Delete { key } => {
            cli::facts_delete(store, &key).await?;
            // Successful delete renders nothing on stdout in either mode;
            // exit 0 is the operator-visible signal. JSON consumers can
            // probe `kx mem facts get <key>` to confirm the soft-delete.
            Ok(())
        }
    }
}

/// Read the `facts add --stdin` value from the process's standard input.
/// Reads to EOF, trims trailing newlines (a single trailing `\n` from
/// `echo` or a heredoc is operator-friendly to strip; multi-line values
/// are preserved otherwise).
fn read_stdin_value() -> Result<String, CliError> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| CliError::Runtime {
            message: format!("reading --stdin failed: {e}"),
            hint: "Pipe the value into stdin (e.g. `echo foo | kx mem facts add k --stdin`)."
                .to_string(),
        })?;
    let trimmed = buf.trim_end_matches('\n').to_string();
    Ok(trimmed)
}

/// Resolve a project name to its `~/.kx/projects/<name>/` data dir,
/// returning `CliError::NotFound` (exit 3) when the dir does not exist.
/// Used by per-subcommand `--project` overrides (S-history-4: unknown
/// project is exit 3).
fn resolve_project_data_dir(project: &str) -> Result<PathBuf, CliError> {
    let dir = data_dir_for(project);
    if !dir.exists() {
        return Err(CliError::NotFound {
            message: format!("project '{project}' not found"),
            hint: "Run `kx init` inside that project's directory, or list known projects with `ls ~/.kx/projects/`.".to_string(),
        });
    }
    Ok(dir)
}

/// Open the project-scoped memory store rooted at `data_dir/memory.db`.
///
/// `Store::new` runs the kernex-memory migration sweep on every call.
/// On a current DB this is a `CREATE TABLE IF NOT EXISTS _migrations`
/// followed by one row-existence probe per known migration. The cost is
/// sub-10 ms on SSD but scales linearly with migration count; pushing a
/// fast-path check upstream is tracked separately (FU-D-AG-04).
#[tracing::instrument(name = "kernex.mem.open_store", skip_all, err)]
async fn open_store(data_dir: &Path) -> Result<Arc<dyn MemoryStore>, CliError> {
    let db_path = data_dir.join("memory.db");
    let cfg = MemoryConfig {
        db_path: db_path.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let store = Store::new(&cfg).await.map_err(|e| CliError::Runtime {
        message: format!("could not open memory store at {}: {e}", db_path.display()),
        hint: "Run `kx init` to bootstrap the project data dir.".to_string(),
    })?;
    Ok(into_handle(store))
}
