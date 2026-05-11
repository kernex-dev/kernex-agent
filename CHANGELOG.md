# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`kx mem *` subcommand surface (Phase D-agent).** Eight atomic commits between `56bd0b7` and `841db57` add a typed CLI surface backed by `kernex_memory::MemoryStore`:
  - `kx mem search <query>` (Step 2.3, commit `632d99b`): FTS5 search with `--limit`, `--since` (e.g. `30d`, `4h`), `--type` filter, and `--json` / `--compact` / `--select` projection flags.
  - `kx mem history` (Step 2.5, commit `632d99b`): closed-conversation summaries with `--last`.
  - `kx mem stats` (Step 2.6, commit `94bb6a6`): counts plus DB size plus `last_write_at`.
  - `kx mem facts {list, get, add, delete}` (Steps 2.7–2.10, commit `60c60a9`): full CRUD; `add --stdin` for newline-bearing values; `delete` is soft-delete to preserve the CC-9 invariant.
  - `kx mem get <id>` and `kx mem save` (Steps 2.4 and 2.11): stubbed; return `CliError::NotImplemented` (exit code 7) pending the upstream typed-row trait surface and the typed observation table respectively.
  - CC-1..CC-8 cross-cutting contract: auto-JSON when stdout is not a TTY, `--json` to force, `--compact` projection, `--select <fields>` allowlist, empty result returns `[]` (not `null`), structured stderr on errors, exit-code taxonomy (0/2/3/4/5/7), `--help` text contract on every subcommand (Step 2.13, commit `7326d26`).
- **REPL parity (Step 2.14, commit `841db57`).** The five memory-related slash commands (`/search`, `/history`, `/memory`, `/facts`, `/facts delete`) now delegate through the same `mem::cli::*` handler functions that the `kx mem *` subcommands dispatch to. Parity is structural: single shared codepath. Side effects: `/facts delete` is now soft-delete (was hard-delete; matches the CLI contract and CC-9); `/history` reads `CLI_CHANNEL="cli"` instead of `runtime.channel`; `/memory` surfaces `last_write_at` when present. Two pre-existing display-naming bugs in `/search` and `/history` are corrected in the same change.
- **REPL parity harness** at `tests/mem_repl_parity.rs`. Scaffold committed in `56bd0b7`; flips from placeholder `#[ignore]` to handler-call assertions once `memory-typed-row-shape` Slice B brings typed records.

### Changed

- **Bumped to `kernex-* 0.6.2`** (commit `8d4185d`) to consume the workspace's `memory-typed-row-shape` Slice A: `Store::run_migrations` fast-path saves ~10 ms cold-open by replacing the per-migration round-trip loop with one `SELECT name FROM _migrations` plus an in-memory `HashSet<String>`. The bump leapfrogged `0.6.1 → 0.6.2` because Slice A's only consumer-visible change is internal to `Store::new` (no API surface change). See [kernex-dev CHANGELOG `[0.6.2]`](https://github.com/kernex-dev/kernex/blob/main/CHANGELOG.md) for full release notes.
- **`CliError` migrated to `thiserror`** (Step 2.3 audit findings, commit `632d99b`). Five-agent best-practices sweep against the `kernex-dev/.claude/skills/` rubric produced: UTF-8-safe `parse_since` (was panicking on multi-byte units like `30日`), `is_already_rendered` walks `err.chain()` (was leaf-only), `#[tracing::instrument]` on `mem::dispatch / open_store / search`, `i64::try_from` on `--limit` to guard usize→i64 wrap on 64-bit, `&dyn MemoryStore` instead of `&Arc<dyn ...>` where ownership isn't needed, plus three new regression tests.
- **Previously bumped to `kernex-* 0.6.1`** to consume the workspace's first OIDC trusted-publishing release. Added `kernex-memory` as a direct dep (was previously pulled transitively via `kernex-runtime`). The direct pin is the prerequisite for the `kx mem *` subcommand surface above.
- **Previously bumped to `kernex-* 0.6.0`** to consume the workspace's M6 release (MemoryStore trait + soft-delete on facts in `kernex-memory`, `Runtime::store_handle()` returning `Arc<dyn MemoryStore>`, workspace split into `kernex-adapter-core` / `kernex-presets` / `kernex-brain`, and the new `release` / `release-fast` cargo profiles).

### Added — earlier in this unreleased window

- **Cargo feature graph.** `default = ["agent-claude", "memory-cli", "serve"]` plus five non-default adapter feature flags (`agent-codex`, `agent-opencode`, `agent-cursor`, `agent-cline`, `agent-windsurf`), `tui`, and five preset feature flags (declared, no implementation yet). The default `kx` binary stays under the 15 MB hard ceiling on macOS aarch64 release builds. See `openspec/archive/2026-05-cargo-feature-graph/`.
- **Three-variant size matrix in CI.** `.github/workflows/size-gate.yml` builds the `default`, `--no-default-features --features memory-cli` minimal, and `--features agent-all,tui,serve,preset-all` full variants on every PR. Per-variant ceilings: 8 MB minimal soft-warn, 15 MB default hard-fail, 25 MB full informational. Plus a minimal-variant dep-tree leak audit guarding against `serve`-only or `tui`-only deps regressing into the minimal binary.
- **Sticky `## Binary Sizes` PR comment.** New workflow pair `.github/workflows/binary-size-build.yml` (matrix build on `pull_request`) and `.github/workflows/binary-size-comment.yml` (comment-write on `workflow_run`) post the three-variant size deltas vs `main` directly to the PR thread. One sticky comment per PR; pushes update in place. See `openspec/changes/binary-size-pr-comment/`.

## [0.5.0] - 2026-05-07

### Changed

- **Bumped to `kernex-* 0.5.0`** to consume the new per-crate typed errors landed in the workspace's M5 release (see [kernex-dev CHANGELOG `[0.5.0]`](https://github.com/kernex-dev/kernex/blob/main/CHANGELOG.md#050---2026-05-07) for the full migration notes and design rationale).
- No source changes required in `kernex-agent`: the agent uses `anyhow::Result` end-to-end and does not pattern-match on `KernexError` variants, so the breaking change in the workspace's error shape is invisible at this layer.

### Notes

- This release is a coordinated bump with the kernex-dev workspace's 0.5.0 milestone; agent and runtime stay version-locked.
- Downstream code that calls into `kernex-agent` and previously matched on the agent's `anyhow::Error` chain via `downcast_ref::<KernexError>()` can now drill one level further with `.downcast_ref::<kernex_providers::ProviderError>()` etc. on the boxed inner errors. See the workspace CHANGELOG for examples.

## [0.4.4] - 2026-05-07

### Changed

- **README**: refreshed install instructions and feature table to reflect 11-provider count, Bedrock optional feature, and current cargo install command. Switched the embedded logo to the canonical `favicon.svg` shared with the kernex-web marketing site so all three repos render the same mark.

## [0.4.3] - 2026-05-07

This release lands the full audit punch-list (Critical / High / Medium / Low) and adds first-class diagnostics, prompt-cache visibility, and several DX features.

### Security

- **Critical**: `--max-turns` flag was wired to `max_tokens` end-to-end; renamed to `--max-tokens` (`cli.rs`, `main.rs`, `config.rs`, `commands.rs`, `serve/{routes,jobs,mod}.rs`) so the flag's behavior matches its name. Real per-run turn capping requires `Runtime::run()` and is out of scope.
- **Critical**: `@import` arbitrary file read closed in `src/loader.rs` — rejects absolute paths, canonicalizes resolved targets and parents, refuses any target not under `base_dir`, and drops files > 256 KiB. Three new tests cover absolute-path rejection, `..` traversal, and oversized files.
- **High**: `kx serve` now does graceful shutdown via `axum::serve(...).with_graceful_shutdown()` (SIGINT + SIGTERM); worker `JoinHandle` is awaited so in-flight `complete_with_needs` writes settle before exit. Worker semaphore now `acquire_owned().await?` *before* spawn so the bounded mpsc fills under load and clients see real `503 job queue full` back-pressure.
- **High**: tracing subscriber initialized at the head of `main::run()` (env-filter falls back to `warn`, output to stderr), not just inside `cmd_serve`. Scheduler gained a `SchedulerHandle` with a watch-channel shutdown; the loop selects on `shutdown_rx.changed()` and is skipped entirely in one-shot mode.
- **High**: `DefaultBodyLimit::max(1 MiB)` applied at the Axum router layer over `/run`, `/webhook`, `/jobs`, `/health`. Webhook HMAC is now fail-closed: returns 503 when `KERNEX_WEBHOOK_SECRET_<EVENT>` is unset (instead of silently bypassing) and 400 when the event segment fails `^[a-z0-9-]{1,64}$`.
- **High**: builtins no longer auto-fetch from `kernex-dev/main`. Builtin skills now ship via `include_str!` baked into the binary, eliminating the silent supply-chain channel where any push to that branch would auto-install as `TrustLevel::Trusted` on the next `kx init`.
- **High**: skill name validation extended into `skill_file_path` (was previously only on the public CLI surface), preventing path traversal via internally-derived skill names.
- **Medium**: `.kx.toml` parse failures now abort startup with a contextual error instead of silently falling back to defaults; schema gained an optional `version: u32` and `CURRENT_SCHEMA_VERSION` constant; configs declaring a higher version are rejected with an "upgrade kx" message; `api_key` field rejected via `#[serde(deny_unknown_fields)]` so legacy configs surface a hard error.
- **Medium**: bearer token rejected if shorter than 32 bytes at startup. `--workers 0` rejected; values above 256 clamped with a `tracing::warn!`. SQLite `jobs.db` and parent dir now `chmod 0o600 / 0o700` on Unix. Every `uses:` reference in CI workflows SHA-pinned with trailing `# vX.Y.Z` comments.
- **Low**: SHA-256 manifest correctly described in SECURITY.md as TOFU integrity (not authenticity); permission model documented as advisory; `Trusted` auto-stamping of builtins documented; `data_dir.to_str().unwrap_or("~/.kx")` fallback replaced with `to_string_lossy()` for non-UTF-8 paths.

### Added

- **`kx doctor`**: install diagnostics subcommand — checks tool path, data dir, provider env vars, and skill-manifest integrity in one pass.
- **`/cost` slash command**: surfaces per-conversation token usage with prompt-cache hit ratio (uses the `kernex-runtime` 0.4.2 `Store::get_total_usage` breakdown).
- **AGENTS.md interop**: `kx` now treats `AGENTS.md` as a first-class system-prompt source alongside `CLAUDE.md` for cross-tool projects.
- **`--auto-compact` default-on**: `RuntimeBuilder::auto_compact(true)` is the default; `--no-auto-compact` flag opts out. Rolls in the new `kernex-runtime` 0.4.1 capability.

### Changed

- **Tests**: filesystem tests migrated from `temp_dir().join("__kx_…__")` to `tempfile::TempDir` (`b93df86`). Each test now gets a unique random path with RAII cleanup; parallel runs no longer alias on shared paths.
- **Refactor**: `commands.rs` split into a pure `parse(&str) -> SlashCommand<'_>` plus side-effect handler. 11 parse-matrix tests pin previously-implicit boundaries (`/searchfoo` → `Unknown`, `/quit ` trailing space → `Unknown`, slash names case-sensitive).
- **Refactor**: provider list collapsed from three per-provider matches into a single `const PROVIDERS: &[ProviderSpec]` plus `provider_spec`, `default_model`, `api_key_var`, `env_api_key` helpers — adding or removing a provider is now a one-row edit.
- **Errors**: 22 signatures across `main.rs`, `builtins.rs`, `serve/mod.rs` migrated from `Box<dyn Error>` to `anyhow::Result`; `.with_context(...)` added at `RuntimeBuilder::build()` cold-start boundary.
- **Build**: dropped `[patch.crates-io]` block now that the kernex-* sibling crates are published at 0.4.2; Cargo.lock regenerated against the published versions.
- **Deps**: `rustls-webpki` and `rand` bumped to clear `cargo audit` warnings (RUSTSEC-2026-0098 / 0099 / 0104 / 0097).

### CI

- **`cargo deny check`** added: advisories + bans + licenses + sources gate, blocking openssl / native-tls (rustls-only policy) and pinning the license allow-list.
- **clippy** extended to `--all-targets` so test, example, and bench code is also linted with `-D warnings`.

## [0.4.1] - 2026-04-03

### Added

- **Env var support for all flags** — every CLI flag now resolves from a corresponding `KERNEX_*` env var if not provided on the command line (provider, model, api-key, base-url, etc.). Reduces flag noise on every invocation.
- **Compose deploy parity** — `deploy/docker-compose.local.yml` and `deploy/docker-compose.vps.yml` pass full provider env-var sets through to the `kx serve` container so any of the 11 providers can be used in production without container surgery.

### CI

- **`--locked` enforcement** on every cargo invocation in CI to catch `Cargo.lock` drift before Docker build.

### Docs

- **CHANGELOG**: restructured into versioned `[0.4.0]` / `[0.2.0]` sections following the Keep a Changelog format.

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
- **`kx init`** — installs all 12 builtin agent skills to `~/.kx/skills/` (7 core: `frontend-developer`, `backend-architect`, `security-engineer`, `devops-automator`, `reality-checker`, `api-tester`, `performance-benchmarker`; 5 specialist: `senior-developer`, `ai-engineer`, `accessibility-auditor`, `agents-orchestrator`, `project-manager`)
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
