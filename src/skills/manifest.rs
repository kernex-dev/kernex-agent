use std::path::{Path, PathBuf};

use colored::Colorize;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::types::InstalledSkill;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct SkillsManifest {
    #[serde(default)]
    pub skills: Vec<InstalledSkill>,
}

#[derive(Debug)]
pub enum VerifyResult {
    Ok,
    Modified { expected: String, actual: String },
    Missing,
}

impl SkillsManifest {
    pub fn load(data_dir: &Path) -> Self {
        let path = data_dir.join("skills.toml");
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!(
                    "{} failed to read {}: {}",
                    "warning:".yellow().bold(),
                    path.display(),
                    e
                );
                return Self::default();
            }
        };

        match toml::from_str(&content) {
            Ok(manifest) => manifest,
            Err(e) => {
                eprintln!(
                    "{} failed to parse {}: {}",
                    "warning:".yellow().bold(),
                    path.display(),
                    e
                );
                Self::default()
            }
        }
    }

    pub fn save(&self, data_dir: &Path) -> Result<(), std::io::Error> {
        let path = data_dir.join("skills.toml");
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&path, content)
    }

    pub fn add(&mut self, skill: InstalledSkill) {
        if let Some(existing) = self.skills.iter_mut().find(|s| s.name == skill.name) {
            *existing = skill;
        } else {
            self.skills.push(skill);
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let len_before = self.skills.len();
        self.skills.retain(|s| s.name != name);
        self.skills.len() < len_before
    }

    pub fn find(&self, name: &str) -> Option<&InstalledSkill> {
        self.skills.iter().find(|s| s.name == name)
    }

    pub fn list(&self) -> &[InstalledSkill] {
        &self.skills
    }
}

pub fn skill_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("skills")
}

/// Returns the path to a skill's SKILL.md file.
///
/// # Safety note
/// The caller MUST validate `skill_name` before calling this function.
/// An unvalidated name could allow path traversal (e.g. `../../etc/passwd`).
pub fn skill_file_path(data_dir: &Path, skill_name: &str) -> PathBuf {
    data_dir.join("skills").join(skill_name).join("SKILL.md")
}

pub fn compute_sha256(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let result = hasher.finalize();
    result.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn verify_skill(data_dir: &Path, skill: &InstalledSkill) -> VerifyResult {
    let path = skill_file_path(data_dir, &skill.name);
    let content = match std::fs::read(&path) {
        Ok(c) => c,
        Err(_) => return VerifyResult::Missing,
    };
    let actual = compute_sha256(&content);
    if actual == skill.sha256 {
        VerifyResult::Ok
    } else {
        VerifyResult::Modified {
            expected: skill.sha256.clone(),
            actual,
        }
    }
}
