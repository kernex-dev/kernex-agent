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
    #[allow(dead_code)]
    pub fn description(self) -> &'static str {
        match self {
            Self::ContextFiles => "Reference project files",
            Self::ContextGit => "Reference git history/status",
            Self::SuggestEdits => "Suggest code modifications",
            Self::SuggestCommands => "Suggest shell commands",
            Self::SuggestNetwork => "Suggest network requests",
        }
    }

    #[allow(dead_code)]
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

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

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
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub requested_permissions: BTreeSet<Permission>,
    pub content: String,
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
