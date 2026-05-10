# Tasks: Cargo feature graph for kernex-agent

> **Reference:** [proposal.md](proposal.md).
> Each task is sized at roughly two focused hours. Change tag: `[s1-b]`.

## Coordination

Single-repo `kernex-agent`-only. No paired runtime PR. Depends on `workspace-profile-baseline` having landed in `kernex-dev/kernex`, which expanded `[workspace.dependencies]` and shipped `size-gate.yml` with the kernex-agent half guarded by `if: contains(github.repository, 'kernex-agent')`. This change flips that guard.

Pre-commit gate (must pass on each variant being touched, before each commit):

```
cargo build
cargo clippy --all-targets -- -D warnings
cargo test
cargo fmt --check
```

No `Co-Authored-By` trailers. No `--no-verify`. No auto-commit.

## Step 0: pre-execution audit (gates Step 1)

### P0-1. Verify upstream `[workspace.dependencies]` reachability `[s1-b]`

Inspect the runtime workspace's root `Cargo.toml` for the pins this change needs. If any pin is missing, file a follow-up against `kernex-dev/kernex`. Do not silently add new pins from inside `kernex-agent`.

### P0-2. Confirm dep-version alignment with the runtime workspace `[s1-b]`

`kernex-agent/Cargo.toml` pins `kernex-runtime = "0.5.0"` and four sister crates at the same version, sourced from crates.io. Run `cargo tree -p kernex-agent --depth 1`. If any `kernex-*` version drifts from the published baseline, reconcile before introducing feature flags.

### P0-3. Confirm pre-change baselines are present `[s1-b]`

The pre-change baselines were captured at commit `2385855` (already on `origin/main`):

- `docs/agent-size-baseline-2026-05-10.txt`
- `docs/agent-bloat-baseline-2026-05-10-crates.txt`
- `docs/agent-bloat-baseline-2026-05-10-functions.txt`
- `docs/agent-dep-tree-baseline-2026-05-10.txt`

Verify all four artefacts exist on `main`. They are the reference for verifying "no behaviour change" in Step 5.

**What to verify before Step 1:** all four baseline artefacts present; default `cargo build` clean against current `main`.

## Step 1: feature graph in Cargo.toml

### P1-1. Author the `[features]` section `[s1-b]`

Replace the empty feature space with:

- `default = ["agent-claude", "memory-cli", "serve"]`.
- Core: `memory-cli = []`, `serve = ["dep:axum", "dep:hmac", "dep:subtle", "dep:uuid", "dep:rusqlite"]`, `tui = ["dep:ratatui", "dep:crossterm"]`.
- Adapters: `agent-claude = []`, `agent-codex = ["dep:toml_edit"]`, `agent-opencode = []`, `agent-cursor = []`, `agent-cline = []`, `agent-windsurf = ["agent-cursor"]`.
- Meta: `agent-shell-cli = ["agent-claude", "agent-codex", "agent-opencode"]`, `agent-ide = ["agent-cursor", "agent-cline", "agent-windsurf"]`, `agent-all = ["agent-shell-cli", "agent-ide"]`.
- Presets: five `preset-*` empty arrays plus `preset-all` rolling them up.
- Existing: `bedrock = ["kernex-providers/bedrock"]` preserved as-is.

### P1-2. Inline placeholder comments `[s1-b]`

Each placeholder feature (every adapter except `agent-claude`, `tui`, every preset) gets a one-line comment marking it as a placeholder. Intent: reviewers reject PRs that try to land an implementation under any of these flags.

### P1-3. Verify default build clean `[s1-b]`

`cargo build` and `cargo clippy --all-targets -- -D warnings` against default features. Default behaviour must be unchanged.

**What to verify before Step 2:** default `cargo tree -e normal --no-dedupe | wc -l` matches the pre-change snapshot to within plus or minus two lines.

## Step 2: dep gating

### P2-1. Move serve-only deps to optional `[s1-b]`

Mark `axum`, `hmac`, `subtle`, `uuid`, `rusqlite` as `optional = true`. The `serve = ["dep:axum", ...]` feature from P1-1 activates them. Verify with:

```
cargo tree --no-default-features --features memory-cli -e normal | grep -E 'axum|hmac|subtle|uuid|rusqlite'
```

Expected output: zero matches.

### P2-2. Add TUI placeholder deps as optional `[s1-b]`

Add `ratatui` and `crossterm` to `[dependencies]` with `optional = true`. They do not exist in the current `Cargo.toml`. Gate them behind `tui = ["dep:ratatui", "dep:crossterm"]`. No source uses them yet; `cargo build --features tui` produces an empty link beyond the stub module from P3-5.

### P2-3. Add adapter-shape placeholder dep `[s1-b]`

Add `toml_edit` as optional, gated behind `agent-codex`. Existing `toml = "0.8"` (used by `config.rs` for project-TOML parsing) stays unconditional. Document the intent-marker note in `Cargo.toml` per Finding 2 in `proposal.md`.

### P2-4. Audit unconditional deps `[s1-b]`

These stay direct, unconditional pins because every variant including the minimal build exercises them: `kernex-runtime`, `kernex-core`, `kernex-providers`, `kernex-pipelines`, `kernex-sandbox`, `tokio`, `clap`, `serde`, `serde_json`, `anyhow`, `tracing`, `tracing-subscriber`, `async-trait`, `colored`, `dirs`, `rustyline`, `indicatif`, `toml`, `sha2`, `ureq`. If any of them turns out to be serve-only or adapter-only during Step 4 audit, file a follow-up.

### P2-5. Defer workspace inheritance `[s1-b]`

`kernex-agent` is published to crates.io independently and is not a workspace member of `kernex-dev/kernex`, so `{ workspace = true }` does not apply yet. Folding `kernex-agent` into the runtime workspace is a separate follow-up.

### P2-6. Verify build on default and minimal `[s1-b]`

`cargo build` (default) and `cargo build --no-default-features --features memory-cli` (minimal) both succeed and produce a runnable `kx`. The minimal `cargo tree -e normal | wc -l` is strictly less than the default tree count from P0-3. Document the delta in the PR description.

## Step 3: source-level cfg annotations

### P3-1. Gate `mod serve;` in `src/main.rs` `[s1-b]`

Wrap `mod serve;` and `use crate::serve::cmd_serve;` in `#[cfg(feature = "serve")]`.

### P3-2. Gate `Command::Serve` arm in `cli.rs` and `main.rs` `[s1-b]`

Add `#[cfg(feature = "serve")]` to the `Command::Serve { ... }` enum variant in `cli.rs` and to the `match` arm in `main.rs` that dispatches it. Builds without `serve` produce a `kx` whose top-level help omits the subcommand. Apply the same gate to the three `cli_parses_serve_*` unit tests so `cargo test --no-default-features` compiles.

### P3-3. Reserve the `memory-cli` gate (no source moves) `[s1-b]`

The `memory-cli` feature is declared in P1-1; **no source moves in this change** because the memory CLI surface is not introduced here. Slash-command branches in `commands.rs` stay unconditional. Sanity check: `commands.rs` does not import any module that this change is gating.

### P3-4. Reserve the `agent-*` gate scaffold `[s1-b]`

Add `src/adapters/mod.rs` with `#[cfg(feature = "agent-X")] pub mod X;` lines for the adapter features. For this change, ship a one-line stub at `src/adapters/claude.rs` so the default build (which enables `agent-claude`) compiles. The other four per-adapter source files do **not** exist yet; activating any of `agent-codex`, `agent-opencode`, `agent-cursor`, `agent-cline`, `agent-windsurf` deliberately fails the build until the matching module is added.

### P3-5. Reserve the `tui` gate scaffold `[s1-b]`

Add `src/tui/mod.rs` as a one-line stub. Add `#[cfg(feature = "tui")] mod tui;` to `main.rs`. `cargo build --features tui` compiles the stub.

### P3-6. Verify all three variants build `[s1-b]`

```
cargo build                                                            # default
cargo build --no-default-features --features memory-cli                # minimal
cargo build --features agent-all,tui,serve,preset-all                  # full
```

All three succeed. Default and minimal produce a runnable `kx`. Full is allowed to differ from default only in linked-but-unused stub code.

**What to verify before Step 4:** `kx --help` output is identical between pre-change and default; minimal omits `serve` from help; full is functionally identical to default for now.

## Step 4: CI matrix

### P4-1. Copy `size-gate.yml` from the runtime repo `[s1-b]`

The runtime workspace ships `.github/workflows/size-gate.yml` with `binary-size` and `feature-matrix` jobs guarded by `if: contains(github.repository, 'kernex-agent')`. Copy the file into `kernex-agent/.github/workflows/size-gate.yml` and remove the guard so both jobs activate here. Copy `scripts/check-size.sh` and `scripts/diff-bloat.py` from the runtime repo.

### P4-2. Wire the three-variant feature matrix `[s1-b]`

The `feature-matrix` job runs `cargo build --release` against three configurations:

- **minimal**: `--no-default-features --features memory-cli`. **Soft-warn at 8 MB** (8388608 bytes). Informational this change per Finding 3 in `proposal.md`. The matrix records and posts the measured size but does **not** fail on it.
- **default**: no flags. Ceiling 15 MB (15728640 bytes). **Hard gate.**
- **full**: `--features agent-all,tui,serve,preset-all`. Informational only this change (no adapter source files yet).

The default leg pipes its binary into `scripts/check-size.sh` for the ceiling check. The minimal and full legs record their size to the workflow summary and post a warning via `::warning::` if the size grows; neither fails the matrix.

### P4-3. Wire the macOS aarch64 default ceiling `[s1-b]`

The `binary-size` job runs on `macos-latest` (the canonical headline target), builds the default `kx`, and fails on greater than 15 MB (15728640 bytes). Soft warn at 13 MB by emitting `::warning::` without failing the check.

### P4-4. Audit the minimal-variant dep graph `[s1-b]`

A dedicated `dep-tree-audit` CI job runs:

```
cargo tree -e normal --no-default-features --features memory-cli > /tmp/dep-tree-minimal.txt
grep -E 'axum|hmac|subtle|uuid|rusqlite|ratatui|crossterm' /tmp/dep-tree-minimal.txt && exit 1 || echo OK
```

The grep MUST return zero matches; if any serve-only or TUI dep leaks into minimal, the job hard-fails. Commit the dep-tree file as `docs/agent-dep-tree-minimal-2026-05-10.txt` for the PR description and as a regression baseline. Note the audit grep deliberately omits `subtle` and `uuid` because they are pulled transitively via `rustls`/`reqwest`/`ureq` and `kernex-core` respectively, and cannot be removed at this layer (per Finding 2 in `proposal.md`).

### P4-5. Capture default-build bloat artefacts `[s1-b]`

```
cargo bloat --release --crates -n 30 > docs/agent-bloat-2026-05-10-crates.txt
cargo bloat --release -n 30        > docs/agent-bloat-2026-05-10-functions.txt
ls -lh target/release/kx           > docs/agent-size-2026-05-10.txt
```

Commit. These are the post-change companions to the pre-change baselines under `docs/agent-*-baseline-2026-05-10.txt` from P0-3. The diff is the source of truth for "no net-new default-on weight".

**What to verify before Step 5:** default `kx` binary strictly under 15 MB on macOS aarch64; under 13 MB earns no warning. Record actual size in the PR description.

## Step 5: verification

### P5-1. Pre-commit gate on default `[s1-b]`

`cargo build`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo fmt --check`. All four green.

### P5-2. Pre-commit gate on minimal `[s1-b]`

`cargo build --no-default-features --features memory-cli`, plus the matching `clippy` and `test` invocations with the same flags. All three green.

### P5-3. Pre-commit gate on full `[s1-b]`

`cargo build --features agent-all,tui,serve,preset-all`, plus matching `clippy` and `test` invocations. All three green.

### P5-4. Run `cargo audit && cargo deny check` `[s1-b]`

Both green. The new optional deps (`ratatui`, `crossterm`, `toml_edit`) must clear license and advisory checks.

### P5-5. Run `cargo machete` `[s1-b]`

`cargo machete` operates on the manifest, not on a feature set. If it flags any new optional dep (because no source under `default` activates it yet), add to `[package.metadata.cargo-machete] ignored = [...]` with a comment naming the feature that consumes it.

### P5-6. Smoke test minimal binary `[s1-b]`

Build the release minimal binary. Run:

- `kx --help`: must succeed; must not list `serve`.
- `kx mem --help`: acceptable to print empty subcommand help or a short stub message; must not panic.
- `kx serve --help`: MUST fail with a "no such command" exit code.

Document each command's output in the PR description.

## Step 6: archive and post-merge

### P6-1. Archive the change inside the agent repo `[s1-b]`

After the openspec change and the code changes merge to `kernex-agent/main`, move `openspec/changes/cargo-feature-graph/` to `openspec/archive/2026-05-cargo-feature-graph/`. Add a one-line header to each archived file noting the merge date and commit SHA.

### P6-2. Note any opt-in deps that did not land `[s1-b]`

If any of `ratatui`, `crossterm`, `toml_edit` did not survive `cargo audit` or `cargo deny check`, document the swap (or removal) in the merged change's archived `proposal.md` "Risks" section.

## What is intentionally absent

- Adapter implementations for `agent-claude`, `agent-codex`, `agent-opencode`, `agent-cursor`, `agent-cline`, `agent-windsurf`. The features are declared; bodies are not.
- Preset TOML files. The five `preset-*` features are declared as empty arrays only.
- TUI screens, navigation, key bindings.
- Memory CLI subcommand bodies.
- Workspace inheritance for `kernex-agent`'s deps.
- Per-provider feature flags.
- Multi-platform release artefacts for the minimal and full variants.
- A `tower-http` dep declaration; the current `serve` source does not use any `tower_http::*` symbol, so adding it now would be dead optional weight that `cargo machete` would flag.
