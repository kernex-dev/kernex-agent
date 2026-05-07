//! CLAUDE.md loading hierarchy for system prompt composition.
//!
//! Reads from three locations in order (global to local):
//! 1. `~/.kx/CLAUDE.md` — global user instructions
//! 2. `./CLAUDE.md` — project-level instructions
//! 3. `./.kx/CLAUDE.md` — local project override
//!
//! Each file may contain `@import path/to/file.md` directives to pull in
//! additional content. Imports are resolved relative to the containing file.
//! Imports within imported files are not recursively expanded.

use std::path::{Path, PathBuf};

/// Loads and merges CLAUDE.md files from the standard hierarchy.
pub struct SystemPromptLoader {
    /// `~/.kx/CLAUDE.md` — global user instructions.
    pub global_path: PathBuf,
    /// `./CLAUDE.md` — project-level instructions.
    pub project_path: PathBuf,
    /// `./.kx/CLAUDE.md` — local project override.
    pub local_path: PathBuf,
}

impl SystemPromptLoader {
    /// Create a loader for the given working directory.
    pub fn new(cwd: &Path) -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        Self {
            global_path: home.join(".kx").join("CLAUDE.md"),
            project_path: cwd.join("CLAUDE.md"),
            local_path: cwd.join(".kx").join("CLAUDE.md"),
        }
    }

    /// Load and merge all available CLAUDE.md files.
    ///
    /// Files are concatenated in order: global, project, local.
    /// Missing files are silently skipped.
    /// `@import` directives in each file are resolved relative to that file.
    pub fn load(&self) -> String {
        let candidates: &[&PathBuf] = &[&self.global_path, &self.project_path, &self.local_path];

        let parts: Vec<String> = candidates
            .iter()
            .filter(|p| p.exists())
            .filter_map(|p| {
                let content = std::fs::read_to_string(p).ok()?;
                let base_dir = p.parent().unwrap_or_else(|| Path::new("."));
                Some(resolve_imports(&content, base_dir))
            })
            .collect();

        parts.join("\n\n")
    }
}

/// Maximum size for an imported file. Anything larger is silently dropped to
/// prevent a hostile CLAUDE.md from bloating the system prompt past provider
/// context limits.
const MAX_IMPORT_BYTES: u64 = 256 * 1024;

/// Expand `@import path/to/file.md` directives in `content`.
///
/// Lines that start with `@import ` (after trimming) are replaced with the
/// contents of the referenced file. Paths are resolved relative to `base_dir`
/// and confined to descendants of `base_dir` to prevent a hostile CLAUDE.md
/// from reading arbitrary host files (e.g. `~/.ssh/id_rsa`, `/etc/passwd`)
/// into the system prompt that ships to the LLM provider.
///
/// Imports are dropped silently if:
/// - the path is absolute,
/// - the resolved path escapes `base_dir` via `..`,
/// - the file does not exist,
/// - the file exceeds [`MAX_IMPORT_BYTES`].
fn resolve_imports(content: &str, base_dir: &Path) -> String {
    let mut out: Vec<String> = Vec::with_capacity(content.lines().count());
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("@import ") {
            let raw = rest.trim();
            if let Some(imported) = read_confined_import(raw, base_dir) {
                out.push(imported);
            }
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

fn read_confined_import(raw: &str, base_dir: &Path) -> Option<String> {
    let candidate = Path::new(raw);
    if candidate.is_absolute() {
        tracing::warn!(import = raw, "rejected @import: absolute path");
        return None;
    }

    let joined = base_dir.join(candidate);
    let canonical_target = std::fs::canonicalize(&joined).ok()?;
    let canonical_base = std::fs::canonicalize(base_dir).ok()?;
    if !canonical_target.starts_with(&canonical_base) {
        tracing::warn!(
            import = raw,
            "rejected @import: resolves outside base directory"
        );
        return None;
    }

    let metadata = std::fs::metadata(&canonical_target).ok()?;
    if metadata.len() > MAX_IMPORT_BYTES {
        tracing::warn!(
            import = raw,
            size = metadata.len(),
            "rejected @import: file exceeds size cap"
        );
        return None;
    }

    std::fs::read_to_string(&canonical_target).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp_dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("__kx_loader_{tag}__"));
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn load_no_files_returns_empty() {
        let tmp = tmp_dir("empty");
        let loader = SystemPromptLoader {
            global_path: tmp.join("g.md"),
            project_path: tmp.join("p.md"),
            local_path: tmp.join("l.md"),
        };
        assert!(loader.load().is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_single_project_file() {
        let tmp = tmp_dir("single");
        let path = tmp.join("CLAUDE.md");
        fs::write(&path, "Be helpful.").unwrap();

        let loader = SystemPromptLoader {
            global_path: tmp.join("missing_g.md"),
            project_path: path,
            local_path: tmp.join("missing_l.md"),
        };
        assert_eq!(loader.load(), "Be helpful.");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_merges_all_three_files() {
        let tmp = tmp_dir("merge");
        fs::write(tmp.join("global.md"), "Global rules.").unwrap();
        fs::write(tmp.join("project.md"), "Project rules.").unwrap();
        fs::write(tmp.join("local.md"), "Local rules.").unwrap();

        let loader = SystemPromptLoader {
            global_path: tmp.join("global.md"),
            project_path: tmp.join("project.md"),
            local_path: tmp.join("local.md"),
        };
        let result = loader.load();
        assert!(result.contains("Global rules."));
        assert!(result.contains("Project rules."));
        assert!(result.contains("Local rules."));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_order_global_before_project_before_local() {
        let tmp = tmp_dir("order");
        fs::write(tmp.join("a.md"), "first").unwrap();
        fs::write(tmp.join("b.md"), "second").unwrap();
        fs::write(tmp.join("c.md"), "third").unwrap();

        let loader = SystemPromptLoader {
            global_path: tmp.join("a.md"),
            project_path: tmp.join("b.md"),
            local_path: tmp.join("c.md"),
        };
        let result = loader.load();
        let pos_first = result.find("first").unwrap_or(usize::MAX);
        let pos_second = result.find("second").unwrap_or(usize::MAX);
        let pos_third = result.find("third").unwrap_or(usize::MAX);
        assert!(pos_first < pos_second);
        assert!(pos_second < pos_third);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn load_skips_missing_files_silently() {
        let tmp = tmp_dir("skip");
        fs::write(tmp.join("project.md"), "only project").unwrap();

        let loader = SystemPromptLoader {
            global_path: tmp.join("missing_g.md"),
            project_path: tmp.join("project.md"),
            local_path: tmp.join("missing_l.md"),
        };
        assert_eq!(loader.load(), "only project");
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_imports_expands_directive() {
        let tmp = tmp_dir("import_ok");
        fs::write(tmp.join("rules.md"), "imported content").unwrap();

        let content = "before\n@import rules.md\nafter";
        let result = resolve_imports(content, &tmp);
        assert!(result.contains("imported content"));
        assert!(result.contains("before"));
        assert!(result.contains("after"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_imports_missing_file_drops_line() {
        let tmp = tmp_dir("import_missing");
        let content = "@import nonexistent.md\nstays";
        let result = resolve_imports(content, &tmp);
        assert!(result.contains("stays"));
        assert!(!result.contains("@import"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_imports_plain_lines_unchanged() {
        let tmp = tmp_dir("import_plain");
        let content = "line one\nline two\nline three";
        let result = resolve_imports(content, &tmp);
        assert_eq!(result, content);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_imports_indented_directive() {
        let tmp = tmp_dir("import_indent");
        fs::write(tmp.join("extra.md"), "extra rules").unwrap();

        let content = "  @import extra.md";
        let result = resolve_imports(content, &tmp);
        assert!(result.contains("extra rules"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_imports_rejects_absolute_path() {
        let tmp = tmp_dir("import_abs");
        // Even if /etc/passwd exists on the host, an absolute path must be
        // rejected outright so a hostile CLAUDE.md cannot exfiltrate it.
        let content = "before\n@import /etc/passwd\nafter";
        let result = resolve_imports(content, &tmp);
        assert!(result.contains("before"));
        assert!(result.contains("after"));
        assert!(!result.contains("root:"));
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_imports_rejects_parent_traversal() {
        let outer = tmp_dir("import_outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).unwrap();
        fs::write(outer.join("secret.md"), "should not leak").unwrap();
        fs::write(inner.join("ok.md"), "fine").unwrap();

        let content = "@import ../secret.md\n@import ok.md";
        let result = resolve_imports(content, &inner);
        assert!(!result.contains("should not leak"));
        assert!(result.contains("fine"));
        let _ = fs::remove_dir_all(&outer);
    }

    #[test]
    fn resolve_imports_drops_oversized_file() {
        let tmp = tmp_dir("import_huge");
        let big = "X".repeat((MAX_IMPORT_BYTES as usize) + 1);
        fs::write(tmp.join("big.md"), &big).unwrap();

        let content = "head\n@import big.md\ntail";
        let result = resolve_imports(content, &tmp);
        assert!(result.contains("head"));
        assert!(result.contains("tail"));
        assert!(!result.contains("XXXX"));
        let _ = fs::remove_dir_all(&tmp);
    }
}
