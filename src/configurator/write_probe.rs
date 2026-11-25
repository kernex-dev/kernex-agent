//! May `kx install` write a file the user approved?
//!
//! The installer writes into another agent host's configuration (for
//! example `~/.claude/CLAUDE.md`) on the user's say-so, so it is not held
//! to the agent sandbox's write roots. It still refuses the locations the
//! sandbox protects for every caller: system directories, kx's own state
//! (`~/.kx` memory, config, skills and projects) and the toolchain programs.

use std::path::{Component, Path, PathBuf};

/// Device and kernel pseudo-filesystems. kernex 0.12 no longer lists them
/// in its system write-deny set, and the probe below grants every ancestor
/// of the target, so they are refused here.
const PSEUDO_FS: &[&str] = &["/dev", "/proc", "/sys"];

/// Whether the sandbox policy refuses a write to `path` once the path's
/// own directories are granted. `data_dir` is the kx root (`~/.kx`).
pub(crate) fn install_write_blocked(path: &Path, data_dir: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return true;
    };
    let resolved = resolve_existing(path);
    if path.components().any(|c| c == Component::ParentDir)
        || PSEUDO_FS
            .iter()
            .any(|root| path.starts_with(root) || resolved.starts_with(root))
    {
        return true;
    }
    // The path and each directory above it, so a credential or agent-CLI
    // directory on the way (`~/.claude`) counts as granted. The policy
    // drops entries that contain the home directory.
    let granted: Vec<PathBuf> = path.ancestors().map(Path::to_path_buf).collect();
    let scope = kernex_sandbox::SandboxScope::new(parent, data_dir);
    let profile = kernex_sandbox::SandboxProfile::new().with_allowed_paths(granted);
    kernex_sandbox::is_write_blocked(path, &scope, &profile)
}

/// `path` with its deepest existing ancestor resolved, so a symlink on the
/// way (`link -> /dev`) is judged by where it leads.
fn resolve_existing(path: &Path) -> PathBuf {
    for ancestor in path.ancestors() {
        if let Ok(real) = std::fs::canonicalize(ancestor) {
            let rest = path.strip_prefix(ancestor).unwrap_or(Path::new(""));
            return real.join(rest);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_pseudo_filesystem_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let link = tmp.path().join("devlink");
        std::os::unix::fs::symlink("/dev", &link).unwrap();
        assert!(install_write_blocked(
            &link.join("kxprobe"),
            &tmp.path().join(".kx")
        ));
    }

    #[test]
    fn host_config_and_backups_are_writable() {
        let home = tempfile::tempdir().unwrap();
        let kx = home.path().join(".kx");
        for target in [
            home.path().join(".claude/CLAUDE.md"),
            home.path().join(".codex/config.toml"),
            home.path().join("project/AGENTS.md"),
            kx.join("backups/2026-01-01T00-00-00Z-claude-code.tar.gz"),
        ] {
            assert!(
                !install_write_blocked(&target, &kx),
                "{} should be writable",
                target.display()
            );
        }
    }

    /// Under the real home `~/.claude` is agent-CLI state the sandbox closes
    /// to tools; the installer, acting for the user, may still write it.
    #[test]
    fn agent_host_config_under_the_real_home_is_writable() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let kx = home.join(".kx");
        assert!(!install_write_blocked(&home.join(".claude/CLAUDE.md"), &kx));
        assert!(!install_write_blocked(
            &home.join(".codex/config.toml"),
            &kx
        ));
        assert!(install_write_blocked(&kx.join("config.toml"), &kx));
    }

    #[test]
    fn kx_state_and_system_directories_are_refused() {
        let home = tempfile::tempdir().unwrap();
        let kx = home.path().join(".kx");
        for target in [
            kx.join("config.toml"),
            kx.join("memory.db"),
            kx.join("projects/demo/memory.db"),
            kx.join("projects/demo/skills/x/SKILL.md"),
            PathBuf::from("/etc/kx.conf"),
            PathBuf::from("/usr/bin/kx"),
        ] {
            assert!(
                install_write_blocked(&target, &kx),
                "{} should be refused",
                target.display()
            );
        }
        assert!(install_write_blocked(Path::new("relative/file"), &kx));
    }

    #[test]
    fn device_and_kernel_filesystems_are_refused() {
        let home = tempfile::tempdir().unwrap();
        let kx = home.path().join(".kx");
        for target in [
            "/dev/sda",
            "/dev/shm/kx",
            "/proc/self/mem",
            "/sys/kernel/x",
            "/tmp/../dev/null",
        ] {
            assert!(
                install_write_blocked(Path::new(target), &kx),
                "{target} should be refused"
            );
        }
        assert!(!install_write_blocked(&home.path().join("devices.md"), &kx));
    }
}
