use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    Rust,
    Node,
    Python,
    Flutter,
    Php,
    Go,
    Java,
    Swift,
    Ruby,
    Cpp,
    DotNet,
    Unknown,
}

impl std::fmt::Display for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Stack::Rust => write!(f, "Rust"),
            Stack::Node => write!(f, "JavaScript/TypeScript (Node)"),
            Stack::Python => write!(f, "Python"),
            Stack::Flutter => write!(f, "Flutter/Dart"),
            Stack::Php => write!(f, "PHP"),
            Stack::Go => write!(f, "Go"),
            Stack::Java => write!(f, "Java"),
            Stack::Swift => write!(f, "Swift/SwiftUI"),
            Stack::Ruby => write!(f, "Ruby"),
            Stack::Cpp => write!(f, "C/C++"),
            Stack::DotNet => write!(f, ".NET/C#"),
            Stack::Unknown => write!(f, "Unknown"),
        }
    }
}

pub fn detect(project_dir: &Path) -> Stack {
    let markers: &[(&str, Stack)] = &[
        ("Cargo.toml", Stack::Rust),
        ("go.mod", Stack::Go),
        ("Package.swift", Stack::Swift),
        ("pubspec.yaml", Stack::Flutter),
        ("pom.xml", Stack::Java),
        ("build.gradle", Stack::Java),
        ("build.gradle.kts", Stack::Java),
        ("package.json", Stack::Node),
        ("requirements.txt", Stack::Python),
        ("pyproject.toml", Stack::Python),
        ("Pipfile", Stack::Python),
        ("composer.json", Stack::Php),
        ("Gemfile", Stack::Ruby),
        ("CMakeLists.txt", Stack::Cpp),
        ("Directory.Build.props", Stack::DotNet),
    ];

    for (file, stack) in markers {
        if project_dir.join(file).exists() {
            return *stack;
        }
    }

    if has_sln_file(project_dir) {
        return Stack::DotNet;
    }

    Stack::Unknown
}

fn has_sln_file(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.path()
            .extension()
            .map(|ext| ext == "sln")
            .unwrap_or(false)
    })
}

pub fn project_name(project_dir: &Path) -> String {
    project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn stack_display() {
        assert_eq!(Stack::Rust.to_string(), "Rust");
        assert_eq!(Stack::Node.to_string(), "JavaScript/TypeScript (Node)");
        assert_eq!(Stack::Python.to_string(), "Python");
        assert_eq!(Stack::Flutter.to_string(), "Flutter/Dart");
        assert_eq!(Stack::Php.to_string(), "PHP");
        assert_eq!(Stack::Go.to_string(), "Go");
        assert_eq!(Stack::Java.to_string(), "Java");
        assert_eq!(Stack::Swift.to_string(), "Swift/SwiftUI");
        assert_eq!(Stack::Ruby.to_string(), "Ruby");
        assert_eq!(Stack::Cpp.to_string(), "C/C++");
        assert_eq!(Stack::DotNet.to_string(), ".NET/C#");
        assert_eq!(Stack::Unknown.to_string(), "Unknown");
    }

    #[test]
    fn detect_go() {
        let tmp = std::env::temp_dir().join("__kx_stack_go__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("go.mod"), "module example.com/app").unwrap();

        assert_eq!(detect(&tmp), Stack::Go);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_java_maven() {
        let tmp = std::env::temp_dir().join("__kx_stack_java_maven__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("pom.xml"), "<project></project>").unwrap();

        assert_eq!(detect(&tmp), Stack::Java);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_java_gradle() {
        let tmp = std::env::temp_dir().join("__kx_stack_java_gradle__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("build.gradle"), "plugins {}").unwrap();

        assert_eq!(detect(&tmp), Stack::Java);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_java_gradle_kts() {
        let tmp = std::env::temp_dir().join("__kx_stack_java_gradle_kts__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("build.gradle.kts"), "plugins {}").unwrap();

        assert_eq!(detect(&tmp), Stack::Java);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_swift() {
        let tmp = std::env::temp_dir().join("__kx_stack_swift__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("Package.swift"), "// swift-tools-version:5.5").unwrap();

        assert_eq!(detect(&tmp), Stack::Swift);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_rust() {
        let tmp = std::env::temp_dir().join("__kx_stack_rust__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("Cargo.toml"), "[package]").unwrap();

        assert_eq!(detect(&tmp), Stack::Rust);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_node() {
        let tmp = std::env::temp_dir().join("__kx_stack_node__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("package.json"), "{}").unwrap();

        assert_eq!(detect(&tmp), Stack::Node);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_python_requirements() {
        let tmp = std::env::temp_dir().join("__kx_stack_py_req__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("requirements.txt"), "flask").unwrap();

        assert_eq!(detect(&tmp), Stack::Python);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_python_pyproject() {
        let tmp = std::env::temp_dir().join("__kx_stack_py_proj__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("pyproject.toml"), "[project]").unwrap();

        assert_eq!(detect(&tmp), Stack::Python);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_flutter() {
        let tmp = std::env::temp_dir().join("__kx_stack_flutter__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("pubspec.yaml"), "name: app").unwrap();

        assert_eq!(detect(&tmp), Stack::Flutter);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_php() {
        let tmp = std::env::temp_dir().join("__kx_stack_php__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("composer.json"), "{}").unwrap();

        assert_eq!(detect(&tmp), Stack::Php);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_ruby() {
        let tmp = std::env::temp_dir().join("__kx_stack_ruby__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("Gemfile"), "source 'https://rubygems.org'").unwrap();

        assert_eq!(detect(&tmp), Stack::Ruby);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_cpp() {
        let tmp = std::env::temp_dir().join("__kx_stack_cpp__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("CMakeLists.txt"),
            "cmake_minimum_required(VERSION 3.10)",
        )
        .unwrap();

        assert_eq!(detect(&tmp), Stack::Cpp);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_dotnet_sln() {
        let tmp = std::env::temp_dir().join("__kx_stack_dotnet_sln__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("MyApp.sln"), "").unwrap();

        assert_eq!(detect(&tmp), Stack::DotNet);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_dotnet_props() {
        let tmp = std::env::temp_dir().join("__kx_stack_dotnet_props__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("Directory.Build.props"), "<Project>").unwrap();

        assert_eq!(detect(&tmp), Stack::DotNet);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_unknown() {
        let tmp = std::env::temp_dir().join("__kx_stack_unknown__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        assert_eq!(detect(&tmp), Stack::Unknown);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn detect_priority_rust_over_node() {
        // Cargo.toml appears first in the markers list
        let tmp = std::env::temp_dir().join("__kx_stack_priority__");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(tmp.join("Cargo.toml"), "[package]").unwrap();
        std::fs::write(tmp.join("package.json"), "{}").unwrap();

        assert_eq!(detect(&tmp), Stack::Rust);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn project_name_normal() {
        let path = PathBuf::from("/home/user/projects/my-app");
        assert_eq!(project_name(&path), "my-app");
    }

    #[test]
    fn project_name_root() {
        let path = PathBuf::from("/");
        // Root has no file_name, should return "unknown"
        assert_eq!(project_name(&path), "unknown");
    }

    #[test]
    fn project_name_with_spaces() {
        let path = PathBuf::from("/home/user/My Project");
        assert_eq!(project_name(&path), "My Project");
    }
}
