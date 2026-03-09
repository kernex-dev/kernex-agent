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
];

pub fn install_builtin_skills(data_dir: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let skills_dir = data_dir.join("skills");
    let mut manifest = SkillsManifest::load(data_dir);
    let mut installed = 0;
    let now = chrono_now();

    for skill in BUILTIN_SKILLS {
        let skill_dir = skills_dir.join(skill.name);
        std::fs::create_dir_all(&skill_dir)?;
        let skill_path = skill_dir.join("SKILL.md");
        std::fs::write(&skill_path, skill.content)?;

        let sha = compute_sha256(skill.content.as_bytes());
        let granted = Permission::for_trust_level(TrustLevel::Trusted);

        manifest.add(InstalledSkill {
            name: skill.name.to_string(),
            source: format!("builtin/{}", skill.name),
            sha256: sha,
            size_bytes: skill.content.len() as u64,
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
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{secs}")
}

#[allow(dead_code)]
pub fn builtin_count() -> usize {
    BUILTIN_SKILLS.len()
}

#[allow(dead_code)]
pub fn builtin_names() -> Vec<&'static str> {
    BUILTIN_SKILLS.iter().map(|s| s.name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn builtin_count_is_12() {
        assert_eq!(builtin_count(), 12);
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
        let tmp = std::env::temp_dir().join("__kx_builtins_test__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let count = install_builtin_skills(&tmp).unwrap();
        assert_eq!(count, 12);

        for skill in BUILTIN_SKILLS {
            let path = tmp.join("skills").join(skill.name).join("SKILL.md");
            assert!(path.exists(), "Missing: {}", path.display());
        }

        let manifest = SkillsManifest::load(&tmp);
        assert_eq!(manifest.list().len(), 12);

        let senior = manifest.find("senior-developer");
        assert!(senior.is_some());
        let senior = senior.unwrap();
        assert_eq!(senior.trust, TrustLevel::Trusted);
        assert!(senior.source.starts_with("builtin/"));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn builtins_contain_valid_frontmatter() {
        for skill in BUILTIN_SKILLS {
            assert!(
                skill.content.starts_with("---"),
                "Skill '{}' missing frontmatter delimiter",
                skill.name
            );
            assert!(
                skill
                    .content
                    .contains(&format!("name = \"{}\"", skill.name)),
                "Skill '{}' frontmatter name mismatch",
                skill.name
            );
        }
    }
}
