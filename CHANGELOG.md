# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Multi-provider support** — `--provider` flag selects from `claude-code`, `anthropic`, `openai`, `ollama`, `gemini`, `openrouter`; `--model` flag overrides the provider default
- **Provider auto-detection** — resolves provider in order: `--provider` flag → `KERNEX_PROVIDER` env → `ANTHROPIC_API_KEY` present → `OPENAI_API_KEY` present → Ollama reachable at `localhost:11434` → error
- **`kx init`** — installs all 12 builtin agent skills to `~/.kernex/skills/` (7 Tier 1: `frontend-developer`, `backend-architect`, `security-engineer`, `devops-automator`, `reality-checker`, `api-tester`, `performance-benchmarker`; 5 Tier 2: `senior-developer`, `ai-engineer`, `accessibility-auditor`, `agents-orchestrator`, `project-manager`)
- **`kx pipeline run <PATH>`** — executes a `TOPOLOGY.toml` multi-agent pipeline via `kernex-pipelines`
- **`kx audit`** — runs a code quality audit against the current project using the active provider
- **`kx docs`** — runs a documentation coverage audit against the current project using the active provider
- **`kx cron list/add/delete`** — manage scheduled tasks stored in `kernex-memory`
- **KAIROS scheduler** — background loop (`scheduler::spawn()`) that polls due tasks every 60 seconds and runs them through the active provider
- **`SystemPromptLoader`** — auto-detects and loads system prompt from `.kx.toml` or `KERNEX_SYSTEM_PROMPT` env var
- **Project permission rules** — `permission_rules` field in `.kx.toml` wired end-to-end to `RuntimeBuilder`
- **Session header** — shows `[provider/model]` in cyan on `kx dev` startup; prints `kx init` tip when no skills are installed
- **Skills CLI commands** (`kx skills list/add/remove/verify`)
- **Skills permission model** — trust levels: `sandboxed`, `standard`, `trusted`
- **SHA-256 integrity verification** — computed on install; `kx skills verify` detects tampering
- **Audit logging** — all skill install/remove/verify operations logged to `~/.kernex/audit.log`
- CONTRIBUTING.md with development guidelines

### Changed

- Tokio dependency trimmed from `features = ["full"]` to explicit features (`rt-multi-thread`, `macros`, `sync`, `time`, `rt`) — reduces compile surface
- `run_oneshot_command()` shared helper extracted for `kx audit` and `kx docs` to avoid duplication

### Tests

- 191 tests total (up from 19 in v0.1.0)

## [0.1.0] - 2026-03-07

### Added

- Initial MVP — `kx dev` interactive coding assistant
- Conversation lifecycle with inline commands and one-shot mode
- Ctrl+C handler with graceful conversation close
- Multiline input support with `"""` delimiters
- Rustyline for readline support (history, line editing)
- `/facts` command to view and delete stored facts
- `.kx.toml` project config support
- `/search` command for FTS5 memory search
- Spinner indicator during LLM calls
- `/history` command for conversation history
- `/retry` command for failed completions
- `dev` as the default subcommand
- Claude CLI availability validation on startup
- Improved multiline prompt with line numbers
- `/config` command to show active configuration
