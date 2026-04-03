use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

/// Maximum allowed size for a SKILL.md file (64 KB).
pub const MAX_SKILL_SIZE: u64 = 64 * 1024;

/// Maximum length for a skill name.
pub const MAX_SKILL_NAME_LEN: usize = 64;

/// Permissions a skill can request.
///
/// Skills are text-only (SKILL.md), but the text influences the LLM.
/// Permissions declare *intent* so the user can make informed decisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Permission {
    /// Can reference project files in context.
    ContextFiles,
    /// Can reference git history and status.
    ContextGit,
    /// May suggest code edits to project files.
    SuggestEdits,
    /// May suggest shell commands to execute.
    SuggestCommands,
    /// May suggest HTTP requests or network operations.
    SuggestNetwork,
}

impl Permission {
    #[cfg(test)]
    pub fn risk_level(self) -> RiskLevel {
        match self {
            Self::ContextFiles | Self::ContextGit => RiskLevel::Low,
            Self::SuggestEdits => RiskLevel::Medium,
            Self::SuggestCommands | Self::SuggestNetwork => RiskLevel::High,
        }
    }

    /// Permissions granted at a given trust level.
    pub fn for_trust_level(level: TrustLevel) -> BTreeSet<Permission> {
        match level {
            TrustLevel::Sandboxed => [Self::ContextFiles].into_iter().collect(),
            TrustLevel::Standard => [Self::ContextFiles, Self::ContextGit, Self::SuggestEdits]
                .into_iter()
                .collect(),
            TrustLevel::Trusted => [
                Self::ContextFiles,
                Self::ContextGit,
                Self::SuggestEdits,
                Self::SuggestCommands,
                Self::SuggestNetwork,
            ]
            .into_iter()
            .collect(),
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ContextFiles => write!(f, "context:files"),
            Self::ContextGit => write!(f, "context:git"),
            Self::SuggestEdits => write!(f, "suggest:edits"),
            Self::SuggestCommands => write!(f, "suggest:commands"),
            Self::SuggestNetwork => write!(f, "suggest:network"),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

#[cfg(test)]
impl fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => write!(f, "low"),
            Self::Medium => write!(f, "medium"),
            Self::High => write!(f, "HIGH"),
        }
    }
}

/// Trust level assigned to an installed skill.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrustLevel {
    /// Minimal permissions — context:files only.
    #[default]
    Sandboxed,
    /// Standard permissions — context + suggest:edits.
    Standard,
    /// Full permissions — all capabilities.
    Trusted,
}

impl fmt::Display for TrustLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sandboxed => write!(f, "sandboxed"),
            Self::Standard => write!(f, "standard"),
            Self::Trusted => write!(f, "trusted"),
        }
    }
}

/// Parsed content of a SKILL.md file (frontmatter + body).
#[derive(Debug, Clone)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub requested_permissions: BTreeSet<Permission>,
    pub content: String,
    /// Optional domain taxonomy from `metadata.domain` (e.g. "task", "review", "ops", "orchestration").
    pub domain: Option<String>,
    /// Trigger phrases from the `trigger` frontmatter field (pipe-delimited).
    /// Used to help agents identify when this skill is relevant.
    pub triggers: Vec<String>,
}

/// A skill that has been installed and its permissions resolved.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledSkill {
    pub name: String,
    pub source: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub installed_at: String,
    pub trust: TrustLevel,
    pub granted_permissions: BTreeSet<Permission>,
    pub denied_permissions: BTreeSet<Permission>,
}

/// Source reference for a skill on GitHub.
#[derive(Debug, Clone)]
pub struct SkillSource {
    pub owner: String,
    pub repo: String,
    pub path: Option<String>,
}

impl SkillSource {
    pub fn raw_url(&self) -> String {
        let default_path = format!("skills/{}", self.inferred_skill_name());
        let path = self.path.as_deref().unwrap_or(&default_path);
        format!(
            "https://raw.githubusercontent.com/{}/{}/main/{}/SKILL.md",
            self.owner, self.repo, path
        )
    }

    pub fn display_source(&self) -> String {
        match &self.path {
            Some(p) => format!("{}/{}/{}", self.owner, self.repo, p),
            None => format!("{}/{}", self.owner, self.repo),
        }
    }

    fn inferred_skill_name(&self) -> String {
        self.repo.clone()
    }
}

impl fmt::Display for SkillSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.display_source())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_display() {
        assert_eq!(Permission::ContextFiles.to_string(), "context:files");
        assert_eq!(Permission::ContextGit.to_string(), "context:git");
        assert_eq!(Permission::SuggestEdits.to_string(), "suggest:edits");
        assert_eq!(Permission::SuggestCommands.to_string(), "suggest:commands");
        assert_eq!(Permission::SuggestNetwork.to_string(), "suggest:network");
    }

    #[test]
    fn permission_risk_levels() {
        assert_eq!(Permission::ContextFiles.risk_level(), RiskLevel::Low);
        assert_eq!(Permission::ContextGit.risk_level(), RiskLevel::Low);
        assert_eq!(Permission::SuggestEdits.risk_level(), RiskLevel::Medium);
        assert_eq!(Permission::SuggestCommands.risk_level(), RiskLevel::High);
        assert_eq!(Permission::SuggestNetwork.risk_level(), RiskLevel::High);
    }

    #[test]
    fn permission_for_trust_sandboxed() {
        let perms = Permission::for_trust_level(TrustLevel::Sandboxed);
        assert!(perms.contains(&Permission::ContextFiles));
        assert!(!perms.contains(&Permission::SuggestEdits));
        assert!(!perms.contains(&Permission::SuggestCommands));
    }

    #[test]
    fn permission_for_trust_standard() {
        let perms = Permission::for_trust_level(TrustLevel::Standard);
        assert!(perms.contains(&Permission::ContextFiles));
        assert!(perms.contains(&Permission::ContextGit));
        assert!(perms.contains(&Permission::SuggestEdits));
        assert!(!perms.contains(&Permission::SuggestCommands));
    }

    #[test]
    fn permission_for_trust_trusted() {
        let perms = Permission::for_trust_level(TrustLevel::Trusted);
        assert_eq!(perms.len(), 5);
        assert!(perms.contains(&Permission::SuggestCommands));
        assert!(perms.contains(&Permission::SuggestNetwork));
    }

    #[test]
    fn trust_level_default() {
        assert_eq!(TrustLevel::default(), TrustLevel::Sandboxed);
    }

    #[test]
    fn trust_level_display() {
        assert_eq!(TrustLevel::Sandboxed.to_string(), "sandboxed");
        assert_eq!(TrustLevel::Standard.to_string(), "standard");
        assert_eq!(TrustLevel::Trusted.to_string(), "trusted");
    }

    #[test]
    fn risk_level_display() {
        assert_eq!(RiskLevel::Low.to_string(), "low");
        assert_eq!(RiskLevel::Medium.to_string(), "medium");
        assert_eq!(RiskLevel::High.to_string(), "HIGH");
    }

    #[test]
    fn skill_source_raw_url_simple() {
        let source = SkillSource {
            owner: "acme".to_string(),
            repo: "my-skill".to_string(),
            path: None,
        };
        assert_eq!(
            source.raw_url(),
            "https://raw.githubusercontent.com/acme/my-skill/main/skills/my-skill/SKILL.md"
        );
    }

    #[test]
    fn skill_source_raw_url_with_path() {
        let source = SkillSource {
            owner: "acme".to_string(),
            repo: "skills-repo".to_string(),
            path: Some("skills/rust".to_string()),
        };
        assert_eq!(
            source.raw_url(),
            "https://raw.githubusercontent.com/acme/skills-repo/main/skills/rust/SKILL.md"
        );
    }

    #[test]
    fn skill_source_display() {
        let source = SkillSource {
            owner: "acme".to_string(),
            repo: "my-skill".to_string(),
            path: None,
        };
        assert_eq!(source.to_string(), "acme/my-skill");

        let source_with_path = SkillSource {
            owner: "acme".to_string(),
            repo: "repo".to_string(),
            path: Some("path/to/skill".to_string()),
        };
        assert_eq!(source_with_path.to_string(), "acme/repo/path/to/skill");
    }
}
