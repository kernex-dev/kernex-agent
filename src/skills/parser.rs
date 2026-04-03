use std::collections::BTreeSet;
use std::fmt;

use super::types::{
    Permission, SkillManifest, SkillSource, SkillTool, MAX_SKILL_NAME_LEN, MAX_SKILL_SIZE,
};

struct ParsedFrontmatter {
    name: String,
    description: String,
    permissions: BTreeSet<Permission>,
    domain: Option<String>,
    triggers: Vec<String>,
    toolbox: Vec<SkillTool>,
}

/// Accumulator for a `[toolbox.NAME]` section being parsed.
struct InProgressTool {
    name: String,
    description: Option<String>,
    command: Option<String>,
    args: Vec<String>,
    parameters_schema: Option<String>,
}

impl InProgressTool {
    fn new(name: String) -> Self {
        Self {
            name,
            description: None,
            command: None,
            args: Vec::new(),
            parameters_schema: None,
        }
    }

    fn finish(self, toolbox: &mut Vec<SkillTool>) {
        if let (Some(description), Some(command)) = (self.description, self.command) {
            toolbox.push(SkillTool {
                name: self.name,
                description,
                command,
                args: self.args,
                parameters_schema: self.parameters_schema,
            });
        }
    }
}

/// Parse a TOML inline array of strings: `["a", "b", "c"]` → `vec!["a", "b", "c"]`.
fn parse_args_array(value: &str) -> Vec<String> {
    let inner = value.trim().trim_start_matches('[').trim_end_matches(']');
    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[derive(Debug, Clone)]
pub enum SkillParseError {
    MissingFrontmatter,
    MissingField(String),
    InvalidName(String),
    InvalidSource(String),
    TooLarge { size: u64, max: u64 },
    InvalidPermission(String),
}

impl fmt::Display for SkillParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFrontmatter => {
                write!(f, "missing YAML frontmatter (expected --- delimiters)")
            }
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

/// Extract a simple `key: value` or `key = "value"` from a line.
/// Returns `None` if the line is not in either format (e.g. list items, blank lines,
/// TOML section headers like `[permissions]`).
fn extract_key_value(line: &str) -> Option<(&str, &str)> {
    // YAML-style: `key: value` (preferred, checked first)
    if let Some(colon_pos) = line.find(':') {
        let key = line[..colon_pos].trim();
        // Reject section headers like `[permissions]` and lines where the key contains spaces
        if !key.is_empty() && !key.contains(' ') && !key.contains('[') {
            let value = line[colon_pos + 1..].trim();
            return Some((key, value));
        }
    }
    // TOML-style: `key = "value"` (legacy builtins)
    if let Some(eq_pos) = line.find('=') {
        let key = line[..eq_pos].trim();
        if !key.is_empty() && !key.contains(' ') && !key.contains('[') {
            let value = line[eq_pos + 1..]
                .trim()
                .trim_matches('"')
                .trim_matches('\'');
            return Some((key, value));
        }
    }
    None
}

/// Parse the YAML/TOML frontmatter block into a `ParsedFrontmatter`.
fn parse_frontmatter(yaml: &str) -> Result<ParsedFrontmatter, SkillParseError> {
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    let mut permissions: BTreeSet<Permission> = BTreeSet::new();
    let mut domain: Option<String> = None;
    let mut triggers: Vec<String> = Vec::new();
    let mut toolbox: Vec<SkillTool> = Vec::new();
    let mut in_permissions_list = false;
    let mut in_metadata_block = false;
    let mut in_triggers_list = false;
    let mut current_tool: Option<InProgressTool> = None;

    for line in yaml.lines() {
        let is_indented = line.starts_with("  ") || line.starts_with('\t');
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        // TOML section header: `[toolbox.NAME]`, `[permissions]`, etc.
        if trimmed.starts_with('[') && trimmed.ends_with(']') && !is_indented {
            let section = &trimmed[1..trimmed.len() - 1];
            // Finish any in-progress toolbox tool before entering a new section
            if let Some(tool) = current_tool.take() {
                tool.finish(&mut toolbox);
            }
            in_permissions_list = false;
            in_metadata_block = false;
            in_triggers_list = false;
            if let Some(tool_name) = section.strip_prefix("toolbox.") {
                current_tool = Some(InProgressTool::new(tool_name.to_string()));
            }
            continue;
        }

        // Exit nested block contexts when we encounter an unindented non-section line
        if !is_indented {
            in_permissions_list = false;
            in_metadata_block = false;
            in_triggers_list = false;
        }

        // List item (YAML `- item` syntax)
        if let Some(item) = trimmed.strip_prefix("- ") {
            if in_permissions_list {
                permissions.insert(parse_permission(item.trim())?);
            } else if in_triggers_list {
                let phrase = item.trim().to_string();
                if !phrase.is_empty() {
                    triggers.push(phrase);
                }
            }
            // List items outside known list contexts are silently skipped
            continue;
        }

        if let Some((key, value)) = extract_key_value(trimmed) {
            // If inside a [toolbox.NAME] section, parse as tool properties
            if let Some(ref mut tool) = current_tool {
                match key {
                    "description" => tool.description = Some(value.to_string()),
                    "command" => tool.command = Some(value.to_string()),
                    "args" => tool.args = parse_args_array(value),
                    "parameters" => tool.parameters_schema = Some(value.to_string()),
                    _ => {}
                }
                continue;
            }

            // Handle indented keys inside the metadata block
            if in_metadata_block {
                if key == "domain" && !value.is_empty() {
                    domain = Some(value.to_string());
                }
                // Other metadata fields (author, version, etc.) are ignored
                continue;
            }

            match key {
                "name" => name = Some(value.to_string()),
                "description" => description = Some(value.to_string()),
                "permissions" => in_permissions_list = true,
                "metadata" => in_metadata_block = true,
                "trigger" | "triggers" => {
                    if value.is_empty() {
                        // No inline value: expect a YAML list to follow
                        in_triggers_list = true;
                    } else {
                        // Inline pipe-delimited: `trigger = "rust|cargo|clippy"`
                        triggers = value
                            .split('|')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                _ => {
                    // Unknown keys are silently ignored for forward-compatibility.
                }
            }
        }
    }

    // Finish the last toolbox tool if any
    if let Some(tool) = current_tool.take() {
        tool.finish(&mut toolbox);
    }

    let name = name
        .filter(|n| !n.is_empty())
        .ok_or_else(|| SkillParseError::MissingField("name".to_string()))?;
    let description = description
        .filter(|d| !d.is_empty())
        .ok_or_else(|| SkillParseError::MissingField("description".to_string()))?;

    Ok(ParsedFrontmatter {
        name,
        description,
        permissions,
        domain,
        triggers,
        toolbox,
    })
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

    let parsed = parse_frontmatter(yaml_block)?;

    validate_skill_name(&parsed.name)?;

    Ok(SkillManifest {
        name: parsed.name,
        description: parsed.description,
        requested_permissions: parsed.permissions,
        content,
        domain: parsed.domain,
        triggers: parsed.triggers,
        toolbox: parsed.toolbox,
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

    #[test]
    fn parse_toml_style_frontmatter() {
        let raw = "\
---
name = \"rust-best-practices\"
description = \"Rust coding guidelines for idiomatic code\"
version = \"0.1.0\"
trigger = \"rust|cargo|clippy\"
---

# Rust Best Practices
";
        let manifest = parse_skill_md(raw).unwrap();
        assert_eq!(manifest.name, "rust-best-practices");
        assert_eq!(
            manifest.description,
            "Rust coding guidelines for idiomatic code"
        );
        assert_eq!(manifest.requested_permissions.len(), 0);
        assert!(manifest.domain.is_none());
        assert_eq!(manifest.triggers, vec!["rust", "cargo", "clippy"]);
    }

    #[test]
    fn parse_trigger_pipe_delimited_yaml_style() {
        let raw = "\
---
name: devops-skill
description: DevOps automation
trigger: docker|kubernetes|ci/cd
---
content
";
        let manifest = parse_skill_md(raw).unwrap();
        assert_eq!(manifest.triggers, vec!["docker", "kubernetes", "ci/cd"]);
    }

    #[test]
    fn parse_triggers_yaml_list() {
        let raw = "\
---
name: my-skill
description: A skill with list triggers
triggers:
  - docker
  - kubernetes
---
content
";
        let manifest = parse_skill_md(raw).unwrap();
        assert_eq!(manifest.triggers, vec!["docker", "kubernetes"]);
    }

    #[test]
    fn parse_no_trigger_returns_empty() {
        let raw = "\
---
name: simple-skill
description: No triggers defined
---
content
";
        let manifest = parse_skill_md(raw).unwrap();
        assert!(manifest.triggers.is_empty());
    }

    #[test]
    fn parse_trigger_trims_whitespace() {
        let raw = "\
---
name: padded-skill
description: Trigger with extra spaces
trigger = \"  rust  |  cargo  |  clippy  \"
---
content
";
        let manifest = parse_skill_md(raw).unwrap();
        assert_eq!(manifest.triggers, vec!["rust", "cargo", "clippy"]);
    }

    #[test]
    fn parse_yaml_metadata_domain() {
        let raw = "\
---
name: skill-factory
description: Create and iterate on SKILL.md files for any domain.
metadata:
  author: jose-hurtado
  version: \"1.0\"
  domain: ops
---

# Skill Factory
";
        let manifest = parse_skill_md(raw).unwrap();
        assert_eq!(manifest.name, "skill-factory");
        assert_eq!(manifest.domain, Some("ops".to_string()));
    }

    #[test]
    fn parse_toml_permissions_block_does_not_error() {
        // TOML-style permission blocks use a completely different format.
        // The parser should not error — it ignores the TOML-specific fields.
        let raw = "\
---
name = \"devops-automator\"
description = \"DevOps and infrastructure automation.\"
version = \"0.1.0\"

[permissions]
files = [\"read:src/**\"]
commands = [\"docker\", \"kubectl\"]
---

# DevOps Automator
";
        let manifest = parse_skill_md(raw).unwrap();
        assert_eq!(manifest.name, "devops-automator");
        assert_eq!(manifest.requested_permissions.len(), 0);
    }

    #[test]
    fn parse_toolbox_single_tool() {
        let raw = "\
---
name = \"api-tester\"
description = \"API endpoint testing and verification.\"
version = \"0.1.0\"

[toolbox.api_request]
description = \"Send an HTTP request and capture the response.\"
command = \"curl\"
args = [\"-s\", \"-w\", \"%{http_code}\"]
parameters = { type = \"object\", properties = { url = { type = \"string\" } }, required = [\"url\"] }
---

# API Tester
";
        let manifest = parse_skill_md(raw).unwrap();
        assert_eq!(manifest.toolbox.len(), 1);
        let tool = &manifest.toolbox[0];
        assert_eq!(tool.name, "api_request");
        assert_eq!(
            tool.description,
            "Send an HTTP request and capture the response."
        );
        assert_eq!(tool.command, "curl");
        assert_eq!(tool.args, vec!["-s", "-w", "%{http_code}"]);
        assert!(tool.parameters_schema.is_some());
    }

    #[test]
    fn parse_toolbox_multiple_tools() {
        let raw = "\
---
name = \"security-engineer\"
description = \"Application security scanning and review.\"
version = \"0.1.0\"

[toolbox.semgrep_scan]
description = \"Run Semgrep static analysis.\"
command = \"semgrep\"
args = [\"scan\", \"--json\"]

[toolbox.gitleaks_detect]
description = \"Detect hardcoded secrets.\"
command = \"gitleaks\"
args = [\"detect\"]
---

# Security Engineer
";
        let manifest = parse_skill_md(raw).unwrap();
        assert_eq!(manifest.toolbox.len(), 2);
        assert_eq!(manifest.toolbox[0].name, "semgrep_scan");
        assert_eq!(manifest.toolbox[0].command, "semgrep");
        assert_eq!(manifest.toolbox[1].name, "gitleaks_detect");
        assert_eq!(manifest.toolbox[1].command, "gitleaks");
    }

    #[test]
    fn parse_toolbox_no_tools() {
        let raw = "\
---
name = \"simple-skill\"
description = \"No toolbox defined.\"
---
content
";
        let manifest = parse_skill_md(raw).unwrap();
        assert!(manifest.toolbox.is_empty());
    }

    #[test]
    fn parse_toolbox_incomplete_tool_is_skipped() {
        // A tool with missing command is silently dropped
        let raw = "\
---
name = \"incomplete-skill\"
description = \"Has an incomplete toolbox entry.\"

[toolbox.no_command]
description = \"This tool has no command field.\"
---
content
";
        let manifest = parse_skill_md(raw).unwrap();
        assert!(manifest.toolbox.is_empty());
    }

    #[test]
    fn parse_toolbox_with_trigger_and_permissions() {
        // Toolbox coexists with other frontmatter fields
        let raw = "\
---
name = \"reality-checker\"
description = \"Verify claims against evidence.\"
version = \"0.1.0\"
trigger = \"reality check|ship it\"

[permissions]
files = [\"read:src/**\"]
commands = [\"npm\"]

[toolbox.run_tests]
description = \"Run the test suite.\"
command = \"npm\"
args = [\"test\", \"--\", \"--reporter=json\"]
---

# Reality Checker
";
        let manifest = parse_skill_md(raw).unwrap();
        assert_eq!(manifest.triggers, vec!["reality check", "ship it"]);
        assert_eq!(manifest.toolbox.len(), 1);
        assert_eq!(manifest.toolbox[0].name, "run_tests");
        assert_eq!(
            manifest.toolbox[0].args,
            vec!["test", "--", "--reporter=json"]
        );
    }
}
