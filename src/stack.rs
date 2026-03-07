use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stack {
    Rust,
    Node,
    Python,
    Flutter,
    Php,
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
            Stack::Unknown => write!(f, "Unknown"),
        }
    }
}

pub fn detect(project_dir: &Path) -> Stack {
    let markers: &[(&str, Stack)] = &[
        ("Cargo.toml", Stack::Rust),
        ("pubspec.yaml", Stack::Flutter),
        ("package.json", Stack::Node),
        ("requirements.txt", Stack::Python),
        ("pyproject.toml", Stack::Python),
        ("Pipfile", Stack::Python),
        ("composer.json", Stack::Php),
    ];

    for (file, stack) in markers {
        if project_dir.join(file).exists() {
            return *stack;
        }
    }

    Stack::Unknown
}

pub fn project_name(project_dir: &Path) -> String {
    project_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}
