use std::collections::{BTreeSet, HashMap};

use serde::Deserialize;

use super::types::{Permission, TrustLevel};

#[derive(Debug, Clone, Deserialize)]
pub struct SkillOverride {
    pub trust: Option<TrustLevel>,
    #[serde(default)]
    pub deny: Vec<Permission>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PermissionPolicy {
    #[serde(default)]
    pub default_trust: TrustLevel,
    #[serde(default)]
    pub trusted_sources: Vec<String>,
    #[serde(default)]
    pub blocked_skills: Vec<String>,
    #[serde(default)]
    pub overrides: HashMap<String, SkillOverride>,
}

impl Default for PermissionPolicy {
    fn default() -> Self {
        Self {
            default_trust: TrustLevel::Sandboxed,
            trusted_sources: Vec::new(),
            blocked_skills: Vec::new(),
            overrides: HashMap::new(),
        }
    }
}

impl PermissionPolicy {
    pub fn is_blocked(&self, skill_name: &str) -> bool {
        self.blocked_skills.iter().any(|b| b == skill_name)
    }

    pub fn is_trusted_source(&self, source: &str) -> bool {
        self.trusted_sources
            .iter()
            .any(|prefix| source.starts_with(prefix.as_str()))
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedPermissions {
    pub granted: BTreeSet<Permission>,
    pub denied: BTreeSet<Permission>,
}

pub fn resolve_permissions(
    requested: &BTreeSet<Permission>,
    source: &str,
    policy: &PermissionPolicy,
    skill_name: &str,
) -> ResolvedPermissions {
    let override_entry = policy.overrides.get(skill_name);

    let trust = if let Some(ov) = override_entry {
        if let Some(t) = ov.trust {
            t
        } else if policy.is_trusted_source(source) {
            TrustLevel::Trusted
        } else {
            policy.default_trust
        }
    } else if policy.is_trusted_source(source) {
        TrustLevel::Trusted
    } else {
        policy.default_trust
    };

    let allowed = Permission::for_trust_level(trust);

    let deny_set: BTreeSet<Permission> = override_entry
        .map(|ov| ov.deny.iter().copied().collect())
        .unwrap_or_default();

    let granted: BTreeSet<Permission> = requested
        .intersection(&allowed)
        .copied()
        .filter(|p| !deny_set.contains(p))
        .collect();

    let denied: BTreeSet<Permission> = requested
        .iter()
        .copied()
        .filter(|p| !granted.contains(p))
        .collect();

    ResolvedPermissions { granted, denied }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_default() {
        let policy = PermissionPolicy::default();
        assert_eq!(policy.default_trust, TrustLevel::Sandboxed);
        assert!(policy.trusted_sources.is_empty());
        assert!(policy.blocked_skills.is_empty());
    }

    #[test]
    fn policy_is_blocked() {
        let policy = PermissionPolicy {
            blocked_skills: vec!["bad-skill".to_string()],
            ..Default::default()
        };
        assert!(policy.is_blocked("bad-skill"));
        assert!(!policy.is_blocked("good-skill"));
    }

    #[test]
    fn policy_is_trusted_source() {
        let policy = PermissionPolicy {
            trusted_sources: vec!["anthropics/".to_string(), "vercel-labs/".to_string()],
            ..Default::default()
        };
        assert!(policy.is_trusted_source("anthropics/skills"));
        assert!(policy.is_trusted_source("vercel-labs/agent-skills"));
        assert!(!policy.is_trusted_source("random/repo"));
    }

    #[test]
    fn resolve_sandboxed_grants_minimal() {
        let policy = PermissionPolicy::default();
        let mut requested = BTreeSet::new();
        requested.insert(Permission::ContextFiles);
        requested.insert(Permission::SuggestEdits);
        requested.insert(Permission::SuggestCommands);

        let resolved = resolve_permissions(&requested, "random/skill", &policy, "test-skill");

        assert!(resolved.granted.contains(&Permission::ContextFiles));
        assert!(!resolved.granted.contains(&Permission::SuggestEdits));
        assert!(resolved.denied.contains(&Permission::SuggestEdits));
        assert!(resolved.denied.contains(&Permission::SuggestCommands));
    }

    #[test]
    fn resolve_trusted_source_grants_all() {
        let policy = PermissionPolicy {
            trusted_sources: vec!["anthropics/".to_string()],
            ..Default::default()
        };
        let mut requested = BTreeSet::new();
        requested.insert(Permission::ContextFiles);
        requested.insert(Permission::SuggestCommands);

        let resolved = resolve_permissions(&requested, "anthropics/skills", &policy, "test-skill");

        assert!(resolved.granted.contains(&Permission::ContextFiles));
        assert!(resolved.granted.contains(&Permission::SuggestCommands));
        assert!(resolved.denied.is_empty());
    }

    #[test]
    fn resolve_override_trust() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "special-skill".to_string(),
            SkillOverride {
                trust: Some(TrustLevel::Trusted),
                deny: vec![],
            },
        );
        let policy = PermissionPolicy {
            overrides,
            ..Default::default()
        };
        let mut requested = BTreeSet::new();
        requested.insert(Permission::SuggestCommands);

        let resolved = resolve_permissions(&requested, "random/repo", &policy, "special-skill");

        assert!(resolved.granted.contains(&Permission::SuggestCommands));
    }

    #[test]
    fn resolve_override_deny_specific() {
        let mut overrides = HashMap::new();
        overrides.insert(
            "restricted-skill".to_string(),
            SkillOverride {
                trust: Some(TrustLevel::Trusted),
                deny: vec![Permission::SuggestCommands],
            },
        );
        let policy = PermissionPolicy {
            overrides,
            ..Default::default()
        };
        let mut requested = BTreeSet::new();
        requested.insert(Permission::SuggestEdits);
        requested.insert(Permission::SuggestCommands);

        let resolved = resolve_permissions(&requested, "random/repo", &policy, "restricted-skill");

        assert!(resolved.granted.contains(&Permission::SuggestEdits));
        assert!(!resolved.granted.contains(&Permission::SuggestCommands));
        assert!(resolved.denied.contains(&Permission::SuggestCommands));
    }

    #[test]
    fn resolved_contains_permission() {
        let mut granted = BTreeSet::new();
        granted.insert(Permission::ContextFiles);
        let resolved = ResolvedPermissions {
            granted,
            denied: BTreeSet::new(),
        };
        assert!(resolved.granted.contains(&Permission::ContextFiles));
        assert!(!resolved.granted.contains(&Permission::SuggestEdits));
    }
}
