# CLAUDE.md — kx

## Project

`kx` is a CLI dev assistant built on [kernex-runtime](https://github.com/kernex-dev/kernex). It provides an interactive REPL for AI-assisted development, with automatic stack detection and project-scoped context.

**Binary name:** `kx`
**Crate name:** `kernex-agent`

## Git Rules

1. **No Co-Author:** Never append `Co-Authored-By` lines to commit messages.
2. **No auto-commit/push:** Always ask before committing or pushing.
3. **Commit style:** Conventional commits — `feat:`, `fix:`, `refactor:`, `docs:`, `chore:`, `test:`.
4. **Atomic commits:** One logical change per commit.
5. **Branch protection:** Never force push to `main`.

## Architecture

Single-binary CLI application:

```
src/
├── main.rs      # Entry point, REPL loop, runtime initialization
├── cli.rs       # Clap-based argument parsing
├── commands.rs  # Slash commands (/help, /quit, /retry, etc.)
├── config.rs    # Project config loading (.kx.toml)
├── prompts.rs   # System prompt generation
└── stack.rs     # Tech stack detection (Rust, Node, Python, etc.)
```

**Dependencies:**
- `kernex-runtime` — Agent lifecycle, message pipeline, memory
- `kernex-core` — Request/response types, context needs
- `kernex-providers` — Claude Code CLI provider

**Data directory:** `~/.kx/projects/<project-name>/`

## Code Standards

- Rust edition 2021
- Strict clippy: `clippy::unwrap_used`, `clippy::expect_used` denied
- `cargo fmt` before every commit
- `anyhow` for error handling (binary boundary)
- Async with tokio runtime
- Tracing for logging (not println)

## Pre-Commit Gate

| Step | Action |
|------|--------|
| 1 | `cargo build` |
| 2 | `cargo clippy -- -D warnings` |
| 3 | `cargo test` |
| 4 | `cargo fmt --check` |
| 5 | Commit only after 1-4 pass |

## Relationship to kernex-dev

This is a **consumer** of the published `kernex-*` crates. It does not share a workspace with kernex-dev.

- Uses `kernex-runtime = "0.3"` from crates.io
- Uses `kernex-core = "0.3"` from crates.io
- Uses `kernex-providers = "0.3"` from crates.io

When updating kernex dependencies, bump versions in `Cargo.toml` and run `cargo update`.

## Usage

```bash
# Interactive REPL
kx dev

# One-shot command
kx "explain this function"

# With subcommand
kx dev "refactor this file"
```
