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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
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
    #[allow(dead_code)]
    pub trust: TrustLevel,
    pub granted: BTreeSet<Permission>,
    pub denied: BTreeSet<Permission>,
}

impl ResolvedPermissions {
    #[allow(dead_code)]
    pub fn has(&self, perm: Permission) -> bool {
        self.granted.contains(&perm)
    }
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

    ResolvedPermissions {
        trust,
        granted,
        denied,
    }
}
