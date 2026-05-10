# Proposal: Cargo feature graph for kernex-agent

> **Change ID:** `cargo-feature-graph`
> **Author:** Jose Hurtado
> **Status:** Draft v0.1
> **Estimated effort:** ~3 working days
> **Repo:** `kernex-dev/kernex-agent` (this repo)

## Operator friction

`kernex.dev` advertises **Single Binary, No runtime dependencies, Under 15 MB**. The runtime workspace at `kernex-dev/kernex` already enforces that ceiling at the library level via the `workspace-profile-baseline` change (`[profile.release]` with `lto = "fat"`, `panic = "abort"`, `opt-level = "z"`, plus a `size-gate.yml` workflow). The runtime side has its discipline. The binary side does not.

Today `kernex-agent/Cargo.toml` ships with **zero feature flags**. All 25 production deps are unconditional: `kernex-runtime`, `kernex-core`, `kernex-providers`, `kernex-pipelines`, `kernex-sandbox`, `tokio`, `clap`, `colored`, `dirs`, `rustyline`, `indicatif`, `serde`, `toml`, `sha2`, `ureq`, `tracing`, `async-trait`, `serde_json`, `axum`, `uuid`, `tracing-subscriber`, `hmac`, `subtle`, `rusqlite`, `anyhow`. Concrete consequences:

1. **No way to ship a minimal `kx`.** A user who wants only `kx mem search "auth"` for CI still pays for `axum`, `hmac`, `subtle`, `uuid`, `rusqlite`, the full daemon stack behind `kx serve`.
2. **No way to opt into a single agent surface.** Configurator adapters land unconditionally into every variant unless a gate already exists.
3. **No place to land a TUI without breaking the size budget.** `ratatui` plus `crossterm` are roughly 2 MB combined; without a `tui` feature to gate against, that weight ships default-on.
4. **No CI signal for variant ceilings.** The `size-gate.yml` workflow templated in `workspace-profile-baseline` ships a `feature-matrix` job that is currently a no-op because no feature graph exists in this crate to exercise.
5. **No size-budget visibility per variant.** Today there is one variant. Any future "minimal" or "full" claim is aspirational until the graph is in place.

This change installs the missing levers. It is the binary-side companion to `workspace-profile-baseline`, not a feature delivery.

## Solution overview

Introduce a Cargo feature graph in `kernex-agent/Cargo.toml`. Mark serve-only and TUI-only deps as `optional = true` and gate them behind features. Annotate the existing source surfaces (`mod serve;`, the `Command::Serve` arm) with `#[cfg(feature = "...")]`. Wire CI to build three variants: minimal, default, full.

This is **pure-config plus mechanical source annotations.** No new runtime logic. No new public API. No semver-relevant change for consumers. The default `cargo install kernex-agent` produces the same binary it produces today, because `default = ["agent-claude", "memory-cli", "serve"]` corresponds to today's behaviour.

The change declares **placeholder features** for surfaces not yet built. Placeholders reserve the cfg shape so subsequent additions are purely additive:

- Adapter features: `agent-claude`, `agent-codex`, `agent-opencode`, `agent-cursor`, `agent-cline`, `agent-windsurf`. Empty arrays except `agent-codex = ["dep:toml_edit"]` and `agent-windsurf = ["agent-cursor"]`.
- Meta features: `agent-shell-cli`, `agent-ide`, `agent-all`.
- Preset features: five empty `preset-*` arrays plus a `preset-all` rollup.
- TUI feature: `tui = ["dep:ratatui", "dep:crossterm"]` with no source files yet beyond a one-line stub.

The placeholder pattern mirrors the templated `binary-size` and `feature-matrix` jobs in `workspace-profile-baseline`'s `size-gate.yml`: declare the surface, leave the body to the change that needs it, hold the shape stable.

## Scope

### In scope

1. **Feature graph in `kernex-agent/Cargo.toml`**:
   - `default = ["agent-claude", "memory-cli", "serve"]`.
   - Core: `memory-cli = []`, `serve = ["dep:axum", "dep:hmac", "dep:subtle", "dep:uuid", "dep:rusqlite"]`, `tui = ["dep:ratatui", "dep:crossterm"]`.
   - Adapters: `agent-claude = []`, `agent-codex = ["dep:toml_edit"]`, `agent-opencode = []`, `agent-cursor = []`, `agent-cline = []`, `agent-windsurf = ["agent-cursor"]`.
   - Meta: `agent-shell-cli = ["agent-claude", "agent-codex", "agent-opencode"]`, `agent-ide = ["agent-cursor", "agent-cline", "agent-windsurf"]`, `agent-all = ["agent-shell-cli", "agent-ide"]`.
   - Presets: five `preset-*` empty arrays plus `preset-all` rolling them up.
   - Existing `bedrock = ["kernex-providers/bedrock"]` preserved as-is.
2. **Dep gating.** Mark `axum`, `hmac`, `subtle`, `uuid`, `rusqlite` as `optional = true`. Add `ratatui`, `crossterm`, `toml_edit` as new optional deps. Leave universal deps (`tokio`, `clap`, `serde`, `serde_json`, `anyhow`, `tracing`, `tracing-subscriber`, `async-trait`, `colored`, `dirs`, `rustyline`, `indicatif`, `toml`, `sha2`, `ureq`) unconditional.
3. **Source-level `#[cfg(feature = "...")]` annotations** in `kernex-agent/src/`:
   - Top-level `mod serve;` and its `use` import gated on `serve`.
   - The `Command::Serve` enum variant in `cli.rs` and the dispatch arm in `main.rs` gated on `serve`. clap-derive 4 supports `#[cfg]` on `Subcommand` variants directly. The three `cli_parses_serve_*` unit tests get the same gate so `cargo test --no-default-features` compiles.
   - One-line stub files: `src/adapters/mod.rs`, `src/adapters/claude.rs`, `src/tui/mod.rs`. They keep the default build linking cleanly without implementing any real adapter or TUI surface.
4. **CI matrix.** Copy `size-gate.yml` from `kernex-dev/kernex` into `kernex-agent/.github/workflows/`, drop the `if: contains(github.repository, 'kernex-agent')` guards (this repo IS that target), and run the three-variant matrix described in Success criteria. Copy `scripts/check-size.sh` and `scripts/diff-bloat.py` from the runtime workspace.
5. **Verify minimal binary is functional.** `cargo build --no-default-features --features memory-cli` produces a binary that compiles cleanly, runs, and prints help. Exit codes are sensible on stub subcommands; no panics.
6. **Pre-commit gate green on all three variants.** `cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo fmt --check` all pass for default, minimal, and full feature sets.

### Out of scope

- Actual adapter implementations for any of `agent-codex`, `agent-opencode`, `agent-cursor`, `agent-cline`, `agent-windsurf`. The features are declared; the modules do not exist beyond the `agent-claude` stub.
- Preset TOML files. The five `preset-*` features are declared as empty arrays only.
- TUI screens, navigation, key bindings. The `tui` feature is declared and the stub module compiles; nothing is rendered.
- Memory-CLI subcommand bodies. The `memory-cli` feature is declared and reserved; subcommand handlers are not touched here.
- Any change to `kernex-runtime`, `kernex-providers`, `kernex-memory`, `kernex-core`, `kernex-pipelines`, or `kernex-sandbox`. This change is `kernex-agent`-only.
- Per-provider feature flags. The native provider matrix in `kernex-providers` stays unconditional.
- Workspace dep inheritance. `kernex-agent` is published independently; `{ workspace = true }` does not apply yet.
- A `tower-http` dep declaration. The current `serve` source does not import any `tower_http::*` symbol. If middleware lands later, `tower-http` becomes a follow-up addition under the `serve` feature.

### Cross-repo coordination

This change is `kernex-agent`-only. It does not pair with a runtime PR. It depends on `workspace-profile-baseline` having landed in `kernex-dev/kernex`, which expanded `[workspace.dependencies]` and shipped `size-gate.yml` with the kernex-agent half guarded by `if: contains(github.repository, 'kernex-agent')`. This change flips that guard.

## Why this scope

- **Foundation, not feature.** Like the runtime-side profile baseline, this installs the levers that every later addition pulls. Skipping it means the next adapter lands as default-on with no escape hatch.
- **Low risk.** Pure config plus mechanical source annotations. No public API change. The default user gets the same binary they get today.
- **Size-budget visibility.** Three variants give CI three reference points. The default ceiling is enforceable today; the minimal and full ceilings become measurable once the graph exists.

## Success criteria

The change ships when:

1. `kernex-agent/Cargo.toml` declares the full feature graph above, including default, core, adapter, preset, meta, and the existing `bedrock` feature.
2. `cargo build` (default) succeeds and produces a binary functionally equivalent to the pre-change `kx`.
3. `cargo build --no-default-features --features memory-cli` produces a functional minimal binary that runs `kx --help` without panicking. Serve-only and adapter-only deps are absent from the dep graph (verified via `cargo tree`).
4. `cargo build --release --features agent-all,tui,serve,preset-all` succeeds. The build proves the feature graph is internally consistent even though most placeholder modules compile to no-ops.
5. CI matrix passes for all three variants. Per-variant binary sizes recorded in the PR description. **Default ceiling 15 MB is the only hard gate this change**: minimal soft-warns at 8 MB but does not fail the matrix; full is informational only (no adapter or TUI source files yet).
6. Pre-commit gate green on all three variants.
7. Default-build size still under 15 MB on macOS aarch64 release.
8. No semver-relevant change to any public surface.

## Risks

- **`default = ["agent-claude", ...]` requires the Claude path to compile cleanly with other adapters off.** The codebase has Claude-specific logic mixed throughout `main.rs` (provider registry, `check_claude_cli`, default-model assignment) without any feature gate, because there is nothing to gate against yet. Mitigation: `agent-claude` is introduced as a no-op (the existing logic stays unconditional), but the cfg surface is reserved. A later change attaches actual `#[cfg(feature = "agent-claude")]` when a dedicated adapter module lands.
- **The `serve` feature must not silently leak `axum` into the minimal variant.** Mitigation: explicit `cargo tree -e normal --no-default-features --features memory-cli` audit during verification; the audit output is committed under `docs/agent-dep-tree-minimal-2026-05-10.txt` for the PR description and future regression checks. CI's `dep-tree-audit` job hard-fails if any of `axum`, `hmac`, `rusqlite`, `ratatui`, `crossterm` appears in the minimal dep tree. The grep deliberately excludes `subtle` and `uuid` because both are structurally transitive (see Finding 2 below).
- **Cargo feature unification across the workspace can surface unexpected combinations.** A downstream test that enables `bedrock` could pull in transitive deps unexpectedly. Mitigation: keep the existing `bedrock = ["kernex-providers/bedrock"]` line as-is; do not add new cross-crate feature passthrough; flag any unification surprise during verification.
- **The 15 MB ceiling could trip if the default profile is heavier than estimated.** Mitigation: run `cargo bloat --release --crates -n 30` against the default variant during verification and commit it under `docs/agent-bloat-2026-05-10-crates.txt`. Net-new default-on dep weight is treated as a regression to investigate before merge.
- **Placeholder feature surface tempts premature implementation.** Mitigation: each placeholder feature gets a one-line comment in `Cargo.toml` saying "do not implement here". Reviewers reject PRs that touch placeholder modules.
- **CI matrix run time.** Mitigation: cache the cargo registry and target dir per variant; soft warning only for run-time growth.

## Pre-implementation baseline findings

Three findings recorded after the pre-change baseline capture (the dated artefacts under `docs/agent-*-baseline-2026-05-10.txt` already on `origin/main`). Each finding has a chosen mitigation for this change plus a tracked follow-up. The findings update posture but do not expand scope.

### Finding 1. Default binary is at 94 percent of the 15 MB ceiling, not 75 percent

`target/release/kx` measured 14,851,984 bytes pre-change (full file `docs/agent-size-baseline-2026-05-10.txt`). Actual headroom is 1.07 MB.

- **Mitigation for this change.** Tighten vigilance only: measure default-variant size at every verification step and surface any net-new default-on weight before merging. Do not alter the 15 MB hard gate; the 13 MB soft-warn at the macOS leg fires pre- and post-change, which is the correct signal.
- **Follow-up.** Reclamation work (narrow `tokio` feature set; narrow `tracing-subscriber` `env-filter`; audit `reqwest` default features given the agent only consumes streaming JSON; check whether `colored` and `indicatif` should sit behind a `tty-output` feature) is tracked as an internal follow-up. Out of scope here.

### Finding 2. Several deps are structurally transitive, not adapter-specific

Three deps that the feature graph nominally gates are in fact pulled transitively by infrastructure crates that every variant (including minimal) compiles in. Their per-feature declarations are documentation-only for size purposes; promoting any of them to a real opt-in needs a workspace-wide refactor of the underlying carrier.

- `toml_edit v0.22.27` is a transitive dep of `toml v0.8.23`. `toml` is a direct dep of `kernex-agent`, `kernex-core`, `kernex-memory`, `kernex-pipelines`, and `kernex-runtime`. The `agent-codex = ["dep:toml_edit"]` line is the intent marker.
- `subtle v2.6.1` is a transitive dep of `rustls v0.23.40`, which `reqwest` (and through it `hyper-rustls`, `tokio-rustls`) and `ureq` both pull unconditionally. The `serve` feature still gates `subtle` as a Cargo dep marker for the manifest, but the binary always carries it.
- `uuid v1.23.1` is a transitive dep of `kernex-core`, which every kernex-agent variant pulls. The `serve` feature gates `uuid` for manifest hygiene only.

The actual savings the `serve` feature still delivers in the minimal variant are `axum`, `hmac`, and `rusqlite` (plus `axum`'s own dep tree: `hyper`, `hyper-util`, `http`). Those three are the only deps the minimal-variant CI dep-tree audit hard-fails on. The audit grep deliberately excludes `subtle` and `uuid` because they cannot be removed at this layer.

- **Mitigation for this change.** Keep the optional Cargo declarations for `subtle` and `uuid` as intent markers (alongside `toml_edit`). Add an inline comment near each in `kernex-agent/Cargo.toml`. The CI dep-tree-audit job in `.github/workflows/size-gate.yml` greps only on `axum|hmac|rusqlite|ratatui|crossterm` for the minimal variant; the comment in the job step explains the omission.
- **Follow-up.** When `rustls` becomes optional (e.g. an HTTP-stack-free build mode for kernex-agent), `subtle` becomes promotable. When `kernex-core` exposes a feature to gate the `uuid` dep, `uuid` becomes promotable. Same pattern for `toml_edit` (gating `toml`). Track the dep tree at each major release.

### Finding 3. The 8 MB minimal target is unreachable in this change alone

The minimal variant `--no-default-features --features memory-cli` drops `axum`, `hyper`, `hyper_util`, `http`, `libsqlite3-sys`, `webpki` (around 500 to 600 KiB visible in the top-30 bloat list, more transitively). It does not drop the unconditional infrastructure (`kernex-runtime`, `kernex-providers`, `kernex-sandbox`, `reqwest`, `rustls`, `tokio`, `clap`, `rustyline`, `indicatif`, `tracing-subscriber`, `toml`, `sha2`, `ureq`), which is roughly 7 to 8 MB on its own. Reaching 8 MB requires a deeper architectural change so `kernex-runtime` composes in only when an adapter is selected.

- **Mitigation for this change.** Treat the 8 MB minimal ceiling as aspirational. The CI matrix soft-warns the minimal variant at 8 MB instead of hard-failing; the minimal variant's actual measured size is recorded in the PR description as the new baseline. The 15 MB default ceiling stays the only hard gate.
- **Follow-up.** A later change must lower the minimal variant binary toward 8 MB by extracting the runtime composition surface so non-runtime variants of `kx` link only what they use. Tracked as an internal follow-up.
