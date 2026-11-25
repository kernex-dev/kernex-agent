# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Breaking: kx runs on the kernex 0.12 crates.** The sandbox, the file
  tools, MCP servers and the agent CLIs now share one path policy, and kx
  hands it the real directories: the project as the workspace,
  `~/.kx/projects/<name>` as the data directory, its `memory.db`, and
  `~/.kx/config.toml`. Tool subprocesses write only in the project and the
  temp and toolchain cache directories, and cannot read or write the
  memory database, the skills or the config. The `read`, `write`, `edit`,
  `grep` and `glob` tools are now confined to the project too (reads also
  reach the installed skills). On Linux, tool subprocesses need kernel 6.2
  or later (Landlock ABI 3); older kernels no longer count as enforcing,
  and kx refuses rather than run unsandboxed. Every other project's data
  directory under `~/.kx/projects/` is also closed to the tools, one
  blocked path per project. With every tool allowed (`--cli-tools all`
  or `kx serve --tools full`) the `claude-code` and `opencode` CLIs run
  their tools without asking, inside the sandbox; a tool list never skips
  their permission checks. That sandbox has the network, and for
  claude-code the keychain and the login token, so the commands the CLI
  runs have them too. See the
  [kernex 0.12.0 changelog](https://github.com/kernex-dev/kernex/blob/main/CHANGELOG.md)
  for the full list.
- **Breaking: `web_fetch` is off by default** in `kx dev`, one-shot runs,
  pipelines and `kx serve`, and is not offered to the model. Turn it on
  for a host list with `--web-fetch docs.rs,crates.io` or
  `web_fetch = ["docs.rs"]` in `~/.kx/config.toml`, or for any public host
  with `all`. A project's `.kx.toml` cannot turn it on; it may only set
  `web_fetch = "none"`. The
  `claude-code` and `opencode` providers get their own WebFetch and
  WebSearch only with `all`. Under `--cli-tools all`, or a tool list with
  a `Bash` entry, their shell has the network whatever `web_fetch` says.
  Under `kx serve --tools read-only` it stays off.
- **Breaking: the agent CLIs get a tool list by default.** In `kx dev`,
  one-shot runs and pipelines, claude-code may use `Read`, `Glob`, `Grep`,
  `Edit`, `Write`, `NotebookEdit` and `TodoWrite`, plus `WebFetch` and
  `WebSearch` under `web_fetch = "all"`. Before, it ran every tool with
  its permission checks off. The list has no shell and names no MCP
  server, so unless `web_fetch = "all"` adds the fetch tools, no tool the
  model runs reaches the network. Set `cli_tools` in `~/.kx/config.toml`,
  or `--cli-tools` (env `KX_CLI_TOOLS`) for one run: `"all"` brings back
  every tool with the checks off, and a list such as `["Read", "Edit",
  "Bash(cargo test *)"]` offers and pre-approves those entries. kx refuses
  an empty list, an unknown tool or a malformed rule. While `web_fetch` is
  not `"all"` it leaves `WebFetch` and `WebSearch` out with a warning, and
  refuses the run when the list names nothing else. A command a `Bash`
  entry lets through has the CLI's network and login token. opencode has
  no tool list, so it now needs `--cli-tools all`. A project's `.kx.toml`
  cannot set `cli_tools`, and `kx serve` keeps its `--tools` profile.
- **Breaking: the claude-code provider has its own login.** kx runs the
  Claude CLI with `CLAUDE_CONFIG_DIR` set to `cli/claude` in the project's
  data directory, so the login in `~/.claude` no longer applies. The
  compose files under `deploy/` no longer mount `.credentials.json`; put
  the token in `.env` instead. Export
  `CLAUDE_CODE_OAUTH_TOKEN` (from `claude setup-token`) or
  `ANTHROPIC_API_KEY`, or log in once per project with
  `CLAUDE_CONFIG_DIR=<that directory> claude`. kx prints these options when
  the CLI reports it is not logged in, and `kx doctor` shows which login it
  will use.
- **Breaking: `--no-memory` also stops storing.** A session without memory
  leaves the memory database as it found it, so each message is answered
  without the earlier ones, `/clear` writes nothing, the REPL neither
  loads nor saves its input history, and `kx dev` runs no scheduled tasks
  (their completion is written to that database). `kx serve` jobs
  that do not name a channel no
  longer store their exchange either.
- **Breaking: a skill's MCP server needs its command in
  `permissions.commands`.** The runtime does not start it otherwise, and
  gives it the network only when the skill declares `permissions.network`
  (a server started with `npx -y` needs both). No builtin skill declares
  an MCP server. The startup gate now says whether each server it lists
  gets the network.
- **Breaking: kx refuses to start while a skill's name or description
  holds a control character.** The runtime decodes escapes such as `\n`
  in a TOML frontmatter and lists every skill in the prompt, so such a
  field could print a forged heading or trust line there. The error names
  the skill; fix its frontmatter or remove it.

- **`kx dev` lists installed skills instead of pasting them.** The system
  prompt carried every installed skill's full instructions on every turn:
  about 108 KB (roughly 27,000 tokens) for the 21 builtins. It now carries
  one line per skill with its trust level and denied permissions (about
  1.7 KB), plus the guardrails, and tells the model to read a skill's
  `SKILL.md` with its read tool before acting on it. The runtime's own
  list gives each file's path. A model without a read tool still gets the
  full instructions. The prompt stays the same across turns, so prompt
  caching is unaffected. Because that list comes from the runtime, kx now
  refuses to start while the `SKILL.md` of any skill it installed differs
  from the hash recorded at install (before, it only left the skill out of
  its own list). A skill directory added by hand has no recorded hash.

### Added

- **`--web-fetch` and `--read-path` flags, `web_fetch` and `read_paths`
  config keys.** The keys go in the new `~/.kx/config.toml`, which applies
  to every project. `read_paths` names extra directories the file tools may
  read. In a project's `.kx.toml` both keys are ignored with a warning,
  except `web_fetch = "none"`, which turns the tool off.

- **Eight more builtin skills.** `kx init` now installs 21 skills instead of
  13. The new ones are `test-engineer`, `debugger`, `technical-writer`,
  `database-engineer`, `release-manager`, `data-engineer`,
  `mobile-developer` and `incident-responder`. `debugger`,
  `release-manager` and `incident-responder` declare read-only `git`
  toolbox commands (`git log`, `git status`, `git tag --list`); the other
  five declare none.

### Fixed

- **A provider name in another case no longer skips the tool list.**
  `--provider Claude-Code` (or a `kx serve` job naming it that way) built
  the claude-code CLI through the runtime's factory, which ignores case,
  so it ran with its permission checks off even under `kx serve --tools
  read-only`, and opencode was not refused. kx now folds the name first.

## [0.8.0] - 2026-11-23

### Security

- `Cargo.lock` moves `url` to 2.5.9 (RUSTSEC-2026-0592, a stack overflow in
  `Url::origin()` on nested `blob:` URLs).

### Changed

- **Breaking: `kx serve` jobs run with read-only tools by default.** A job
  could run shell commands, write files and fetch URLs with nobody
  watching, steered by whatever text it read. Jobs can now read and search
  files only. Start the server with `--tools full` (or `KX_SERVE_TOOLS=full`)
  to get the previous behavior; do that only where you control the input of
  every job. Workflows that fetch pages or run commands need `full`. Under
  `read-only` the `claude-code` provider is started with an allowlist
  instead of `--dangerously-skip-permissions`, and the `opencode` provider
  is refused because it has no allowlist.
- **Breaking: `kx serve` jobs no longer share conversation history.** Every
  job on a channel read the prompts and answers of the jobs before it. Each
  job now starts empty. A `/run` request that names a `channel` keeps a
  shared conversation; webhook jobs never do.

- **`deploy/docker-compose.local.yml` publishes the port on 127.0.0.1 only.**
  It published 8080 on every interface over plain http, although the file
  described itself as localhost access. Set `KX_BIND=0.0.0.0` in `.env` to
  serve the LAN again.
- **The compose files run a pinned release instead of `latest`.** They
  tracked `latest` with `pull_policy: always`, so every restart could run a
  new image. Set `KX_VERSION` in `.env` to upgrade. A test keeps the default
  on the crate version.
- The Caddy container receives `DOMAIN` and `ACME_EMAIL` only. It used to
  receive the whole `.env`, including the auth token and provider keys.
- Base images are pinned by digest and the Caddy rate-limit plugin by
  commit. Dependabot now proposes image updates for `deploy/`.

## [0.7.2] - 2026-11-22

### Changed

- **`cargo install kernex-agent` installs the current release again.**
  crates.io had served 0.5.0 since May 2026: 0.6.0 through 0.7.1 were tagged
  and shipped as binaries and a container image, but nothing published the
  crate. The release workflow now publishes it from the tag through
  crates.io trusted publishing, after the same gate as the binaries. No
  source changes since 0.7.1.
- The published crate no longer carries deployment files, design notes, CI
  and editor configuration: 100 files instead of 174. They remain in the
  repository.
- `Cargo.lock` moved off three yanked releases (`chacha20`, `clipboard-win`,
  `spin`), which `cargo install --locked` warned about.

## [0.7.1] - 2026-11-22

### Security

- **A project `.kx.toml` can no longer choose the provider or its endpoint.**
  `provider.name` overrode `--provider`, and `provider.base_url` was honored,
  so running `kx` (or just `kx doctor`) inside a cloned repository could send
  your API key to a host named by that repository, which then answered as the
  model and drove kx's tools. Both fields are now ignored with a warning; use
  `--provider` / `KERNEX_PROVIDER` and `--base-url` / `KERNEX_BASE_URL`.
  `provider.model`, `max_tokens` and `timeout_secs` still work. Upgrade if you
  run kx in repositories you did not write.
- **Skills that start commands need `--trust trusted`.** A skill installed at
  the default `sandboxed` trust could declare an MCP server or a toolbox, and
  the runtime started it without consulting kx's trust level or hash.
  `kx skills add` now refuses such a skill below `trusted` and prints the
  command lines when it accepts one. At startup kx refuses to run while an
  installed skill would launch a command and is untrusted, unlisted in the
  manifest, edited since install, or has an `mcp.json` / `toolbox.json`
  sidecar. If this blocks you, the message names the skill and both ways out.
- **File tools can no longer read credential stores or write shell startup
  files.** `read`, `write` and `edit` run inside the kx process, outside the OS
  sandbox, and were limited only by the runtime's data-directory check. They
  are now denied `~/.ssh`, `~/.aws`, `~/.gnupg`, `~/.config/gh`, `~/.npmrc`,
  `~/.cargo/credentials.toml`, `~/.git-credentials` and similar stores, plus
  `~/.zshrc`, `~/.zshenv`, `~/.bashrc`, `~/.profile`, `~/Library/LaunchAgents`
  and other files a login runs. The same paths are denied to sandboxed
  subprocesses. This is a deny list: the tools are still not confined to the
  project directory.

### Fixed

- `--no-memory` now does what it says for reads. It returned the runtime's
  default settings, which load recall, facts, summaries, pending tasks and
  outcomes; it now loads none of them. `kx serve` jobs use the same switch.
  The exchange is still written to the store afterwards.
- README and SECURITY.md stated that every tool call runs inside the OS
  sandbox. They now say which tools do and which do not.

## [0.7.0] - 2026-11-21

### Changed

- Synced to kernex 0.11.0. Provider default models now come from the runtime
  (`ProviderFactory::default_model`) instead of a table kept in `kx`. With the
  0.10.0 pin the banner could name one model while another was sent; the two
  can no longer differ. The 0.11.0 defaults replace models their providers
  retired: gemini `gemini-3.6-flash`, groq `openai/gpt-oss-120b`, deepseek
  `deepseek-flash`, xai `grok-4.3`, and OpenRouter IDs in the form OpenRouter
  lists (`anthropic/claude-sonnet-4.6`). `--model`, `KERNEX_MODEL` and
  `provider.model` still override them.

### Added

- `--provider opencode`: runs the local OpenCode CLI inside the kernex sandbox,
  so sandboxed sessions work with any model OpenCode is configured for.

### Fixed

- `cargo build --no-default-features` (alone or with `--features serve`)
  compiles again. The REPL's `/search`, `/history`, `/facts` and `/memory`
  commands use the memory module, which was compiled only with the
  `memory-cli` feature; the module is now always built and the feature gates
  just the `kx mem` subcommand.
- `kx mcp` on a build without the `mcp` feature (the default) now exits with
  an error naming the feature. It used to be read as the one-shot prompt
  "mcp" and sent to the provider.

### Security

- Bumped `anyhow` to 1.0.104, closing RUSTSEC-2026-0190 (unsoundness in
  `Error::downcast_mut`). `anyhow` is a direct dependency of `kx`.
- Bumped `quinn-proto` to 0.11.16, closing RUSTSEC-2026-0185 (remote memory
  exhaustion from unbounded out-of-order stream reassembly, 7.5 high). The
  crate is present in `Cargo.lock` but is not in the build graph for any
  supported target, so released binaries were never exposed; the bump keeps
  `cargo audit` and `cargo deny` clean.

- Bumped `rustls` to 0.23.46 (RUSTSEC-2026-0346, -0347, -0348, -0349),
  `rustls-webpki` to 0.103.17 (RUSTSEC-2026-0382) and `webpki-roots` to 1.0.12
  (RUSTSEC-2026-0381). This is the TLS stack `kx` uses for every HTTP provider.
- Bumped `quinn-proto` to 0.11.19 (RUSTSEC-2026-0338 to -0342, -0523, -0524)
  and `quinn-udp` to 0.5.16 (RUSTSEC-2026-0538).
- Bumped `tokio` to 1.53.3, closing RUSTSEC-2026-0386 (`AsyncFd::new` and
  related constructors violate I/O safety).

All of the above are lockfile-only, with no source changes.

Accepted for now, each with a reason and a review date in `deny.toml` (and
`.cargo/audit.toml` for the first): RUSTSEC-2026-0459 in the SQLite bundled by
`libsqlite3-sys`, a local-only issue that needs a crafted database and `-wal`
pair, while `kx` opens only databases it creates; the fix needs sqlx 0.9 in
the kernex crates. Also the unmaintained notices for `basic-toml` and `chrono`,
neither of which has a patched release.

### Changed

- Bumped `kernex-*` dependencies to 0.10.0 (mid-loop token budget and the scheduled-task claim/run-history store surface). See the [kernex-dev CHANGELOG](https://github.com/kernex-dev/kernex/blob/main/CHANGELOG.md) for upstream changes.

## [0.6.1] - 2026-06-12

### Fixed

- **Docker build.** The builder stage now copies `templates/` (compiled in
  via `include_str!` since the Claude adapter landed); the v0.6.0 image
  build failed on the first tag push since that change. Binaries and the
  GitHub Release for v0.6.0 were unaffected.

## [0.6.0] - 2026-06-12

### Security

- **Fail-closed tool execution.** Every provider kx builds now requires
  OS-level sandbox enforcement: hosts that cannot apply Seatbelt (macOS) or
  Landlock (Linux 5.13+) refuse to spawn agent tool subprocesses instead of
  silently running them unsandboxed. With `kernex-* 0.9.0`, tool
  subprocesses run with subprocess environment isolation, a `$HOME` write
  lockdown plus credential read-deny list, and network egress denied by
  default with a per-tool `network = true` opt-in (full coverage on macOS;
  TCP-only on Linux 6.7+, with the older-kernel gap logged).
- **Skill ref pinning.** `kx skills add owner/repo@<sha|tag>` installs that
  exact ref; the manifest records the requested ref and the resolved commit
  SHA alongside the content SHA-256.
- **Symlink-safe configurator.** Installs refuse to write through symlinked
  targets, and backups archive symlinks as links, so a planted link can
  neither redirect writes nor smuggle foreign content into a restore.
- **Signed releases.** Standalone binaries ship with `SHA256SUMS` and
  per-artifact sigstore attestations (SLSA v1 via GitHub OIDC); the release
  workflow gains a non-publishing dry-run entry point.
- **SECURITY.md rewritten** to describe enforced behavior, deployment
  warnings (`kx serve` TLS/rate-limiting posture, plaintext-at-rest
  `jobs.db`), and the updated threat-model caveats.

### Changed

- **Bumped to `kernex-* 0.9.0`.** See the
  [kernex-dev CHANGELOG `[0.9.0]`](https://github.com/kernex-dev/kernex/blob/main/CHANGELOG.md)
  for the upstream security-hardening and provider-correctness notes
  (subprocess env isolation, sandbox scope, egress policy, enforced skill
  permissions, hardened MCP command validation, SSRF resolve-time pinning,
  adaptive thinking, SSE rebuild, pricing and model-default fixes).

### Added

- **Codex CLI adapter (opt-in, behind `--features agent-codex`).** Wires kernex into OpenAI Codex CLI. `kx install --agent codex --preset solo-dev` resolves to three components: `config-toml` (writes `~/.codex/config.toml` with `[mcp_servers.kernex]` upserted via `toml_edit`, preserving non-kernex entries byte-for-byte), `agents-md` (writes `<cwd>/AGENTS.md` with a `<!-- kernex:begin -->` / `<!-- kernex:end -->` marker block, leaving the rest of the file untouched), and `output-style` (writes `~/.codex/output-style.md`). New `merge_codex_config_toml` and `merge_marker_block` helpers cover the merge semantics; the latter lives in `src/adapters/shared.rs` for reuse by the upcoming OpenCode, Cursor, and Cline adapter sprints. Detection uses the new `Detection::with_project_root` constructor (kernex-adapter-core 0.8.3) so the cwd at `kx install` time flows through into the Stage 5 plan as the project-local allowlisted root per ADR-001. New `delta-agent-codex` CI gate enforces a 800 KiB ceiling on macOS aarch64 per F-LOCK-03. Public docs at [docs/adapters/codex.md](docs/adapters/codex.md).
- **`Detection::project_root` consumed in `component_path`.** The configurator's Stage 2 RESOLVE now dispatches paths on `(opts.agent, component)` so each adapter targets its own file layout. Claude paths stay under `<home>/.claude/`; Codex paths come from `detection.config_root` (default `<home>/.codex/`) and `detection.project_root` (default `std::env::current_dir()`). `resolve_preset(name, agent)` is now agent-aware; unknown agents under a known preset surface as `InstallError::UnknownAgent` with exit code 2.
- **`kx mem save` wired.** The placeholder `CliError::NotImplemented` returned by `kx mem save` is replaced with a real handler that calls `MemoryStore::save_observation` and renders the saved row through the same auto-JSON / table path as `kx mem search`. Both inline-flag and `--stdin` JSON input modes are supported; eight CLI-layer validators surface the standard exit-code taxonomy (missing title / type → exit 2; unknown type → exit 2 with a hint listing the seven valid kinds; mixing `--stdin` and inline fields → exit 2; sandbox-blocked data dir → exit 4 per the structured-error spec). New `SaveRecord` projection mirrors `kernex_memory::Observation` with optional fields rendering as JSON `null`.
- **`kx mem get <id>` wired.** The placeholder `CliError::NotImplemented` returned by `kx mem get` is replaced with a real handler that calls `MemoryStore::get_message_by_id` and renders the matched row through the same auto-JSON / table path as `kx mem search`. Missing ids return `CliError::NotFound` (exit code 3) with the same hint shape as the rest of the surface. The `id` argument is now typed as `String` (was `i64`); message ids in `kernex-memory` are UUIDs, not integers, and the old type was a latent bug that would have rejected every real id.

### Changed

- **Bumped to `kernex-* 0.8.0`** to consume the workspace's typed observation table. `MemoryStore::get_memory_stats` returns a 4-tuple `(conversations, messages, observations, facts)` instead of the prior 3-tuple; the `StatsRecord::observations` field now carries a real observation count from the typed table instead of the placeholder that surfaced the message count. Five new trait methods (`save_observation`, `get_observation_by_id`, `search_observations`, `soft_delete_observation`, `list_soft_deleted_observations`) are now reachable through `Arc<dyn MemoryStore>`. See [kernex-dev CHANGELOG `[0.8.0]`](https://github.com/kernex-dev/kernex/blob/main/CHANGELOG.md) for the full upstream notes.
- **Bumped to `kernex-* 0.7.0`** to consume the workspace's typed-row migration. The trait now returns typed `MessageRow` and `HistoryRow` structs instead of `(String, String, String)` / `(String, String)` tuples, and `search_messages` accepts `since: Option<SystemTime>` so the `--since` cutoff filters at the SQL layer instead of in the agent's post-fetch loop. See [kernex-dev CHANGELOG `[0.7.0]`](https://github.com/kernex-dev/kernex/blob/main/CHANGELOG.md) for the full upstream notes.
- **`mem::cli::search` pushes `--since` server-side.** The agent no longer parses each row's SQLite timestamp string after the fetch; it parses `--since` once into a `SystemTime` cutoff and hands it to `store.search_messages(..., Some(cutoff))`. Net effect at the CLI is identical (the search recency-cutoff spec scenario stays green); net effect at the trait boundary is one fewer round of string parsing per row and one fewer source of timestamp-format drift between caller and store.
- **Deleted `parse_sqlite_utc`, `timestamp_after`, `days_from_civil`** (~60 LOC of hand-rolled timestamp math + their tests). The new typed surface returns `SystemTime` directly, so the agent compares `SystemTime` against `SystemTime` and formats once via `chrono::DateTime<Utc>` for display. `chrono` is now a direct dep (was previously transitive through `kernex-memory`).
- **`src/lib.rs` extracted from `main.rs`.** `tests/mem_repl_parity.rs` integration tests reach `mem::cli::*` handlers directly through the library surface. Five parity tests flipped from placeholder `#[ignore]` to live `tokio::test` bodies asserting record-set stability across consecutive idempotent reads. `[profile.release]` adds `lto = "fat"` + `codegen-units = 1` so the cross-crate boundary cost stays absorbed; binary returns to (and improves on) the pre-split baseline.

### Notes on `[Unreleased]` ordering

The entries below this line predate the 0.7.0 paired migration. Read them in their original order; they describe the work that landed against `kernex-* 0.6.2` and earlier.

---

- **`kx mem *` subcommand surface.** Multiple atomic commits between `56bd0b7` and `841db57` add a typed CLI surface backed by `kernex_memory::MemoryStore`:
  - `kx mem search <query>` (commit `632d99b`): FTS5 search with `--limit`, `--since` (e.g. `30d`, `4h`), `--type` filter, and `--json` / `--compact` / `--select` projection flags.
  - `kx mem history` (commit `632d99b`): closed-conversation summaries with `--last`.
  - `kx mem stats` (commit `94bb6a6`): counts plus DB size plus `last_write_at`.
  - `kx mem facts {list, get, add, delete}` (commit `60c60a9`): full CRUD; `add --stdin` for newline-bearing values; `delete` is soft-delete to preserve the soft-delete-invisible-to-default-reads invariant.
  - `kx mem get <id>`: originally stubbed pending the upstream typed-row trait surface; wired in the 0.7.0 paired migration above.
  - `kx mem save`: originally stubbed; wired in the 0.8.0 paired migration above against the typed observation table.
  - CC-1..CC-8 cross-cutting contract: auto-JSON when stdout is not a TTY, `--json` to force, `--compact` projection, `--select <fields>` allowlist, empty result returns `[]` (not `null`), structured stderr on errors, exit-code taxonomy (0/2/3/4/5/7), `--help` text contract on every subcommand (commit `7326d26`).
- **REPL parity (commit `841db57`).** The five memory-related slash commands (`/search`, `/history`, `/memory`, `/facts`, `/facts delete`) now delegate through the same `mem::cli::*` handler functions that the `kx mem *` subcommands dispatch to. Parity is structural: single shared codepath. Side effects: `/facts delete` is now soft-delete (was hard-delete; matches the CLI contract); `/history` reads `CLI_CHANNEL="cli"` instead of `runtime.channel`; `/memory` surfaces `last_write_at` when present. Two pre-existing display-naming bugs in `/search` and `/history` are corrected in the same change.
- **REPL parity harness** at `tests/mem_repl_parity.rs`. Scaffold committed in `56bd0b7`; flipped from placeholder `#[ignore]` to live handler-call assertions once the typed-row migration brought typed records and the `src/lib.rs` extraction made the integration boundary reachable.

### Changed

- **Bumped to `kernex-* 0.6.2`** (commit `8d4185d`) to consume the workspace's migrations fast-path. `Store::run_migrations` saves ~10 ms cold-open by replacing the per-migration round-trip loop with one `SELECT name FROM _migrations` plus an in-memory `HashSet<String>`. The bump leapfrogged `0.6.1 → 0.6.2` because the migrations fast-path's only consumer-visible change is internal to `Store::new` (no API surface change). See [kernex-dev CHANGELOG `[0.6.2]`](https://github.com/kernex-dev/kernex/blob/main/CHANGELOG.md) for full release notes.
- **`CliError` migrated to `thiserror`** (commit `632d99b`). Audit findings produced: UTF-8-safe `parse_since` (was panicking on multi-byte units like `30日`), `is_already_rendered` walks `err.chain()` (was leaf-only), `#[tracing::instrument]` on `mem::dispatch / open_store / search`, `i64::try_from` on `--limit` to guard usize→i64 wrap on 64-bit, `&dyn MemoryStore` instead of `&Arc<dyn ...>` where ownership isn't needed, plus three new regression tests.
- **Previously bumped to `kernex-* 0.6.1`** to consume the workspace's first OIDC trusted-publishing release. Added `kernex-memory` as a direct dep (was previously pulled transitively via `kernex-runtime`). The direct pin is the prerequisite for the `kx mem *` subcommand surface above.
- **Previously bumped to `kernex-* 0.6.0`** to consume the workspace's 0.6.0 release (`MemoryStore` trait + soft-delete on facts in `kernex-memory`, `Runtime::store_handle()` returning `Arc<dyn MemoryStore>`, workspace split into `kernex-adapter-core` / `kernex-presets` / `kernex-brain`, and the new `release` / `release-fast` cargo profiles).

### Added — earlier in this unreleased window

- **Cargo feature graph.** `default = ["agent-claude", "memory-cli", "serve"]` plus five non-default adapter feature flags (`agent-codex`, `agent-opencode`, `agent-cursor`, `agent-cline`, `agent-windsurf`), `tui`, and five preset feature flags (declared, no implementation yet). The default `kx` binary stays under the 15 MB hard ceiling on macOS aarch64 release builds. See `openspec/archive/2026-05-cargo-feature-graph/`.
- **Three-variant size matrix in CI.** `.github/workflows/size-gate.yml` builds the `default`, `--no-default-features --features memory-cli` minimal, and `--features agent-all,tui,serve,preset-all` full variants on every PR. Per-variant ceilings: 8 MB minimal soft-warn, 15 MB default hard-fail, 25 MB full informational. Plus a minimal-variant dep-tree leak audit guarding against `serve`-only or `tui`-only deps regressing into the minimal binary.
- **Sticky `## Binary Sizes` PR comment.** New workflow pair `.github/workflows/binary-size-build.yml` (matrix build on `pull_request`) and `.github/workflows/binary-size-comment.yml` (comment-write on `workflow_run`) post the three-variant size deltas vs `main` directly to the PR thread. One sticky comment per PR; pushes update in place. See `openspec/changes/binary-size-pr-comment/`.

## [0.5.0] - 2026-05-07

### Changed

- **Bumped to `kernex-* 0.5.0`** to consume the new per-crate typed errors landed in the workspace's 0.5.0 release (see [kernex-dev CHANGELOG `[0.5.0]`](https://github.com/kernex-dev/kernex/blob/main/CHANGELOG.md#050---2026-05-07) for the full migration notes and design rationale).
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
