use std::path::Path;

use serde::Deserialize;

use crate::skills::permissions::PermissionPolicy;
use crate::skills::types::TrustLevel;
use crate::stack::Stack;

#[derive(Debug, Default, Deserialize)]
pub struct ProjectConfig {
    pub stack: Option<String>,
    pub system_prompt: Option<String>,
    pub provider: Option<ProviderConfig>,
    pub skills: Option<SkillsConfig>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SkillsConfig {
    /// Default trust level for newly installed skills (sandboxed, standard, trusted)
    pub default_trust: Option<String>,
    /// Sources that are automatically trusted
    #[serde(default)]
    pub trusted_sources: Vec<String>,
    /// Skill names to block from being loaded
    #[serde(default)]
    pub blocked: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct ProviderConfig {
    pub max_turns: Option<u32>,
    pub timeout_secs: Option<u64>,
    pub model: Option<String>,
}

impl ProjectConfig {
    pub fn load(project_dir: &Path) -> Self {
        let path = project_dir.join(".kx.toml");
        if !path.exists() {
            return Self::default();
        }

        match std::fs::read_to_string(&path) {
            Ok(content) => toml::from_str(&content).unwrap_or_else(|e| {
                eprintln!("warn: failed to parse .kx.toml: {e}");
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn skills_policy(&self) -> PermissionPolicy {
        let skills = match &self.skills {
            Some(s) => s,
            None => return PermissionPolicy::default(),
        };

        let default_trust = match skills.default_trust.as_deref() {
            Some("sandboxed") => TrustLevel::Sandboxed,
            Some("standard") => TrustLevel::Standard,
            Some("trusted") => TrustLevel::Trusted,
            _ => TrustLevel::Sandboxed,
        };

        PermissionPolicy {
            default_trust,
            trusted_sources: skills.trusted_sources.clone(),
            blocked_skills: skills.blocked.clone(),
            overrides: std::collections::HashMap::new(),
        }
    }

    pub fn resolve_stack(&self, detected: Stack) -> Stack {
        match self.stack.as_deref() {
            Some("rust") => Stack::Rust,
            Some("node" | "javascript" | "typescript") => Stack::Node,
            Some("python") => Stack::Python,
            Some("flutter" | "dart") => Stack::Flutter,
            Some("php") => Stack::Php,
            _ => detected,
        }
    }
}
