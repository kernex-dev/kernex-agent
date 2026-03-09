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

        Stack::Go => {
            "
- Follow Go idioms. Use gofmt and golint.
- Handle errors explicitly — never ignore returned errors.
- Prefer composition over inheritance. Keep interfaces small.
- Check go.mod for the Go version and existing dependencies."
        }

        Stack::Java => {
            "
- Follow Java conventions. Use the project's build tool (Maven or Gradle).
- Prefer modern Java features when the version allows.
- Use dependency injection patterns if a framework is in use (Spring, etc).
- Check pom.xml or build.gradle for existing dependencies."
        }

        Stack::Swift => {
            "
- Follow Swift API Design Guidelines. Use Swift Package Manager when possible.
- Prefer value types (structs) over reference types (classes) when appropriate.
- Use SwiftUI idioms if the project uses SwiftUI (declarative, @State, @Binding).
- Check Package.swift or project settings for the Swift version and dependencies."
        }

        Stack::Ruby => {
            "
- Follow Ruby community style (rubocop). Use blocks and iterators idiomatically.
- Respect the framework in use (Rails, Sinatra, etc). Check Gemfile for dependencies.
- Use Bundler for dependency management.
- Prefer symbols over strings for hash keys when appropriate."
        }

        Stack::Cpp => {
            "
- Follow the project's existing style (C or C++, version standard).
- Use CMake or the project's build system. Check CMakeLists.txt for settings.
- Prefer RAII and smart pointers in C++. Avoid raw new/delete.
- Be careful with memory management, buffer overflows, and undefined behavior."
        }

        Stack::DotNet => {
            "
- Follow .NET naming conventions (PascalCase for public members).
- Use the project's build system (dotnet CLI, MSBuild). Check .csproj files.
- Prefer async/await for I/O-bound operations.
- Use dependency injection when the framework supports it (ASP.NET Core, etc)."
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
    fn prompt_go_has_go_rules() {
        let prompt = dev_system_prompt(Stack::Go, "test");
        assert!(prompt.contains("gofmt"));
        assert!(prompt.contains("golint"));
        assert!(prompt.contains("go.mod"));
    }

    #[test]
    fn prompt_java_has_java_rules() {
        let prompt = dev_system_prompt(Stack::Java, "test");
        assert!(prompt.contains("Maven"));
        assert!(prompt.contains("Gradle"));
        assert!(prompt.contains("pom.xml"));
    }

    #[test]
    fn prompt_swift_has_swift_rules() {
        let prompt = dev_system_prompt(Stack::Swift, "test");
        assert!(prompt.contains("Swift"));
        assert!(prompt.contains("SwiftUI"));
        assert!(prompt.contains("Package.swift"));
    }

    #[test]
    fn prompt_ruby_has_ruby_rules() {
        let prompt = dev_system_prompt(Stack::Ruby, "test");
        assert!(prompt.contains("rubocop"));
        assert!(prompt.contains("Gemfile"));
        assert!(prompt.contains("Bundler"));
    }

    #[test]
    fn prompt_cpp_has_cpp_rules() {
        let prompt = dev_system_prompt(Stack::Cpp, "test");
        assert!(prompt.contains("CMake"));
        assert!(prompt.contains("RAII"));
        assert!(prompt.contains("smart pointers"));
    }

    #[test]
    fn prompt_dotnet_has_dotnet_rules() {
        let prompt = dev_system_prompt(Stack::DotNet, "test");
        assert!(prompt.contains("PascalCase"));
        assert!(prompt.contains("async/await"));
        assert!(prompt.contains(".csproj"));
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
