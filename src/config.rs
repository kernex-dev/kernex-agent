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
            Some("go" | "golang") => Stack::Go,
            Some("java" | "kotlin") => Stack::Java,
            Some("swift" | "swiftui") => Stack::Swift,
            _ => detected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_default() {
        let config = ProjectConfig::default();
        assert!(config.stack.is_none());
        assert!(config.system_prompt.is_none());
        assert!(config.provider.is_none());
        assert!(config.skills.is_none());
    }

    #[test]
    fn config_load_nonexistent() {
        let tmp = std::env::temp_dir().join("__kx_config_missing__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let config = ProjectConfig::load(&tmp);
        assert!(config.stack.is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn config_load_valid() {
        let tmp = std::env::temp_dir().join("__kx_config_valid__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join(".kx.toml"),
            r#"
stack = "rust"
system_prompt = "Custom prompt"

[provider]
model = "claude-sonnet"
max_turns = 5
timeout_secs = 120
"#,
        )
        .unwrap();

        let config = ProjectConfig::load(&tmp);
        assert_eq!(config.stack, Some("rust".to_string()));
        assert_eq!(config.system_prompt, Some("Custom prompt".to_string()));
        assert!(config.provider.is_some());
        let provider = config.provider.unwrap();
        assert_eq!(provider.model, Some("claude-sonnet".to_string()));
        assert_eq!(provider.max_turns, Some(5));
        assert_eq!(provider.timeout_secs, Some(120));

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn config_load_with_skills() {
        let tmp = std::env::temp_dir().join("__kx_config_skills__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(
            tmp.join(".kx.toml"),
            r#"
[skills]
default_trust = "standard"
trusted_sources = ["anthropics/", "vercel/"]
blocked = ["bad-skill"]
"#,
        )
        .unwrap();

        let config = ProjectConfig::load(&tmp);
        assert!(config.skills.is_some());
        let skills = config.skills.unwrap();
        assert_eq!(skills.default_trust, Some("standard".to_string()));
        assert_eq!(skills.trusted_sources.len(), 2);
        assert_eq!(skills.blocked, vec!["bad-skill"]);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn config_load_invalid_toml() {
        let tmp = std::env::temp_dir().join("__kx_config_invalid__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        std::fs::write(tmp.join(".kx.toml"), "invalid { toml").unwrap();

        let config = ProjectConfig::load(&tmp);
        // Should return default on parse error
        assert!(config.stack.is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn skills_policy_default() {
        let config = ProjectConfig::default();
        let policy = config.skills_policy();
        assert_eq!(policy.default_trust, TrustLevel::Sandboxed);
        assert!(policy.trusted_sources.is_empty());
        assert!(policy.blocked_skills.is_empty());
    }

    #[test]
    fn skills_policy_from_config() {
        let config = ProjectConfig {
            skills: Some(SkillsConfig {
                default_trust: Some("trusted".to_string()),
                trusted_sources: vec!["acme/".to_string()],
                blocked: vec!["blocked-skill".to_string()],
            }),
            ..Default::default()
        };

        let policy = config.skills_policy();
        assert_eq!(policy.default_trust, TrustLevel::Trusted);
        assert_eq!(policy.trusted_sources, vec!["acme/"]);
        assert_eq!(policy.blocked_skills, vec!["blocked-skill"]);
    }

    #[test]
    fn skills_policy_standard_trust() {
        let config = ProjectConfig {
            skills: Some(SkillsConfig {
                default_trust: Some("standard".to_string()),
                trusted_sources: vec![],
                blocked: vec![],
            }),
            ..Default::default()
        };

        let policy = config.skills_policy();
        assert_eq!(policy.default_trust, TrustLevel::Standard);
    }

    #[test]
    fn skills_policy_invalid_trust_defaults_sandboxed() {
        let config = ProjectConfig {
            skills: Some(SkillsConfig {
                default_trust: Some("invalid".to_string()),
                trusted_sources: vec![],
                blocked: vec![],
            }),
            ..Default::default()
        };

        let policy = config.skills_policy();
        assert_eq!(policy.default_trust, TrustLevel::Sandboxed);
    }

    #[test]
    fn resolve_stack_override_rust() {
        let config = ProjectConfig {
            stack: Some("rust".to_string()),
            ..Default::default()
        };
        assert_eq!(config.resolve_stack(Stack::Unknown), Stack::Rust);
    }

    #[test]
    fn resolve_stack_override_node_aliases() {
        let config_node = ProjectConfig {
            stack: Some("node".to_string()),
            ..Default::default()
        };
        assert_eq!(config_node.resolve_stack(Stack::Unknown), Stack::Node);

        let config_js = ProjectConfig {
            stack: Some("javascript".to_string()),
            ..Default::default()
        };
        assert_eq!(config_js.resolve_stack(Stack::Unknown), Stack::Node);

        let config_ts = ProjectConfig {
            stack: Some("typescript".to_string()),
            ..Default::default()
        };
        assert_eq!(config_ts.resolve_stack(Stack::Unknown), Stack::Node);
    }

    #[test]
    fn resolve_stack_override_flutter_aliases() {
        let config_flutter = ProjectConfig {
            stack: Some("flutter".to_string()),
            ..Default::default()
        };
        assert_eq!(config_flutter.resolve_stack(Stack::Unknown), Stack::Flutter);

        let config_dart = ProjectConfig {
            stack: Some("dart".to_string()),
            ..Default::default()
        };
        assert_eq!(config_dart.resolve_stack(Stack::Unknown), Stack::Flutter);
    }

    #[test]
    fn resolve_stack_override_go_aliases() {
        let config_go = ProjectConfig {
            stack: Some("go".to_string()),
            ..Default::default()
        };
        assert_eq!(config_go.resolve_stack(Stack::Unknown), Stack::Go);

        let config_golang = ProjectConfig {
            stack: Some("golang".to_string()),
            ..Default::default()
        };
        assert_eq!(config_golang.resolve_stack(Stack::Unknown), Stack::Go);
    }

    #[test]
    fn resolve_stack_override_java_aliases() {
        let config_java = ProjectConfig {
            stack: Some("java".to_string()),
            ..Default::default()
        };
        assert_eq!(config_java.resolve_stack(Stack::Unknown), Stack::Java);

        let config_kotlin = ProjectConfig {
            stack: Some("kotlin".to_string()),
            ..Default::default()
        };
        assert_eq!(config_kotlin.resolve_stack(Stack::Unknown), Stack::Java);
    }

    #[test]
    fn resolve_stack_override_swift_aliases() {
        let config_swift = ProjectConfig {
            stack: Some("swift".to_string()),
            ..Default::default()
        };
        assert_eq!(config_swift.resolve_stack(Stack::Unknown), Stack::Swift);

        let config_swiftui = ProjectConfig {
            stack: Some("swiftui".to_string()),
            ..Default::default()
        };
        assert_eq!(config_swiftui.resolve_stack(Stack::Unknown), Stack::Swift);
    }

    #[test]
    fn resolve_stack_uses_detected_when_no_override() {
        let config = ProjectConfig::default();
        assert_eq!(config.resolve_stack(Stack::Python), Stack::Python);
        assert_eq!(config.resolve_stack(Stack::Rust), Stack::Rust);
    }

    #[test]
    fn resolve_stack_invalid_override_uses_detected() {
        let config = ProjectConfig {
            stack: Some("invalid".to_string()),
            ..Default::default()
        };
        assert_eq!(config.resolve_stack(Stack::Python), Stack::Python);
    }
}
