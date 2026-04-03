use std::collections::HashSet;
use std::path::Path;

use serde::Deserialize;

/// A single step in a named workflow.
#[derive(Debug, Deserialize)]
pub struct WorkflowStep {
    /// Unique identifier within the workflow. Used for output references.
    pub id: String,
    /// Name of the skill to activate for this step.
    pub skill: String,
    /// Input template. Supports `{input}` (original job message) and
    /// `{step_id.output}` (output from a previous step).
    pub input: String,
    /// Execution mode override: "task" | "evaluate" | "review".
    pub mode: Option<String>,
    /// Step IDs this step depends on. Must all appear earlier in the steps list.
    #[serde(default)]
    pub depends_on: Vec<String>,
}

/// A named workflow loaded from a TOML file.
#[derive(Debug, Deserialize)]
pub struct Workflow {
    pub name: String,
    pub description: String,
    pub steps: Vec<WorkflowStep>,
}

/// Load a workflow by name from `<data_dir>/workflows/<name>.toml`.
pub fn load_workflow(name: &str, data_dir: &Path) -> Result<Workflow, String> {
    let path = data_dir.join("workflows").join(format!("{name}.toml"));
    let content =
        std::fs::read_to_string(&path).map_err(|_| format!("workflow not found: {name}"))?;
    let workflow: Workflow =
        toml::from_str(&content).map_err(|e| format!("invalid workflow {name}: {e}"))?;
    validate_workflow(&workflow)?;
    tracing::debug!(name = %workflow.name, description = %workflow.description, "workflow loaded");
    Ok(workflow)
}

/// Substitute `{input}` and `{step_id.output}` placeholders in a step input template.
pub fn render_input(
    template: &str,
    original_input: &str,
    outputs: &std::collections::HashMap<String, String>,
) -> String {
    let mut result = template.replace("{input}", original_input);
    for (id, output) in outputs {
        result = result.replace(&format!("{{{id}.output}}"), output);
    }
    result
}

/// Validate that `depends_on` references only steps that appear earlier in the list.
fn validate_workflow(w: &Workflow) -> Result<(), String> {
    if w.steps.is_empty() {
        return Err(format!("workflow '{}' has no steps", w.name));
    }
    let mut seen: HashSet<&str> = HashSet::new();
    for step in &w.steps {
        for dep in &step.depends_on {
            if !seen.contains(dep.as_str()) {
                return Err(format!(
                    "workflow '{}': step '{}' depends_on '{}' which has not been defined yet",
                    w.name, step.id, dep
                ));
            }
        }
        seen.insert(&step.id);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_substitutes_input() {
        let mut outputs = std::collections::HashMap::new();
        outputs.insert("step1".to_string(), "result A".to_string());

        let result = render_input("Review: {input} and {step1.output}", "hello", &outputs);
        assert_eq!(result, "Review: hello and result A");
    }

    #[test]
    fn render_unknown_placeholder_left_as_is() {
        let outputs = std::collections::HashMap::new();
        let result = render_input("Value: {unknown.output}", "x", &outputs);
        assert_eq!(result, "Value: {unknown.output}");
    }

    #[test]
    fn validate_rejects_forward_depends_on() {
        let w = Workflow {
            name: "bad".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    id: "a".to_string(),
                    skill: "s".to_string(),
                    input: "{input}".to_string(),
                    mode: None,
                    depends_on: vec!["b".to_string()],
                },
                WorkflowStep {
                    id: "b".to_string(),
                    skill: "s".to_string(),
                    input: "{a.output}".to_string(),
                    mode: None,
                    depends_on: vec![],
                },
            ],
        };
        assert!(validate_workflow(&w).is_err());
    }

    #[test]
    fn validate_accepts_valid_depends_on() {
        let w = Workflow {
            name: "ok".to_string(),
            description: "".to_string(),
            steps: vec![
                WorkflowStep {
                    id: "a".to_string(),
                    skill: "s".to_string(),
                    input: "{input}".to_string(),
                    mode: None,
                    depends_on: vec![],
                },
                WorkflowStep {
                    id: "b".to_string(),
                    skill: "s".to_string(),
                    input: "{a.output}".to_string(),
                    mode: None,
                    depends_on: vec!["a".to_string()],
                },
            ],
        };
        assert!(validate_workflow(&w).is_ok());
    }

    #[test]
    fn validate_rejects_empty_steps() {
        let w = Workflow {
            name: "empty".to_string(),
            description: "".to_string(),
            steps: vec![],
        };
        assert!(validate_workflow(&w).is_err());
    }

    #[test]
    fn load_workflow_missing_file_returns_error() {
        let dir = std::env::temp_dir().join("__kx_wf_test_missing__");
        let result = load_workflow("nonexistent", &dir);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("workflow not found"));
    }

    #[test]
    fn load_workflow_parses_valid_toml() {
        let dir = std::env::temp_dir().join("__kx_wf_test_valid__");
        std::fs::create_dir_all(dir.join("workflows")).unwrap();
        let content = r#"
name = "test-flow"
description = "A test workflow"

[[steps]]
id = "review"
skill = "senior-developer"
input = "Review: {input}"

[[steps]]
id = "gate"
skill = "reality-checker"
input = "Verify: {review.output}"
depends_on = ["review"]
"#;
        std::fs::write(dir.join("workflows").join("test-flow.toml"), content).unwrap();
        let wf = load_workflow("test-flow", &dir).unwrap();
        assert_eq!(wf.name, "test-flow");
        assert_eq!(wf.steps.len(), 2);
        assert_eq!(wf.steps[1].depends_on, vec!["review"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
