use std::path::Path;

use crate::skills::manifest::{skill_file_path, SkillsManifest};
use crate::skills::parser::parse_skill_md;

/// Build a serve-mode system prompt from a base instruction block + Level 1 skill metadata.
///
/// Level 1 means only name, description, domain, and trust level are injected (30-100 tokens
/// per skill). The full SKILL.md body never enters context at this level.
///
/// If `skill_names` is empty the base prompt is returned as-is.
/// Skills that are not installed or fail to parse are skipped with a warning log.
pub fn build_serve_system_prompt(
    skill_names: &[String],
    data_dir: &Path,
    mode: Option<&str>,
) -> String {
    let base = match mode {
        Some("evaluate") | Some("review") => {
            "You are an evaluation agent running in headless server mode. \
             Analyze the provided input and return a structured assessment. \
             Be objective, cite specific evidence for every claim, and never \
             assert facts you cannot verify from the provided context."
        }
        _ => {
            "You are a task agent running in headless server mode. \
             Complete the requested task and return your output. \
             Only assert facts you can verify from the provided context."
        }
    };

    if skill_names.is_empty() {
        return base.to_string();
    }

    let manifest = SkillsManifest::load(data_dir);
    let mut skill_lines: Vec<String> = Vec::new();

    for name in skill_names {
        let Some(installed) = manifest.find(name) else {
            tracing::warn!(skill = %name, "requested skill not installed, skipping");
            continue;
        };

        let path = skill_file_path(data_dir, &installed.name);
        let raw = match std::fs::read_to_string(&path) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(skill = %name, error = %e, "failed to read skill file, skipping");
                continue;
            }
        };

        let parsed = match parse_skill_md(&raw) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(skill = %name, error = %e, "failed to parse skill, skipping");
                continue;
            }
        };

        let domain_tag = parsed.domain.map(|d| format!(" [{d}]")).unwrap_or_default();
        let trigger_tag = if parsed.triggers.is_empty() {
            String::new()
        } else {
            format!(" triggers: {}", parsed.triggers.join(", "))
        };

        let tool_tag = if parsed.toolbox.is_empty() {
            String::new()
        } else {
            let names = parsed
                .toolbox
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!(" tools: [{names}]")
        };

        skill_lines.push(format!(
            "- **{}**{} [{}]: {}{}{}",
            parsed.name, domain_tag, installed.trust, parsed.description, trigger_tag, tool_tag
        ));
    }

    if skill_lines.is_empty() {
        return base.to_string();
    }

    let mut prompt = base.to_string();
    prompt.push_str("\n\n## Active Skills\n");
    for line in &skill_lines {
        prompt.push('\n');
        prompt.push_str(line);
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_skills_returns_base_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path();

        let prompt = build_serve_system_prompt(&[], tmp, None);
        assert!(prompt.contains("task agent"));
        assert!(!prompt.contains("Active Skills"));
    }

    #[test]
    fn evaluate_mode_changes_base_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path();

        let prompt = build_serve_system_prompt(&[], tmp, Some("evaluate"));
        assert!(prompt.contains("evaluation agent"));
        assert!(!prompt.contains("task agent"));
    }

    #[test]
    fn review_mode_is_evaluation_variant() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path();

        let prompt = build_serve_system_prompt(&[], tmp, Some("review"));
        assert!(prompt.contains("evaluation agent"));
    }

    #[test]
    fn unknown_skill_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let tmp = dir.path();

        let skill_names = vec!["nonexistent-skill".to_string()];
        let prompt = build_serve_system_prompt(&skill_names, tmp, None);
        // Should return base prompt without Active Skills block (skill was skipped)
        assert!(!prompt.contains("Active Skills"));
        assert!(prompt.contains("task agent"));
    }
}
