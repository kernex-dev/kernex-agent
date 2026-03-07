use crate::stack::Stack;

pub fn dev_system_prompt(stack: Stack, project_name: &str) -> String {
    let stack_rules = match stack {
        Stack::Rust => {
            "
- Use idiomatic Rust. Prefer Result/Option over panics.
- Follow clippy recommendations. Use `cargo fmt` style.
- Prefer owned types in APIs, references internally.
- Handle errors with thiserror or anyhow as appropriate."
        }

        Stack::Node => {
            "
- Use TypeScript strict mode when tsconfig is available.
- Prefer ESM imports. Follow existing lint/format config (eslint/prettier/biome).
- Handle async errors properly. Avoid any where possible.
- Check package.json scripts before suggesting build/test commands."
        }

        Stack::Python => {
            "
- Follow PEP 8. Use type hints.
- Respect the project's tooling (ruff, black, mypy, pytest).
- Prefer pathlib over os.path. Use context managers for resources.
- Check for virtual environment (venv, poetry, pipenv)."
        }

        Stack::Flutter => {
            "
- Follow Dart effective style. Use const constructors where possible.
- Respect the state management pattern in use (Riverpod, Bloc, Provider, etc).
- Keep widgets small and composable.
- Check pubspec.yaml for dependencies before suggesting new ones."
        }

        Stack::Php => {
            "
- Follow PSR-12 coding standards.
- Use type declarations. Respect the framework in use (Laravel, WordPress, etc).
- Use Composer for dependency management.
- Sanitize all user input. Use prepared statements for DB queries."
        }

        Stack::Unknown => {
            "
- Detect and follow the project's existing conventions.
- Check for config files to understand the tooling before suggesting commands."
        }
    };

    format!(
        r#"You are kx, a senior dev assistant working on the project "{project_name}".
Detected stack: {stack}.

## Core behavior
- Be direct and concise. Lead with the answer.
- Read code before suggesting changes. Understand existing patterns.
- Prefer editing existing files over creating new ones.
- Keep changes minimal and focused. No over-engineering.
- Never commit or expose secrets. Warn if you detect hardcoded credentials.

## Stack-specific rules
{stack_rules}

## Memory
You have persistent memory for this project. Use it to remember decisions, patterns, and context across sessions.

## Output
- Reference files as `path:line` when discussing code.
- Use code blocks with language tags.
- If a task is ambiguous, ask for clarification before acting."#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_contains_project_name() {
        let prompt = dev_system_prompt(Stack::Rust, "my-project");
        assert!(prompt.contains("my-project"));
    }

    #[test]
    fn prompt_contains_stack_name() {
        let prompt = dev_system_prompt(Stack::Rust, "test");
        assert!(prompt.contains("Rust"));

        let prompt_node = dev_system_prompt(Stack::Node, "test");
        assert!(prompt_node.contains("JavaScript/TypeScript"));
    }

    #[test]
    fn prompt_rust_has_rust_rules() {
        let prompt = dev_system_prompt(Stack::Rust, "test");
        assert!(prompt.contains("idiomatic Rust"));
        assert!(prompt.contains("clippy"));
        assert!(prompt.contains("cargo fmt"));
        assert!(prompt.contains("thiserror"));
    }

    #[test]
    fn prompt_node_has_node_rules() {
        let prompt = dev_system_prompt(Stack::Node, "test");
        assert!(prompt.contains("TypeScript strict"));
        assert!(prompt.contains("ESM imports"));
        assert!(prompt.contains("package.json"));
    }

    #[test]
    fn prompt_python_has_python_rules() {
        let prompt = dev_system_prompt(Stack::Python, "test");
        assert!(prompt.contains("PEP 8"));
        assert!(prompt.contains("type hints"));
        assert!(prompt.contains("pathlib"));
        assert!(prompt.contains("venv"));
    }

    #[test]
    fn prompt_flutter_has_flutter_rules() {
        let prompt = dev_system_prompt(Stack::Flutter, "test");
        assert!(prompt.contains("Dart"));
        assert!(prompt.contains("const constructors"));
        assert!(prompt.contains("pubspec.yaml"));
    }

    #[test]
    fn prompt_php_has_php_rules() {
        let prompt = dev_system_prompt(Stack::Php, "test");
        assert!(prompt.contains("PSR-12"));
        assert!(prompt.contains("Composer"));
        assert!(prompt.contains("prepared statements"));
    }

    #[test]
    fn prompt_unknown_has_generic_rules() {
        let prompt = dev_system_prompt(Stack::Unknown, "test");
        assert!(prompt.contains("existing conventions"));
        assert!(prompt.contains("config files"));
    }

    #[test]
    fn prompt_has_core_behavior() {
        let prompt = dev_system_prompt(Stack::Rust, "test");
        assert!(prompt.contains("Core behavior"));
        assert!(prompt.contains("direct and concise"));
        assert!(prompt.contains("Never commit or expose secrets"));
    }

    #[test]
    fn prompt_has_memory_section() {
        let prompt = dev_system_prompt(Stack::Rust, "test");
        assert!(prompt.contains("Memory"));
        assert!(prompt.contains("persistent memory"));
    }

    #[test]
    fn prompt_has_output_section() {
        let prompt = dev_system_prompt(Stack::Rust, "test");
        assert!(prompt.contains("Output"));
        assert!(prompt.contains("path:line"));
        assert!(prompt.contains("code blocks"));
    }
}
