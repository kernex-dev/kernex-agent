# kx

CLI dev assistant powered by [Kernex](https://github.com/kernex-dev/kernex-dev).

## Features

- **Stack detection** - Automatically detects Rust, Node/TypeScript, Python, Flutter/Dart, and PHP projects
- **Persistent memory** - Remembers decisions, patterns, and context across sessions per project
- **One-shot mode** - Quick answers without entering interactive mode
- **Multiline input** - Paste code blocks with `"""` delimiters
- **Project configuration** - Per-project settings via `.kx.toml`
- **Full-text search** - Search past conversations with FTS5

## What Can kx Do?

kx is your AI coding assistant. It can:

- **Answer questions** about your code, errors, and architecture
- **Suggest refactoring** patterns and improvements
- **Hunt for bugs** and explain potential issues
- **Explain errors** with context from your codebase
- **Remember context** across sessions (facts, decisions, patterns)
- **Search conversations** with full-text search

**Limitations:**
- Cannot modify files directly (suggests changes for you to apply)
- Cannot run shell commands (use your terminal for that)
- Requires Claude CLI for AI capabilities

## Requirements

**Claude CLI must be installed.** kx uses the Claude Code CLI as its AI backend.

Claude Code is Anthropic's official AI coding assistant that runs locally. To install:

1. Visit [claude.ai/download](https://claude.ai/download)
2. Download and install for your platform (macOS, Linux, Windows)
3. Run `claude --version` to verify installation

For documentation: [docs.anthropic.com/en/docs/claude-code](https://docs.anthropic.com/en/docs/claude-code)

## Installation

### Quick Install (requires Rust)

```bash
cargo install kernex-agent
```

Verify installation:
```bash
kx --version
```

### New to Rust?

Install Rust first from [rustup.rs](https://rustup.rs):

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env
cargo install kernex-agent
```

### From Source

```bash
git clone https://github.com/kernex-dev/kernex-agent.git
cd kernex-agent
cargo install --path .
```

## First Run

```bash
cd /path/to/your/project
kx
```

kx automatically detects your project's stack (Rust, Node, Python, etc.) and starts an interactive session:

```
kx dev my-project (Rust)
Type /help for commands, /quit to exit.

> explain the error in src/main.rs
```

For one-shot questions:
```bash
kx "what does this function do?"
```

## Quick Start

### One-shot mode

```bash
# Ask a quick question
kx "explain this error: cannot borrow as mutable"

# With dev subcommand
kx dev "add error handling to src/lib.rs"
```

### Interactive mode

```bash
# Start interactive session in current project
kx

# Or explicitly
kx dev
```

In interactive mode, type your questions and get responses. Use `/help` for available commands.

### Multiline input

For pasting code blocks or multi-line content:

```
> """
  1 | fn main() {
  2 |     println!("Hello");
  3 | }
  4 | """
  (3 lines captured)
```

## Commands

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/search <query>` | Search past conversations (FTS5) |
| `/history` | Show recent conversation history (last 20 messages) |
| `/stack` | Show detected stack and project info |
| `/memory` | Show memory stats and database size |
| `/facts` | List stored facts |
| `/facts delete <key>` | Delete a specific fact |
| `/config` | Show active configuration |
| `/retry` | Retry last failed message |
| `/clear` | Close current conversation |
| `/quit` or `/exit` | Exit kx |

## Configuration

Create a `.kx.toml` file in your project root to customize behavior:

```toml
# Override auto-detected stack
stack = "rust"

# Add project-specific instructions to the system prompt
system_prompt = """
This project uses a custom error type in src/error.rs.
Always use MyError instead of anyhow.
"""

# Provider settings
[provider]
model = "claude-sonnet-4-20250514"  # Model to use
max_turns = 10                       # Max agentic turns per request
timeout_secs = 300                   # Request timeout in seconds
```

### Stack options

Valid values for `stack`:

- `rust`
- `node`, `javascript`, `typescript`
- `python`
- `flutter`, `dart`
- `php`

## Stack Detection

kx automatically detects your project's stack by looking for these files (in order):

| File | Detected Stack |
|------|----------------|
| `Cargo.toml` | Rust |
| `pubspec.yaml` | Flutter/Dart |
| `package.json` | JavaScript/TypeScript (Node) |
| `requirements.txt` | Python |
| `pyproject.toml` | Python |
| `Pipfile` | Python |
| `composer.json` | PHP |

The first match wins. Override with `stack` in `.kx.toml` if needed.

## Data Storage

Project data is stored in:

```
~/.kx/projects/{project-name}/
```

Where `{project-name}` is derived from the directory name. Each project maintains its own:

- Conversation history
- Stored facts
- Input history (readline)

## Architecture

kx is a thin CLI wrapper around the Kernex runtime:

- **kernex-runtime** - Core engine for agent lifecycle and message handling
- **kernex-providers** - Claude Code CLI integration
- **kernex-core** - Shared types and context management

For details on the underlying runtime, see [kernex-dev](https://github.com/kernex-dev/kernex-dev).

## Extending with Skills

kx can be extended with MCP-based skills from [kernex-dev](https://github.com/kernex-dev/kernex).

Available skills: filesystem, git, playwright, github, postgres, sqlite, brave-search.

See [kernex-dev/examples/skills](https://github.com/kernex-dev/kernex/tree/main/examples/skills) for setup.

## Troubleshooting

### "Claude CLI not found"

Ensure Claude Code is installed and in your PATH:

```bash
claude --version
```

If not found, install from [claude.ai/download](https://claude.ai/download).

### "Permission denied: ~/.kx"

Create the directory manually:

```bash
mkdir -p ~/.kx
```

### Database locked

Only one kx session per project can run at a time. Close other sessions or wait for them to complete.

## License

MIT
