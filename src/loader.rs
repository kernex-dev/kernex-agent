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

/// Expand `@import path/to/file.md` directives in `content`.
///
/// Lines that start with `@import ` (after trimming) are replaced with the
/// contents of the referenced file. Paths are resolved relative to `base_dir`.
/// Missing imports are silently dropped.
fn resolve_imports(content: &str, base_dir: &Path) -> String {
    let mut out: Vec<String> = Vec::with_capacity(content.lines().count());
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("@import ") {
            let import_path = base_dir.join(rest.trim());
            if let Ok(imported) = std::fs::read_to_string(&import_path) {
                out.push(imported);
            }
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
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
}
