# Roadmap

Technical roadmap for kx development.

## Current State (v0.1)

- Stack detection for 9 languages (Rust, Node, Python, Flutter, PHP, Go, Java, Swift)
- Persistent memory per project via SQLite
- Full-text search with FTS5
- Skills system with SHA-256 integrity verification
- 153 tests

## Short Term (v0.2)

### Stack Enhancements

| Feature | Status | Description |
|---------|--------|-------------|
| Ruby detection | Planned | Gemfile, .ruby-version |
| C/C++ detection | Planned | CMakeLists.txt, Makefile |
| .NET detection | Planned | .csproj, .sln files |
| Monorepo support | Planned | Detect multiple stacks in subdirectories |

### User Experience

| Feature | Status | Description |
|---------|--------|-------------|
| `kx init` | Planned | Interactive project setup with .kx.toml |
| `kx update` | Planned | Self-update command |
| Shell completions | Planned | Bash, Zsh, Fish completions |
| Conversation export | Planned | Export history as markdown |

## Medium Term (v0.3)

### Integration

| Feature | Status | Description |
|---------|--------|-------------|
| VS Code extension | Research | Sidebar integration |
| Neovim plugin | Research | Telescope integration |
| Git hooks | Planned | Pre-commit code review |

### Memory

| Feature | Status | Description |
|---------|--------|-------------|
| Memory stats dashboard | Planned | `/stats` command with visualizations |
| Conversation pruning | Planned | Auto-archive old conversations |

## Long Term (v1.0)

| Feature | Status | Description |
|---------|--------|-------------|
| Multiple providers | Planned | Support for non-Claude backends |
| Offline mode | Research | Local LLM fallback via Ollama |
| Team sync | Research | Share project context across team |

## Relationship to Kernex

kx is a thin CLI wrapper around [kernex-runtime](https://github.com/kernex-dev/kernex). Core runtime features (providers, memory, pipelines) are developed in the main Kernex repository.

For runtime roadmap, see [kernex-dev/ROADMAP.md](https://github.com/kernex-dev/kernex/blob/main/ROADMAP.md).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). All roadmap items marked "Planned" are open for contribution.
