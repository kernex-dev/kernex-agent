//! Gate on skills that make the runtime start a process.
//!
//! A `SKILL.md` can declare MCP servers and toolboxes. The kernex runtime
//! loads every skill under `{data_dir}/skills` itself and starts those
//! commands when a trigger matches; it does not consult kx's manifest,
//! trust levels or hashes. Without this gate a skill installed at the
//! default `sandboxed` trust, or a `SKILL.md` edited after install, could
//! make kx launch a command the user never approved.
//!
//! The gate asks the runtime's own loader what it would start, so the two
//! cannot disagree about what a file declares.

use std::path::{Path, PathBuf};

use kernex_runtime::skills::load_skills;

use super::manifest::{compute_sha256, SkillsManifest};
use super::types::TrustLevel;

/// Sidecar files the runtime merges into a skill's declarations. They are
/// not covered by the manifest hash, so their presence alone is refused.
const SIDECAR_FILES: [&str; 2] = ["mcp.json", "toolbox.json"];

/// Commands the runtime would start for the skills under `data_dir`, one
/// line per declaration, keyed by the skill's directory name.
fn declared_commands(data_dir: &Path) -> Vec<(String, PathBuf, Vec<String>)> {
    let mut out = Vec::new();
    for skill in load_skills(&data_dir.to_string_lossy()) {
        let mut commands = Vec::new();
        for server in &skill.mcp_servers {
            commands.push(format!(
                "MCP server '{}': {} {}",
                server.name,
                server.command,
                server.args.join(" ")
            ));
        }
        for toolbox in &skill.toolboxes {
            commands.push(format!(
                "toolbox '{}': {} {}",
                toolbox.name,
                toolbox.command,
                toolbox.args.join(" ")
            ));
        }
        let dir = skill
            .path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        for sidecar in SIDECAR_FILES {
            if dir.join(sidecar).exists() {
                commands.push(format!("sidecar file {sidecar}"));
            }
        }
        if commands.is_empty() {
            continue;
        }
        let dir_name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| skill.name.clone());
        out.push((dir_name, skill.path.clone(), commands));
    }
    out
}

/// What the runtime would start if `content` were installed as a skill.
///
/// Writes the content to a private scratch directory and runs the runtime
/// loader over it, so nothing is judged by a second parser.
pub fn commands_declared_by(content: &[u8]) -> Result<Vec<String>, String> {
    let scratch = scratch_dir()?;
    let skill_dir = scratch.join("skills").join("candidate");
    let result = std::fs::create_dir_all(&skill_dir)
        .and_then(|()| std::fs::write(skill_dir.join("SKILL.md"), content))
        .map(|()| {
            declared_commands(&scratch)
                .into_iter()
                .flat_map(|(_, _, commands)| commands)
                .collect()
        })
        .map_err(|e| format!("failed to inspect skill: {e}"));
    let _ = std::fs::remove_dir_all(&scratch);
    result
}

fn scratch_dir() -> Result<PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("kx-skill-check-{}-{nanos}", std::process::id()));
    std::fs::create_dir(&dir).map_err(|e| format!("failed to inspect skill: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    Ok(dir)
}

/// Refuse to start a runtime while an installed skill would launch a
/// command that the manifest does not vouch for.
///
/// A skill may declare commands only when its manifest entry is `trusted`
/// and the file still has the hash recorded at install. Builtins satisfy
/// both. Anything else (lower trust, edited file, unlisted directory,
/// sidecar file) is reported with the way out.
pub fn check_installed(data_dir: &Path) -> Result<(), String> {
    let manifest = SkillsManifest::load(data_dir);
    let mut problems = Vec::new();

    for (dir_name, path, commands) in declared_commands(data_dir) {
        let reason = match manifest.find(&dir_name) {
            None => Some("it is not in the skills manifest".to_string()),
            Some(entry) if entry.trust != TrustLevel::Trusted => {
                Some(format!("its trust level is {}", entry.trust))
            }
            Some(entry) => match std::fs::read(&path) {
                Ok(bytes) if compute_sha256(&bytes) == entry.sha256 => None,
                Ok(_) => Some("its SKILL.md changed after install".to_string()),
                Err(e) => Some(format!("its SKILL.md could not be read: {e}")),
            },
        };
        let sidecar = commands.iter().any(|c| c.starts_with("sidecar file"));
        let reason = match (reason, sidecar) {
            (Some(r), _) => Some(r),
            (None, true) => Some("it has an mcp.json or toolbox.json sidecar".to_string()),
            (None, false) => None,
        };
        if let Some(reason) = reason {
            problems.push(format!(
                "  skill '{dir_name}' would start a command, and {reason}:\n    {}",
                commands.join("\n    ")
            ));
        }
    }

    if problems.is_empty() {
        return Ok(());
    }
    Err(format!(
        "refusing to start: an installed skill declares a command kx has not been told to trust.\n{}\n\
         Remove it with `kx skills remove <name>`, or reinstall it with \
         `kx skills add <source> --trust trusted --force` after reading what it runs.",
        problems.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::{InstalledSkill, Permission};
    use std::collections::BTreeSet;

    const PLAIN: &str = "---\nname: plain\ndescription: Just instructions.\n---\n\nBody.\n";
    const WITH_MCP: &str = "---\nname: helper\ndescription: Looks harmless.\ntrigger: the\nmcp-x: npx -y some-package\n---\n\nBody.\n";
    const WITH_TOOLBOX: &str = "---\nname = \"tb\"\ndescription = \"Has a tool.\"\n\n[toolbox.run]\ndescription = \"Run it\"\ncommand = \"python3\"\nargs = [\"scripts/x.py\"]\n---\n\nBody.\n";

    fn install(data_dir: &Path, name: &str, content: &str, trust: TrustLevel) {
        let dir = data_dir.join("skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), content).unwrap();
        let mut manifest = SkillsManifest::load(data_dir);
        manifest.add(InstalledSkill {
            name: name.to_string(),
            source: format!("someone/{name}"),
            sha256: compute_sha256(content.as_bytes()),
            size_bytes: content.len() as u64,
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            trust,
            granted_permissions: Permission::for_trust_level(trust),
            denied_permissions: BTreeSet::new(),
            requested_ref: None,
            resolved_commit: None,
        });
        manifest.save(data_dir).unwrap();
    }

    #[test]
    fn plain_skill_declares_nothing() {
        assert!(commands_declared_by(PLAIN.as_bytes()).unwrap().is_empty());
    }

    #[test]
    fn mcp_frontmatter_key_is_detected() {
        let commands = commands_declared_by(WITH_MCP.as_bytes()).unwrap();
        assert_eq!(commands.len(), 1, "{commands:?}");
        assert!(commands[0].contains("npx -y some-package"), "{commands:?}");
    }

    #[test]
    fn toolbox_table_is_detected() {
        let commands = commands_declared_by(WITH_TOOLBOX.as_bytes()).unwrap();
        assert_eq!(commands.len(), 1, "{commands:?}");
        assert!(commands[0].contains("python3"), "{commands:?}");
    }

    #[test]
    fn sandboxed_skill_with_a_command_blocks_startup() {
        let tmp = tempfile::tempdir().unwrap();
        install(tmp.path(), "helper", WITH_MCP, TrustLevel::Sandboxed);
        let err = check_installed(tmp.path()).unwrap_err();
        assert!(err.contains("helper"), "{err}");
        assert!(err.contains("trust level"), "{err}");
    }

    #[test]
    fn trusted_unmodified_skill_is_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        install(tmp.path(), "helper", WITH_MCP, TrustLevel::Trusted);
        install(tmp.path(), "plain", PLAIN, TrustLevel::Sandboxed);
        assert!(check_installed(tmp.path()).is_ok());
    }

    #[test]
    fn skill_edited_after_install_blocks_startup() {
        let tmp = tempfile::tempdir().unwrap();
        install(tmp.path(), "plain", PLAIN, TrustLevel::Trusted);
        let edited = PLAIN.replace("description:", "mcp-x: npx -y other\ndescription:");
        std::fs::write(tmp.path().join("skills/plain/SKILL.md"), edited).unwrap();
        let err = check_installed(tmp.path()).unwrap_err();
        assert!(err.contains("changed after install"), "{err}");
    }

    #[test]
    fn unlisted_skill_directory_blocks_startup() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("skills").join("planted");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), WITH_MCP).unwrap();
        let err = check_installed(tmp.path()).unwrap_err();
        assert!(err.contains("not in the skills manifest"), "{err}");
    }

    #[test]
    fn builtins_pass_the_gate() {
        let tmp = tempfile::tempdir().unwrap();
        crate::builtins::install_builtin_skills(tmp.path()).unwrap();
        assert!(check_installed(tmp.path()).is_ok());
    }

    #[test]
    fn no_skills_directory_is_fine() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(check_installed(tmp.path()).is_ok());
    }
}
