# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0] - 2026-04-03

### Added

- **`kx serve`** — headless HTTP daemon (Axum 0.8) with Bearer auth, async job queue, `/run`, `/jobs`, `/jobs/{id}`, `/webhook/{event}`, and `/health` endpoints
- **5 new providers** — `groq`, `mistral`, `deepseek`, `fireworks`, `xai` (11 total)
- **Docker deployment** — multi-arch image (`linux/amd64`, `linux/arm64`) published to GHCR; `deploy/docker-compose.local.yml` for local/Mac Mini and `deploy/docker-compose.vps.yml` for VPS with auto-TLS via Caddy
- **16 bundled skills** — 12 core + 4 reviewer personas (`enterprise-buyer-reviewer`, `developer-dx-reviewer`, `security-skeptic-reviewer`, `non-technical-stakeholder`) embedded in the Docker image
- **4 bundled workflows** — `pr-review`, `feature-design`, `security-audit`, `geo-audit`
- **Constant-time auth** — bearer token comparison uses `subtle::ConstantTimeEq` to prevent timing attacks
- **Job store eviction** — in-memory store capped at 1,000 entries; oldest finished jobs evicted automatically
- **Webhook HMAC** — optional per-event `X-Hub-Signature-256` verification (same format as GitHub webhooks); bearer auth required in all cases
- **SLSA provenance + SBOM** — release pipeline generates and attests build provenance and software bill of materials via `actions/attest-build-provenance`
- **`--locked` CI enforcement** — all `cargo clippy`, `cargo test`, and `cargo build` calls in CI use `--locked` to catch Cargo.lock drift before Docker build

### Tests

- 260 tests total (up from 191 in v0.2.0)

## [0.2.0] - 2026-04-02

### Added

- **Multi-provider support** — `--provider` flag selects from `claude-code`, `anthropic`, `openai`, `ollama`, `gemini`, `openrouter`; `--model` flag overrides the provider default
- **Provider auto-detection** — resolves provider in order: `--provider` flag → `KERNEX_PROVIDER` env → `ANTHROPIC_API_KEY` present → `OPENAI_API_KEY` present → Ollama reachable at `localhost:11434` → error
- **`kx init`** — installs all 12 builtin agent skills to `~/.kx/skills/` (7 Tier 1: `frontend-developer`, `backend-architect`, `security-engineer`, `devops-automator`, `reality-checker`, `api-tester`, `performance-benchmarker`; 5 Tier 2: `senior-developer`, `ai-engineer`, `accessibility-auditor`, `agents-orchestrator`, `project-manager`)
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
- **Audit logging** — all skill install/remove/verify operations logged to `~/.kx/audit.log`
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
