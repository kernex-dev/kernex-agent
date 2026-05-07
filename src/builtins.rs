use std::collections::BTreeSet;
use std::path::Path;

use crate::skills::manifest::{compute_sha256, SkillsManifest};
use crate::skills::types::{InstalledSkill, Permission, TrustLevel};

struct BuiltinSkill {
    name: &'static str,
    content: &'static str,
}

const BUILTIN_SKILLS: &[BuiltinSkill] = &[
    BuiltinSkill {
        name: "accessibility-auditor",
        content: include_str!("../builtins/accessibility-auditor/SKILL.md"),
    },
    BuiltinSkill {
        name: "agents-orchestrator",
        content: include_str!("../builtins/agents-orchestrator/SKILL.md"),
    },
    BuiltinSkill {
        name: "ai-engineer",
        content: include_str!("../builtins/ai-engineer/SKILL.md"),
    },
    BuiltinSkill {
        name: "api-tester",
        content: include_str!("../builtins/api-tester/SKILL.md"),
    },
    BuiltinSkill {
        name: "backend-architect",
        content: include_str!("../builtins/backend-architect/SKILL.md"),
    },
    BuiltinSkill {
        name: "devops-automator",
        content: include_str!("../builtins/devops-automator/SKILL.md"),
    },
    BuiltinSkill {
        name: "frontend-developer",
        content: include_str!("../builtins/frontend-developer/SKILL.md"),
    },
    BuiltinSkill {
        name: "performance-benchmarker",
        content: include_str!("../builtins/performance-benchmarker/SKILL.md"),
    },
    BuiltinSkill {
        name: "project-manager",
        content: include_str!("../builtins/project-manager/SKILL.md"),
    },
    BuiltinSkill {
        name: "reality-checker",
        content: include_str!("../builtins/reality-checker/SKILL.md"),
    },
    BuiltinSkill {
        name: "security-engineer",
        content: include_str!("../builtins/security-engineer/SKILL.md"),
    },
    BuiltinSkill {
        name: "senior-developer",
        content: include_str!("../builtins/senior-developer/SKILL.md"),
    },
    BuiltinSkill {
        name: "skill-factory",
        content: include_str!("../builtins/skill-factory/SKILL.md"),
    },
];

pub fn install_builtin_skills(data_dir: &Path) -> anyhow::Result<usize> {
    let skills_dir = data_dir.join("skills");
    let mut manifest = SkillsManifest::load(data_dir);
    let mut installed = 0;
    let now = chrono_now();

    for skill in BUILTIN_SKILLS {
        // Builtins ship as compiled-in `include_str!` content. We do NOT
        // re-fetch them from GitHub at install time: any maintainer of
        // kernex-dev's `main` branch could otherwise replace a builtin and
        // have every `kx init` blindly trust + install the new version
        // (TrustLevel::Trusted, no operator confirmation). Distributing
        // builtins via cargo gives us the build-time hash baked into the
        // binary and removes a silent supply-chain channel.
        let content = skill.content;
        let skill_dir = skills_dir.join(skill.name);
        std::fs::create_dir_all(&skill_dir)?;
        let skill_path = skill_dir.join("SKILL.md");
        std::fs::write(&skill_path, content.as_bytes())?;

        let sha = compute_sha256(content.as_bytes());
        let granted = Permission::for_trust_level(TrustLevel::Trusted);

        manifest.add(InstalledSkill {
            name: skill.name.to_string(),
            source: format!("builtin/{}", skill.name),
            sha256: sha,
            size_bytes: content.len() as u64,
            installed_at: now.clone(),
            trust: TrustLevel::Trusted,
            granted_permissions: granted,
            denied_permissions: BTreeSet::new(),
        });

        installed += 1;
    }

    manifest.save(data_dir)?;
    Ok(installed)
}

fn chrono_now() -> String {
    crate::utils::iso_timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin_count() -> usize {
        BUILTIN_SKILLS.len()
    }

    fn builtin_names() -> Vec<&'static str> {
        BUILTIN_SKILLS.iter().map(|s| s.name).collect()
    }

    #[test]
    fn all_builtins_have_content() {
        for skill in BUILTIN_SKILLS {
            assert!(
                !skill.content.is_empty(),
                "Builtin skill '{}' has empty content",
                skill.name
            );
        }
    }

    #[test]
    fn builtin_count_is_13() {
        assert_eq!(builtin_count(), 13);
    }

    #[test]
    fn builtin_names_match() {
        let names = builtin_names();
        assert!(names.contains(&"senior-developer"));
        assert!(names.contains(&"frontend-developer"));
        assert!(names.contains(&"security-engineer"));
        assert!(names.contains(&"ai-engineer"));
    }

    #[test]
    fn install_builtin_skills_creates_files_and_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path();

        let count = install_builtin_skills(tmp).unwrap();
        assert_eq!(count, 13);

        for skill in BUILTIN_SKILLS {
            let path = tmp.join("skills").join(skill.name).join("SKILL.md");
            assert!(path.exists(), "Missing: {}", path.display());
        }

        let manifest = SkillsManifest::load(tmp);
        assert_eq!(manifest.list().len(), 13);

        let senior = manifest.find("senior-developer");
        assert!(senior.is_some());
        let senior = senior.unwrap();
        assert_eq!(senior.trust, TrustLevel::Trusted);
        assert!(senior.source.starts_with("builtin/"));
    }

    #[test]
    fn builtins_contain_valid_frontmatter() {
        for skill in BUILTIN_SKILLS {
            assert!(
                skill.content.starts_with("---"),
                "Skill '{}' missing frontmatter delimiter",
                skill.name
            );
            // Accept both TOML-style (name = "...") and YAML-style (name: ...)
            let has_toml = skill
                .content
                .contains(&format!("name = \"{}\"", skill.name));
            let has_yaml = skill.content.contains(&format!("name: {}", skill.name));
            assert!(
                has_toml || has_yaml,
                "Skill '{}' frontmatter name mismatch",
                skill.name
            );
        }
    }
}
