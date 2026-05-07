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
        std::fs::write(&path, content)?;
        // skills.toml is the integrity root for installed skills (each entry
        // pins a SHA-256). Lock it down to 0o600 on Unix; on shared hosts
        // the default 0o644 would let any local user rewrite the file's
        // sha256 fields and pass `kx skills verify` against tampered
        // SKILL.md content.
        tighten_perms(&path);
        Ok(())
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

#[cfg(unix)]
fn tighten_perms(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    if let Ok(meta) = std::fs::metadata(path) {
        let mut perms = meta.permissions();
        perms.set_mode(0o600);
        if let Err(e) = std::fs::set_permissions(path, perms) {
            tracing::warn!(path = %path.display(), "could not chmod 0600 on skills.toml: {e}");
        }
    }
}

#[cfg(not(unix))]
fn tighten_perms(_path: &Path) {}

/// Returns the path to a skill's SKILL.md file, or `None` if the name does
/// not pass [`crate::skills::parser::validate_skill_name`].
///
/// Validation is enforced here as defence-in-depth: a hostile or corrupted
/// `skills.toml` entry whose name contains `..`, `/`, or `\` cannot escape
/// the `{data_dir}/skills/` subtree even if a caller forgets to validate
/// upstream. Callers should treat `None` as "skip this skill" and log.
pub fn skill_file_path(data_dir: &Path, skill_name: &str) -> Option<PathBuf> {
    if crate::skills::parser::validate_skill_name(skill_name).is_err() {
        return None;
    }
    Some(data_dir.join("skills").join(skill_name).join("SKILL.md"))
}

pub fn compute_sha256(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let result = hasher.finalize();
    result.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn verify_skill(data_dir: &Path, skill: &InstalledSkill) -> VerifyResult {
    let Some(path) = skill_file_path(data_dir, &skill.name) else {
        // Manifest entry has an invalid name — treat as missing rather than
        // letting the path-traversal attempt reach the filesystem.
        return VerifyResult::Missing;
    };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::types::{Permission, TrustLevel};
    use std::collections::BTreeSet;

    fn make_skill(name: &str, sha: &str) -> InstalledSkill {
        InstalledSkill {
            name: name.to_string(),
            source: format!("test/{name}"),
            sha256: sha.to_string(),
            size_bytes: 1024,
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            trust: TrustLevel::Sandboxed,
            granted_permissions: BTreeSet::new(),
            denied_permissions: BTreeSet::new(),
        }
    }

    #[test]
    fn manifest_default_is_empty() {
        let manifest = SkillsManifest::default();
        assert!(manifest.skills.is_empty());
    }

    #[test]
    fn manifest_add_new_skill() {
        let mut manifest = SkillsManifest::default();
        let skill = make_skill("test-skill", "abc123");
        manifest.add(skill);
        assert_eq!(manifest.skills.len(), 1);
        assert_eq!(manifest.skills[0].name, "test-skill");
    }

    #[test]
    fn manifest_add_replaces_existing() {
        let mut manifest = SkillsManifest::default();
        manifest.add(make_skill("test-skill", "abc123"));
        manifest.add(make_skill("test-skill", "def456"));
        assert_eq!(manifest.skills.len(), 1);
        assert_eq!(manifest.skills[0].sha256, "def456");
    }

    #[test]
    fn manifest_remove() {
        let mut manifest = SkillsManifest::default();
        manifest.add(make_skill("skill-a", "aaa"));
        manifest.add(make_skill("skill-b", "bbb"));

        let removed = manifest.remove("skill-a");
        assert!(removed);
        assert_eq!(manifest.skills.len(), 1);
        assert!(manifest.find("skill-a").is_none());
        assert!(manifest.find("skill-b").is_some());
    }

    #[test]
    fn manifest_remove_nonexistent() {
        let mut manifest = SkillsManifest::default();
        manifest.add(make_skill("skill-a", "aaa"));

        let removed = manifest.remove("nonexistent");
        assert!(!removed);
        assert_eq!(manifest.skills.len(), 1);
    }

    #[test]
    fn manifest_find() {
        let mut manifest = SkillsManifest::default();
        manifest.add(make_skill("skill-a", "aaa"));

        let found = manifest.find("skill-a");
        assert!(found.is_some());
        assert_eq!(found.unwrap().sha256, "aaa");

        assert!(manifest.find("nonexistent").is_none());
    }

    #[test]
    fn manifest_list() {
        let mut manifest = SkillsManifest::default();
        manifest.add(make_skill("skill-a", "aaa"));
        manifest.add(make_skill("skill-b", "bbb"));

        let list = manifest.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn compute_sha256_consistent() {
        let content = b"Hello, World!";
        let hash1 = compute_sha256(content);
        let hash2 = compute_sha256(content);
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA256 is 64 hex chars
    }

    #[test]
    fn compute_sha256_different_content() {
        let hash1 = compute_sha256(b"Hello");
        let hash2 = compute_sha256(b"World");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn skill_dir_path() {
        let data_dir = Path::new("/home/user/.kx");
        let dir = skill_dir(data_dir);
        assert_eq!(dir, PathBuf::from("/home/user/.kx/skills"));
    }

    #[test]
    fn skill_file_path_correct() {
        let data_dir = Path::new("/home/user/.kx");
        let path = skill_file_path(data_dir, "my-skill").unwrap();
        assert_eq!(
            path,
            PathBuf::from("/home/user/.kx/skills/my-skill/SKILL.md")
        );
    }

    #[test]
    fn skill_file_path_rejects_traversal() {
        let data_dir = Path::new("/home/user/.kx");
        assert!(skill_file_path(data_dir, "../../etc/passwd").is_none());
        assert!(skill_file_path(data_dir, "..").is_none());
        assert!(skill_file_path(data_dir, "foo/bar").is_none());
        assert!(skill_file_path(data_dir, "foo\\bar").is_none());
        assert!(skill_file_path(data_dir, "").is_none());
    }

    #[test]
    fn manifest_save_and_load() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path();

        let mut manifest = SkillsManifest::default();
        let mut perms = BTreeSet::new();
        perms.insert(Permission::ContextFiles);
        manifest.add(InstalledSkill {
            name: "test-skill".to_string(),
            source: "acme/test".to_string(),
            sha256: "abc123".to_string(),
            size_bytes: 512,
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            trust: TrustLevel::Standard,
            granted_permissions: perms,
            denied_permissions: BTreeSet::new(),
        });

        manifest.save(tmp).unwrap();
        let loaded = SkillsManifest::load(tmp);
        assert_eq!(loaded.skills.len(), 1);
        assert_eq!(loaded.skills[0].name, "test-skill");
        assert_eq!(loaded.skills[0].trust, TrustLevel::Standard);
    }

    #[test]
    fn manifest_load_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path();

        let manifest = SkillsManifest::load(tmp);
        assert!(manifest.skills.is_empty());
    }

    #[test]
    fn verify_skill_ok() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path();
        let skill_path = tmp.join("skills").join("test-skill");
        std::fs::create_dir_all(&skill_path).unwrap();

        let content = b"# Test Skill\nSome content";
        let sha = compute_sha256(content);
        std::fs::write(skill_path.join("SKILL.md"), content).unwrap();

        let skill = InstalledSkill {
            name: "test-skill".to_string(),
            source: "test/test-skill".to_string(),
            sha256: sha,
            size_bytes: content.len() as u64,
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            trust: TrustLevel::Sandboxed,
            granted_permissions: BTreeSet::new(),
            denied_permissions: BTreeSet::new(),
        };

        match verify_skill(tmp, &skill) {
            VerifyResult::Ok => {}
            other => panic!("Expected Ok, got {:?}", other),
        }
    }

    #[test]
    fn verify_skill_modified() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path();
        let skill_path = tmp.join("skills").join("mod-skill");
        std::fs::create_dir_all(&skill_path).unwrap();

        std::fs::write(skill_path.join("SKILL.md"), b"Modified content").unwrap();

        let skill = InstalledSkill {
            name: "mod-skill".to_string(),
            source: "test/mod-skill".to_string(),
            sha256: "original_hash_that_doesnt_match".to_string(),
            size_bytes: 100,
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            trust: TrustLevel::Sandboxed,
            granted_permissions: BTreeSet::new(),
            denied_permissions: BTreeSet::new(),
        };

        match verify_skill(tmp, &skill) {
            VerifyResult::Modified { .. } => {}
            other => panic!("Expected Modified, got {:?}", other),
        }
    }

    #[test]
    fn verify_skill_missing() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path();

        let skill = InstalledSkill {
            name: "missing-skill".to_string(),
            source: "test/missing".to_string(),
            sha256: "some_hash".to_string(),
            size_bytes: 100,
            installed_at: "2026-01-01T00:00:00Z".to_string(),
            trust: TrustLevel::Sandboxed,
            granted_permissions: BTreeSet::new(),
            denied_permissions: BTreeSet::new(),
        };

        match verify_skill(tmp, &skill) {
            VerifyResult::Missing => {}
            other => panic!("Expected Missing, got {:?}", other),
        }
    }
}
