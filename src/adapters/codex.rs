//! Codex CLI adapter implementation per Phase F SDD §"Codex CLI adapter".
//!
//! `CodexAdapter` implements `kernex_adapter_core::Adapter` for
//! `AdapterId::CodexCli`. Templates are compiled in via `include_str!`
//! so air-gapped installs work (carried over from Phase E E-CC-7).
//!
//! Sprint F-1 scope so far: module shell, factory wiring, stub
//! templates, `detect()` populating `Detection::with_project_root`,
//! `install_command()` returning the canonical OpenAI install
//! one-liner, and `merge_codex_config_toml` (F-1.6). Out of scope for
//! this commit and landing in subsequent Sprint F-1 commits: the
//! shared `merge_marker_block` helper (F-1.7), preset wiring (F-1.8),
//! integration test in `tests/phase_f_codex.rs` (F-1.10), and the
//! `delta-agent-codex` CI gate (F-1.11).
//!
//! Public openspec scaffold lands at `openspec/changes/phase-f-codex-adapter/`
//! in a separate commit during Sprint F-1 kickoff.

use std::path::PathBuf;
use std::process::Command;

use async_trait::async_trait;
use kernex_adapter_core::Detection;
use kernex_runtime::{Adapter, AdapterError, AdapterId, Capability};
use toml_edit::{DocumentMut, Item, Table, TomlError};

/// Canonical OpenAI Codex install one-liner.
const INSTALL_COMMAND: &str = "npm install -g @openai/codex";

/// Compiled-in templates per Phase F ADR-003. Loaded once at binary link
/// time; no runtime template directory lookup. Content is stub-level for
/// Sprint F-1 scaffold; real template bodies land in subsequent commits.
pub const AGENTS_MD_TMPL: &str = include_str!("../../templates/codex/AGENTS.md.tmpl");
pub const CONFIG_TOML_TMPL: &str = include_str!("../../templates/codex/config.toml.tmpl");
pub const OUTPUT_STYLE_TMPL: &str = include_str!("../../templates/codex/output-style.md.tmpl");

/// Unit struct identity for the Codex CLI adapter. The adapter is
/// stateless; configuration flows through `InstallOptions` at the
/// configurator boundary (Phase E discipline).
#[derive(Debug, Default, Clone, Copy)]
pub struct CodexAdapter;

#[async_trait]
impl Adapter for CodexAdapter {
    fn id(&self) -> AdapterId {
        AdapterId::CodexCli
    }

    fn supports(&self, cap: Capability) -> bool {
        matches!(
            cap,
            Capability::Skills | Capability::Memory | Capability::Mcp | Capability::OutputStyle
        )
    }

    async fn detect(&self) -> Result<Detection, AdapterError> {
        let codex_path = locate_codex();
        let config_root = codex_config_root();
        let installed = codex_path.is_some()
            || config_root
                .as_ref()
                .is_some_and(|p| p.join("config.toml").exists());
        let version = if installed {
            read_codex_version()
        } else {
            None
        };
        // Codex writes both `~/.codex/config.toml` (home-rooted) and
        // `<cwd>/AGENTS.md` (project-rooted) per Phase F SDD spec.md
        // §"Codex CLI adapter". The project_root captures the cwd at
        // `kx install` invocation time so the Stage 5 sandbox check
        // accepts the project-local write per ADR-001 (RESOLVED Option A
        // 2026-05-19; kernex-adapter-core 0.8.3 surfaces the field +
        // builder used here). The Stage 5 sandbox check itself is
        // refactored to consume Detection's config_root + project_root
        // in a follow-up F-1.6 commit; until then the constructor call
        // populates the field for future consumers.
        let project_root = std::env::current_dir().ok();
        Ok(Detection::with_project_root(
            installed,
            config_root,
            project_root,
            version,
        ))
    }

    async fn install_command(&self) -> Result<String, AdapterError> {
        Ok(INSTALL_COMMAND.to_string())
    }
}

fn locate_codex() -> Option<PathBuf> {
    let output = Command::new("which").arg("codex").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn read_codex_version() -> Option<String> {
    let output = Command::new("codex").arg("--version").output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn codex_config_root() -> Option<PathBuf> {
    home_dir().map(|h| h.join(".codex"))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Merge a kernex-rendered `[mcp_servers.*]` block into an existing Codex
/// `~/.codex/config.toml`, preserving formatting and non-kernex entries.
///
/// Codex's MCP-server schema is one named sub-table per server under the
/// `mcp_servers` parent table:
/// ```toml
/// [mcp_servers.kernex]
/// command = "kx"
/// args = ["serve"]
/// ```
///
/// Behaviour:
/// - Existing comments and ordering outside `mcp_servers` are preserved
///   byte-for-byte via `toml_edit::DocumentMut`.
/// - Each `[mcp_servers.<name>]` table from `kernex_block` is upserted
///   into the existing document. Same-name entries are replaced (not
///   merged at the field level) so kernex's rendered template is the
///   source of truth for its own servers. Non-kernex entries under
///   `mcp_servers` are left untouched.
/// - If `existing` has no `mcp_servers` table, one is created.
/// - If `kernex_block` contains no `mcp_servers` table, the call is a
///   no-op returning `existing` unchanged.
///
/// Returns a `TomlError` if either input fails to parse.
pub fn merge_codex_config_toml(existing: &str, kernex_block: &str) -> Result<String, TomlError> {
    let mut doc: DocumentMut = if existing.is_empty() {
        DocumentMut::new()
    } else {
        existing.parse()?
    };
    let kernex: DocumentMut = kernex_block.parse()?;

    let Some(kernex_servers) = kernex.get("mcp_servers").and_then(Item::as_table) else {
        return Ok(doc.to_string());
    };

    // Normalise the `mcp_servers` slot to a Table before mutating it. The
    // explicit replace-if-not-table arm covers a malformed-but-parseable
    // `mcp_servers = "value"` upstream config; we choose to overwrite that
    // with a fresh table rather than crash a customer install.
    if !matches!(doc.get("mcp_servers"), Some(Item::Table(_))) {
        doc.insert("mcp_servers", Item::Table(Table::new()));
    }

    match doc.get_mut("mcp_servers").and_then(Item::as_table_mut) {
        Some(target) => {
            for (name, item) in kernex_servers.iter() {
                target.insert(name, item.clone());
            }
        }
        // Unreachable: we just normalised the slot to a Table above.
        // `unreachable!` is preferred over `.expect()` because lib-level
        // `clippy::expect_used` is denied.
        None => unreachable!("mcp_servers slot normalised to Table above"),
    }

    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kernex_runtime::Capability;

    #[test]
    fn install_command_is_canonical() {
        assert_eq!(INSTALL_COMMAND, "npm install -g @openai/codex");
    }

    #[test]
    fn supports_expected_capabilities() {
        let adapter = CodexAdapter;
        assert!(adapter.supports(Capability::Skills));
        assert!(adapter.supports(Capability::Memory));
        assert!(adapter.supports(Capability::Mcp));
        assert!(adapter.supports(Capability::OutputStyle));
    }

    #[test]
    fn templates_compiled_in() {
        // The include_str! consts are loaded at compile time. These
        // assertions pin the include paths (templates/codex/*.tmpl)
        // even at stub-level content, so a missing file fails at
        // compile rather than at first `kx install` run.
        assert!(!AGENTS_MD_TMPL.is_empty());
        assert!(!CONFIG_TOML_TMPL.is_empty());
        assert!(!OUTPUT_STYLE_TMPL.is_empty());
    }

    #[tokio::test]
    async fn id_is_codex_cli() {
        let adapter = CodexAdapter;
        assert_eq!(adapter.id(), AdapterId::CodexCli);
    }

    #[tokio::test]
    async fn install_command_returns_ok() {
        let adapter = CodexAdapter;
        let cmd = adapter.install_command().await.expect("install command");
        assert_eq!(cmd, INSTALL_COMMAND);
    }

    const KERNEX_BLOCK: &str = r#"
[mcp_servers.kernex]
command = "kx"
args = ["serve"]
"#;

    #[test]
    fn merge_config_toml_creates_table_when_absent() {
        let merged =
            merge_codex_config_toml("# user comment\n", KERNEX_BLOCK).expect("merge succeeds");
        assert!(merged.contains("# user comment"), "comment preserved");
        assert!(
            merged.contains("[mcp_servers.kernex]"),
            "kernex entry added"
        );
        assert!(merged.contains("command = \"kx\""));
    }

    #[test]
    fn merge_config_toml_preserves_existing() {
        let existing = r#"# top-of-file comment
project = "demo"

[mcp_servers.other]
command = "other-server"
args = ["--flag"]
"#;
        let merged = merge_codex_config_toml(existing, KERNEX_BLOCK).expect("merge succeeds");
        assert!(
            merged.contains("# top-of-file comment"),
            "comment preserved"
        );
        assert!(
            merged.contains("project = \"demo\""),
            "top-level key preserved"
        );
        assert!(
            merged.contains("[mcp_servers.other]"),
            "non-kernex server preserved: {merged}"
        );
        assert!(
            merged.contains("command = \"other-server\""),
            "non-kernex command preserved"
        );
        assert!(
            merged.contains("[mcp_servers.kernex]"),
            "kernex entry added: {merged}"
        );
    }

    #[test]
    fn merge_config_toml_upserts_kernex() {
        let existing = r#"
[mcp_servers.kernex]
command = "stale-binary"
args = ["old-arg"]
"#;
        let merged = merge_codex_config_toml(existing, KERNEX_BLOCK).expect("merge succeeds");
        assert!(
            merged.contains("command = \"kx\""),
            "kernex command upserted to current template: {merged}"
        );
        assert!(
            !merged.contains("stale-binary"),
            "stale kernex entry replaced: {merged}"
        );
        assert!(
            !merged.contains("old-arg"),
            "stale kernex args replaced: {merged}"
        );
    }

    #[test]
    fn merge_config_toml_no_op_when_kernex_block_empty() {
        let existing = "project = \"demo\"\n";
        let merged = merge_codex_config_toml(existing, "# nothing here\n").expect("merge succeeds");
        assert_eq!(merged, existing, "empty kernex block leaves existing alone");
    }

    #[test]
    fn merge_config_toml_parse_error_surfaces() {
        let result = merge_codex_config_toml("not = valid = toml", KERNEX_BLOCK);
        assert!(result.is_err(), "malformed existing TOML must error");
    }
}
