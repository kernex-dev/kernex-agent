use std::collections::BTreeSet;
use std::fmt;

use super::types::{Permission, SkillManifest, SkillSource, MAX_SKILL_NAME_LEN, MAX_SKILL_SIZE};

#[derive(Debug, Clone)]
pub enum SkillParseError {
    MissingFrontmatter,
    #[allow(dead_code)]
    InvalidYaml(String),
    MissingField(String),
    InvalidName(String),
    InvalidSource(String),
    TooLarge {
        size: u64,
        max: u64,
    },
    InvalidPermission(String),
}

impl fmt::Display for SkillParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFrontmatter => {
                write!(f, "missing YAML frontmatter (expected --- delimiters)")
            }
            Self::InvalidYaml(msg) => write!(f, "invalid YAML frontmatter: {msg}"),
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
            Self::InvalidName(msg) => write!(f, "invalid skill name: {msg}"),
            Self::InvalidSource(msg) => write!(f, "invalid skill source: {msg}"),
            Self::TooLarge { size, max } => {
                write!(f, "skill file too large: {size} bytes (max {max} bytes)")
            }
            Self::InvalidPermission(perm) => write!(f, "invalid permission: {perm}"),
        }
    }
}

impl std::error::Error for SkillParseError {}

fn parse_permission(s: &str) -> Result<Permission, SkillParseError> {
    match s {
        "context:files" => Ok(Permission::ContextFiles),
        "context:git" => Ok(Permission::ContextGit),
        "suggest:edits" => Ok(Permission::SuggestEdits),
        "suggest:commands" => Ok(Permission::SuggestCommands),
        "suggest:network" => Ok(Permission::SuggestNetwork),
        other => Err(SkillParseError::InvalidPermission(other.to_string())),
    }
}

/// Extract a simple `key: value` from a line. Returns `None` if the line
/// is not in that format (e.g. list items or blank lines).
fn extract_key_value(line: &str) -> Option<(&str, &str)> {
    let colon_pos = line.find(':')?;
    let key = line[..colon_pos].trim();
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    let value = line[colon_pos + 1..].trim();
    Some((key, value))
}

/// Parse the YAML frontmatter block into (name, description, permissions).
fn parse_frontmatter(
    yaml: &str,
) -> Result<(String, String, BTreeSet<Permission>), SkillParseError> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut permissions: BTreeSet<Permission> = BTreeSet::new();
    let mut in_permissions_list = false;

    for line in yaml.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // Check if this is a list item (belongs to the current list context)
        if let Some(item) = trimmed.strip_prefix("- ") {
            if in_permissions_list {
                let perm_str = item.trim();
                permissions.insert(parse_permission(perm_str)?);
                continue;
            }
            // A list item outside a known list context is unexpected but we skip it
            continue;
        }

        // Not a list item, so we leave any list context
        in_permissions_list = false;

        if let Some((key, value)) = extract_key_value(trimmed) {
            match key {
                "name" => {
                    name = Some(value.to_string());
                }
                "description" => {
                    description = Some(value.to_string());
                }
                "permissions" => {
                    // The value after `permissions:` may be empty (list follows)
                    // or could be an inline value we ignore in favour of list items.
                    in_permissions_list = true;
                }
                _ => {
                    // Unknown keys are silently ignored for forward-compatibility.
                }
            }
        }
    }

    let name = name
        .filter(|n| !n.is_empty())
        .ok_or_else(|| SkillParseError::MissingField("name".to_string()))?;
    let description = description
        .filter(|d| !d.is_empty())
        .ok_or_else(|| SkillParseError::MissingField("description".to_string()))?;

    Ok((name, description, permissions))
}

/// Parse a SKILL.md file (YAML frontmatter + markdown content).
pub fn parse_skill_md(raw: &str) -> Result<SkillManifest, SkillParseError> {
    let trimmed = raw.trim_start();

    // Must start with `---`
    let rest = trimmed
        .strip_prefix("---")
        .ok_or(SkillParseError::MissingFrontmatter)?;

    // Find the closing `---`
    let closing_idx = rest
        .find("\n---")
        .ok_or(SkillParseError::MissingFrontmatter)?;

    let yaml_block = &rest[..closing_idx];
    let after_closing = &rest[closing_idx + 4..]; // skip past "\n---"

    let content = after_closing
        .strip_prefix('\n')
        .unwrap_or(after_closing)
        .trim()
        .to_string();

    let (name, description, requested_permissions) = parse_frontmatter(yaml_block)?;

    validate_skill_name(&name)?;

    Ok(SkillManifest {
        name,
        description,
        requested_permissions,
        content,
    })
}

/// Validate that a skill name conforms to the naming rules.
pub fn validate_skill_name(name: &str) -> Result<(), SkillParseError> {
    if name.is_empty() {
        return Err(SkillParseError::InvalidName(
            "name must not be empty".to_string(),
        ));
    }

    if name.len() > MAX_SKILL_NAME_LEN {
        return Err(SkillParseError::InvalidName(format!(
            "name exceeds maximum length of {MAX_SKILL_NAME_LEN} characters"
        )));
    }

    // Reject path traversal characters
    if name.contains('.') || name.contains('/') || name.contains('\\') {
        return Err(SkillParseError::InvalidName(
            "name must not contain '.', '/' or '\\'".to_string(),
        ));
    }

    // Must start and end with alphanumeric
    let first = name.as_bytes()[0];
    if !first.is_ascii_alphanumeric() {
        return Err(SkillParseError::InvalidName(
            "name must start with an alphanumeric character".to_string(),
        ));
    }
    let last = name.as_bytes()[name.len() - 1];
    if !last.is_ascii_alphanumeric() {
        return Err(SkillParseError::InvalidName(
            "name must end with an alphanumeric character".to_string(),
        ));
    }

    // Only lowercase letters, digits, and hyphens
    for ch in name.chars() {
        if !(ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
            return Err(SkillParseError::InvalidName(format!(
                "name contains invalid character '{ch}': only lowercase letters, digits, and hyphens are allowed"
            )));
        }
    }

    Ok(())
}

/// Validate that a skill file does not exceed the maximum allowed size.
pub fn validate_skill_size(size: u64) -> Result<(), SkillParseError> {
    if size > MAX_SKILL_SIZE {
        return Err(SkillParseError::TooLarge {
            size,
            max: MAX_SKILL_SIZE,
        });
    }
    Ok(())
}

/// Check that a segment (owner or repo) contains only valid characters.
fn is_valid_segment(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parse a skill source string in "owner/repo" or "owner/repo/path/to/skill" format.
pub fn parse_source(input: &str) -> Result<SkillSource, SkillParseError> {
    let input = input.trim();

    if input.is_empty() {
        return Err(SkillParseError::InvalidSource(
            "source must not be empty".to_string(),
        ));
    }

    let parts: Vec<&str> = input.splitn(3, '/').collect();

    if parts.len() < 2 {
        return Err(SkillParseError::InvalidSource(format!(
            "expected 'owner/repo' format, got '{input}'"
        )));
    }

    let owner = parts[0];
    let repo = parts[1];

    if !is_valid_segment(owner) {
        return Err(SkillParseError::InvalidSource(format!(
            "invalid owner '{owner}': only alphanumeric, hyphens, and underscores are allowed"
        )));
    }

    if !is_valid_segment(repo) {
        return Err(SkillParseError::InvalidSource(format!(
            "invalid repo '{repo}': only alphanumeric, hyphens, and underscores are allowed"
        )));
    }

    let path = if parts.len() == 3 && !parts[2].is_empty() {
        let path_str = parts[2];

        // Check for path traversal in every component
        for component in path_str.split('/') {
            if component == "." || component == ".." {
                return Err(SkillParseError::InvalidSource(
                    "path must not contain '.' or '..' components".to_string(),
                ));
            }
        }

        Some(path_str.to_string())
    } else {
        None
    };

    Ok(SkillSource {
        owner: owner.to_string(),
        repo: repo.to_string(),
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_skill_md() {
        let raw = "\
---
name: rust-best-practices
description: Rust coding guidelines
permissions:
  - context:files
  - suggest:edits
---

# Rust Best Practices
Some content here.
";
        let manifest = parse_skill_md(raw).ok();
        assert!(manifest.is_some());
        let manifest = manifest.as_ref();
        assert_eq!(
            manifest.map(|m| m.name.as_str()),
            Some("rust-best-practices")
        );
        assert_eq!(
            manifest.map(|m| m.description.as_str()),
            Some("Rust coding guidelines")
        );
        assert_eq!(manifest.map(|m| m.requested_permissions.len()), Some(2));
        assert!(manifest
            .map(|m| m.requested_permissions.contains(&Permission::ContextFiles))
            .unwrap_or(false));
        assert!(manifest
            .map(|m| m.requested_permissions.contains(&Permission::SuggestEdits))
            .unwrap_or(false));
        assert!(manifest
            .map(|m| m.content.contains("# Rust Best Practices"))
            .unwrap_or(false));
    }

    #[test]
    fn parse_skill_md_no_permissions() {
        let raw = "\
---
name: simple-skill
description: A simple skill
---

Content here.
";
        let manifest = parse_skill_md(raw).ok();
        assert!(manifest.is_some());
        let manifest = manifest.as_ref();
        assert_eq!(manifest.map(|m| m.requested_permissions.len()), Some(0));
    }

    #[test]
    fn parse_skill_md_missing_frontmatter() {
        let raw = "no frontmatter here";
        assert!(parse_skill_md(raw).is_err());
    }

    #[test]
    fn parse_skill_md_missing_name() {
        let raw = "\
---
description: no name
---
content
";
        assert!(parse_skill_md(raw).is_err());
    }

    #[test]
    fn parse_skill_md_missing_description() {
        let raw = "\
---
name: test-skill
---
content
";
        assert!(parse_skill_md(raw).is_err());
    }

    #[test]
    fn parse_skill_md_invalid_permission() {
        let raw = "\
---
name: test-skill
description: test
permissions:
  - context:files
  - unknown:perm
---
content
";
        assert!(parse_skill_md(raw).is_err());
    }

    #[test]
    fn validate_name_valid() {
        assert!(validate_skill_name("rust-best-practices").is_ok());
        assert!(validate_skill_name("a").is_ok());
        assert!(validate_skill_name("skill123").is_ok());
        assert!(validate_skill_name("my-skill-2").is_ok());
    }

    #[test]
    fn validate_name_invalid() {
        assert!(validate_skill_name("").is_err());
        assert!(validate_skill_name("-starts-with-hyphen").is_err());
        assert!(validate_skill_name("ends-with-hyphen-").is_err());
        assert!(validate_skill_name("has.dot").is_err());
        assert!(validate_skill_name("has/slash").is_err());
        assert!(validate_skill_name("has\\backslash").is_err());
        assert!(validate_skill_name("UPPERCASE").is_err());
        assert!(validate_skill_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn validate_size_ok() {
        assert!(validate_skill_size(0).is_ok());
        assert!(validate_skill_size(MAX_SKILL_SIZE).is_ok());
    }

    #[test]
    fn validate_size_too_large() {
        assert!(validate_skill_size(MAX_SKILL_SIZE + 1).is_err());
    }

    #[test]
    fn parse_source_owner_repo() {
        let src = parse_source("acme/my-skill").ok();
        assert!(src.is_some());
        let src = src.as_ref();
        assert_eq!(src.map(|s| s.owner.as_str()), Some("acme"));
        assert_eq!(src.map(|s| s.repo.as_str()), Some("my-skill"));
        assert!(src.map(|s| s.path.is_none()).unwrap_or(false));
    }

    #[test]
    fn parse_source_with_path() {
        let src = parse_source("acme/repo/skills/rust").ok();
        assert!(src.is_some());
        let src = src.as_ref();
        assert_eq!(src.map(|s| s.owner.as_str()), Some("acme"));
        assert_eq!(src.map(|s| s.repo.as_str()), Some("repo"));
        assert_eq!(src.and_then(|s| s.path.as_deref()), Some("skills/rust"));
    }

    #[test]
    fn parse_source_path_traversal() {
        assert!(parse_source("acme/repo/../etc").is_err());
        assert!(parse_source("acme/repo/./here").is_err());
    }

    #[test]
    fn parse_source_invalid_owner() {
        assert!(parse_source("bad owner/repo").is_err());
        assert!(parse_source("/repo").is_err());
    }

    #[test]
    fn parse_source_empty() {
        assert!(parse_source("").is_err());
        assert!(parse_source("onlyone").is_err());
    }
}
