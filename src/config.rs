use std::path::Path;

use serde::Deserialize;

use crate::stack::Stack;

#[derive(Debug, Default, Deserialize)]
pub struct ProjectConfig {
    pub stack: Option<String>,
    pub system_prompt: Option<String>,
    pub provider: Option<ProviderConfig>,
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
