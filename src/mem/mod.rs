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
use kernex_memory::{into_handle, MemoryStore, SaveEntry, Store};

use crate::cli::{FactsAction, MemAction, SaveArgs};
use crate::data_dir_for;
use crate::mem::cli::{parse_observation_type, HistoryOpts, SearchOpts, StatsOpts};
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
        MemAction::Save(args) => {
            // Save auto-creates the data dir on first use (matches
            // `kx mem search` and `kx dev`); explicit `--project` still
            // gates existence via `resolve_project_data_dir` to keep the
            // S-history-4 / S-stats-4 family consistent.
            let data_dir = if explicit_project.is_some() {
                resolve_project_data_dir(default_project)?
            } else {
                data_dir_for(default_project)
            };
            let entry = build_save_entry(args)?;
            // ADR-005 S-save-8: refuse the write when the sandbox blocks
            // ~/.kx/projects/. The check probes write access on the data
            // dir; a read-only or absent-but-uncreatable target maps to
            // CliError::Sandbox (exit 4).
            check_sandbox_write(&data_dir)?;
            let store = open_store(&data_dir).await?;
            let record = cli::save(store.as_ref(), entry).await?;
            if json_mode {
                let out = render::render_save_json(&record, flags.compact, &flags.select)?;
                println!("{out}");
            } else {
                print!("{}", render::render_save_table(&record));
            }
            Ok(())
        }
    }
}

/// Normalize `kx mem save` operator input into a typed `SaveEntry`.
///
/// Handles the three exit-2 failure modes from the kx-mem-cli-promotion
/// spec: S-save-6 (mixing `--stdin` with inline fields), S-save-3 / S-save-7
/// (missing or empty title), S-save-4 (missing `--type`), and S-save-5
/// (unknown type). The `--stdin` path reads a full `SaveEntry` JSON
/// document from stdin and parses it via serde; the inline path assembles
/// the entry from clap flags.
fn build_save_entry(args: SaveArgs) -> Result<SaveEntry, CliError> {
    let SaveArgs {
        r#type,
        title,
        what,
        why,
        r#where,
        learned,
        stdin,
    } = args;

    if stdin {
        // S-save-6: any inline structured field alongside --stdin is a
        // usage error. The two input modes are mutually exclusive.
        let inline_set = r#type.is_some()
            || title.is_some()
            || what.is_some()
            || why.is_some()
            || r#where.is_some()
            || learned.is_some();
        if inline_set {
            return Err(CliError::Usage {
                message: "cannot combine --stdin with inline fields".to_string(),
                hint: "Pass the full SaveEntry as JSON via --stdin, or use inline flags only."
                    .to_string(),
            });
        }
        let raw = read_stdin_value()?;
        let entry = parse_save_entry_json(&raw)?;
        validate_save_entry(&entry)?;
        Ok(entry)
    } else {
        // S-save-4: --type is required in the inline path.
        let kind_str = r#type.ok_or_else(|| CliError::Usage {
            message: "--type is required".to_string(),
            hint: format!(
                "Pass --type=<kind>. Valid types: {}",
                crate::mem::types::OBSERVATION_TYPES.join(", ")
            ),
        })?;
        let kind = parse_observation_type(&kind_str)?;
        // S-save-3: a positional title is required; S-save-7: empty
        // title is a usage error before the DB CHECK constraint fires.
        let title = title.ok_or_else(|| CliError::Usage {
            message: "title is required".to_string(),
            hint: "Pass the title as a positional argument: kx mem save --type <kind> \"title\"."
                .to_string(),
        })?;
        if title.is_empty() {
            return Err(CliError::Usage {
                message: "title cannot be empty".to_string(),
                hint: "Provide a non-empty title summarizing the observation.".to_string(),
            });
        }
        let mut entry = SaveEntry::new(crate::mem::cli::CLI_SENDER_ID, kind, title);
        entry.what = what;
        entry.why = why;
        entry.where_field = r#where;
        entry.learned = learned;
        Ok(entry)
    }
}

/// Parse a `SaveEntry` JSON document from `--stdin`. The serde derive on
/// `kernex_memory::SaveEntry` renames the public fields to match the
/// spec's JSON shape (`type`, `where`, snake_case for the rest), so the
/// input format mirrors the output of `kx mem save` (S-save-2).
fn parse_save_entry_json(raw: &str) -> Result<SaveEntry, CliError> {
    serde_json::from_str::<SaveEntry>(raw).map_err(|e| CliError::Usage {
        message: format!("--stdin JSON does not parse as SaveEntry: {e}"),
        hint: "Expected fields: type, title, what, why, where, learned. See `kx mem save --help`."
            .to_string(),
    })
}

/// Apply the inline-path validators to a stdin-parsed entry so S-save-3 /
/// S-save-5 / S-save-7 surface as exit 2 instead of exit 5 (the DB CHECK
/// path).
fn validate_save_entry(entry: &SaveEntry) -> Result<(), CliError> {
    if entry.title.is_empty() {
        return Err(CliError::Usage {
            message: "title cannot be empty".to_string(),
            hint: "Provide a non-empty `title` field in the JSON document.".to_string(),
        });
    }
    // ObservationType deserializes via serde so unknown strings fail at
    // parse time; reaching this point means the kind is one of the seven
    // valid variants. No further check required.
    Ok(())
}

/// Probe write access on the resolved data dir. The check creates the
/// directory if missing and writes a small marker file to confirm the
/// process can actually persist a row; permission denials surface as
/// [`CliError::Sandbox`] (exit 4) per ADR-005 S-save-8. Other IO faults
/// (disk full, IO error mid-write) map to [`CliError::Runtime`] so the
/// operator gets a distinct hint surface.
///
/// Why a probe vs trusting the DB write: kernex-sandbox refuses writes
/// at the policy layer before the syscall surfaces an error, so a
/// dedicated probe gives the dispatcher a clean failure point that
/// happens before opening the SQLite store. The marker file is removed
/// immediately; failure to remove it does not block the save (the next
/// save overwrites it).
fn check_sandbox_write(data_dir: &Path) -> Result<(), CliError> {
    use std::io::{ErrorKind, Write};

    if let Err(e) = std::fs::create_dir_all(data_dir) {
        return Err(map_io_to_sandbox_or_runtime(
            e,
            &format!("cannot create data dir {}", data_dir.display()),
        ));
    }
    let probe = data_dir.join(".kx-write-probe");
    let mut file = match std::fs::File::create(&probe) {
        Ok(f) => f,
        Err(e) => {
            return Err(map_io_to_sandbox_or_runtime(
                e,
                &format!("write probe denied at {}", probe.display()),
            ));
        }
    };
    if let Err(e) = file.write_all(b"ok") {
        return Err(map_io_to_sandbox_or_runtime(
            e,
            &format!("write probe failed at {}", probe.display()),
        ));
    }
    drop(file);
    // Best-effort cleanup; a leftover probe is harmless and the next
    // save overwrites it. PermissionDenied here would also indicate a
    // sandbox refusal, but the write itself already succeeded so we
    // don't fail the save on it.
    let _ = std::fs::remove_file(&probe);
    // Match-style guard against ErrorKind being non-exhaustive.
    let _ = ErrorKind::PermissionDenied;
    Ok(())
}

fn map_io_to_sandbox_or_runtime(err: std::io::Error, context: &str) -> CliError {
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        CliError::Sandbox {
            message: format!("{context}: {err}"),
            hint: "The sandbox blocks writes to this path. \
                   Run from a project directory with write access, or \
                   relax the sandbox policy with `kx config sandbox`."
                .to_string(),
        }
    } else {
        CliError::Runtime {
            message: format!("{context}: {err}"),
            hint: "Check disk space and filesystem health, then retry.".to_string(),
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_args() -> SaveArgs {
        SaveArgs {
            r#type: None,
            title: None,
            what: None,
            why: None,
            r#where: None,
            learned: None,
            stdin: false,
        }
    }

    #[test]
    fn s_save_3_missing_title_is_exit_2() {
        // S-save-3: title is required in the inline path. The dispatcher
        // surfaces a usage error before any store is opened.
        let mut a = empty_args();
        a.r#type = Some("bugfix".to_string());
        let err = build_save_entry(a).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(format!("{err}").contains("title is required"));
    }

    #[test]
    fn s_save_4_missing_type_is_exit_2() {
        // S-save-4: --type is required.
        let mut a = empty_args();
        a.title = Some("something".to_string());
        let err = build_save_entry(a).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(format!("{err}").contains("--type is required"));
    }

    #[test]
    fn s_save_5_unknown_type_is_exit_2() {
        // S-save-5: an unknown type rejected with a hint listing the
        // valid set.
        let mut a = empty_args();
        a.r#type = Some("bogus".to_string());
        a.title = Some("x".to_string());
        let err = build_save_entry(a).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.hint().contains("decision"));
    }

    #[test]
    fn s_save_6_stdin_with_inline_field_is_exit_2() {
        // S-save-6: --stdin combined with any inline field exits 2.
        let mut a = empty_args();
        a.stdin = true;
        a.what = Some("...".to_string());
        let err = build_save_entry(a).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(format!("{err}").contains("--stdin"));
    }

    #[test]
    fn s_save_6_stdin_with_type_is_exit_2() {
        // Setting `--type` alongside `--stdin` is the same violation.
        let mut a = empty_args();
        a.stdin = true;
        a.r#type = Some("bugfix".to_string());
        let err = build_save_entry(a).unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn s_save_7_empty_title_is_exit_2() {
        // S-save-7: an empty title is rejected at the CLI layer (exit 2)
        // before the DB CHECK constraint surfaces as a runtime error.
        let mut a = empty_args();
        a.r#type = Some("bugfix".to_string());
        a.title = Some(String::new());
        let err = build_save_entry(a).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(format!("{err}").contains("title cannot be empty"));
    }

    #[test]
    fn s_save_2_stdin_json_parses() {
        // S-save-2 happy path: the JSON shape mirrors the SaveEntry
        // wire format (rename: type, where).
        let raw = r#"{
            "sender_id":"cli-operator",
            "type":"decision",
            "title":"Adopt rusqlite",
            "what":"chose rusqlite over sqlx",
            "why":"sync API matches our blocking store layer",
            "where":"Cargo.toml",
            "learned":"sqlx async did not pay off for our access pattern"
        }"#;
        let entry = parse_save_entry_json(raw).unwrap();
        assert_eq!(entry.title, "Adopt rusqlite");
        assert_eq!(entry.where_field.as_deref(), Some("Cargo.toml"));
        assert_eq!(entry.kind.as_db_str(), "decision");
    }

    #[test]
    fn s_save_2_stdin_invalid_json_is_exit_2() {
        // Malformed JSON should surface as a usage error, not a runtime
        // panic; the hint names the expected fields so the operator can
        // self-correct.
        let err = parse_save_entry_json("not json").unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.hint().contains("type"));
    }

    #[test]
    fn s_save_2_stdin_empty_title_in_json_is_exit_2() {
        // Empty title in JSON also exits 2; reuses validate_save_entry.
        let raw = r#"{"sender_id":"x","type":"bugfix","title":""}"#;
        let entry = parse_save_entry_json(raw).unwrap();
        let err = validate_save_entry(&entry).unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn s_save_8_readonly_dir_is_sandbox_refusal() {
        // S-save-8: the sandbox check runs before the store opens. A
        // read-only data dir surfaces as CliError::Sandbox (exit 4),
        // distinct from a generic IO failure.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let tmp = tempfile::TempDir::new().unwrap();
            let data_dir = tmp.path().join("locked");
            std::fs::create_dir(&data_dir).unwrap();
            let mut perms = std::fs::metadata(&data_dir).unwrap().permissions();
            perms.set_mode(0o555); // r-x for owner: no write
            std::fs::set_permissions(&data_dir, perms.clone()).unwrap();

            let result = check_sandbox_write(&data_dir);

            // Restore so TempDir can clean up.
            perms.set_mode(0o755);
            std::fs::set_permissions(&data_dir, perms).unwrap();

            let err = result.expect_err("read-only dir must refuse the write probe");
            assert_eq!(err.exit_code(), 4);
        }
        // On non-Unix CI runners chmod-based sandboxing is unreliable;
        // skip the assertion there. The Unix path covers the
        // GitHub-hosted ubuntu-* and macos-* matrix entries.
    }

    #[test]
    fn s_save_8_writable_dir_passes_check() {
        let tmp = tempfile::TempDir::new().unwrap();
        check_sandbox_write(tmp.path()).expect("a fresh temp dir must pass the write probe");
        // The probe cleans up after itself.
        assert!(!tmp.path().join(".kx-write-probe").exists());
    }
}
