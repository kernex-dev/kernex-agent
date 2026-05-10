# Proposal: Binary-size PR-comment surface

- **Status:** Draft v0.1
- **Author:** Jose Hurtado
- **Repo:** `kernex-dev/kernex-agent`
- **Change ID:** `binary-size-pr-comment`

## Operator friction

Today `kernex-agent` enforces a per-variant binary-size matrix in `.github/workflows/size-gate.yml`. The `default` variant hard-fails at 15 MB; the `minimal` variant soft-warns at 8 MB; the `full` variant is informational. The current size and any ceiling breaches live in workflow logs only.

A reviewer scanning a PR cannot see the size delta without clicking into the workflow run. Drift accumulates across small PRs because no single PR pushes the binary over a ceiling, but the cumulative effect is invisible until ceiling time.

## Solution overview

Adopt the standard two-workflow pattern for PR-comment bots that consume artifacts from `pull_request`-triggered workflows:

1. **`binary-size-build.yml`** — triggers on `pull_request` with read-only token. Builds all three feature-matrix variants (`minimal`, `default`, `full`) on Linux. For each variant: builds the PR head, captures size, then checks out `main` and rebuilds with the same flags (using rust-cache for amortized cost), captures the baseline size, computes delta in bytes. Writes per-variant fragments to artifacts. The `default` variant retains its existing 15 MB hard-fail via `scripts/check-size.sh`. A `report` job aggregates fragments into `size_report.md` (Markdown table with `| Variant | Bytes | Δ vs main | Status |`) and uploads the `size-report` artifact.
2. **`binary-size-comment.yml`** — triggers on `workflow_run` of the build workflow with `pull-requests: write` permission. Downloads the `size-report` artifact, finds any existing sticky `## Binary Sizes` comment by `peter-evans/find-comment`, and replaces or creates it via `peter-evans/create-or-update-comment`. One sticky comment per PR; pushed updates replace the comment in place.

The two-workflow split is the standard GitHub security pattern: the build runs untrusted PR code with read-only permissions; the comment-write step runs from `main` via `workflow_run` with the higher permission scope. Mixing build and comment in a single workflow on `pull_request_target` would expose the write token to PR-controlled checkout state and is rejected.

## Why these specific tools

`peter-evans/find-comment@v3` and `peter-evans/create-or-update-comment@v4` are MIT-licensed, well-maintained, widely-adopted GitHub Actions used by major OSS projects for sticky-comment patterns. They are SHA-pinned to specific commits matching existing workspace discipline.

## Scope

### In scope

1. New file `.github/workflows/binary-size-build.yml` with a `build` matrix job (3 variants) and a `report` job that aggregates fragments into `size_report.md`.
2. New file `.github/workflows/binary-size-comment.yml` with a single `comment` job triggered on `workflow_run` completion of `binary-size-build`.
3. Edit `.github/workflows/size-gate.yml` to remove the `binary-size` (macOS-aarch64) and `feature-matrix` (Linux 3-variant) jobs that moved to `binary-size-build.yml`. Keep `dep-tree-audit`, `bloat`, and `unused-deps`.
4. SHA-pinned actions: `actions/checkout`, `dtolnay/rust-toolchain`, `Swatinem/rust-cache`, `actions/upload-artifact@v4`, `actions/download-artifact@v4`, `peter-evans/find-comment@v3`, `peter-evans/create-or-update-comment@v4`.
5. Hard-fail discipline preserved: the `default` variant still hard-fails at 15 MB.

### Out of scope

- macOS-aarch64 standalone hard-ceiling check (the previous `binary-size` job in `size-gate.yml`). Per the SDD's spec, the new binary-size-build.yml runs on Linux only; the macOS-specific gate is dropped because (a) the kernex.dev public claim is platform-generic ("Single Binary, Under 15 MB"), and (b) the Linux default-variant hard-fail catches the same regression class. If macOS-specific drift becomes a concern a follow-up PR can re-introduce a macOS-only job.
- `dep-tree-audit`, `bloat`, `unused-deps` jobs — unchanged in `size-gate.yml`.
- Cold-start benchmark deltas as PR comments. Out of scope for this change; potential follow-up.

## Success criteria

1. `binary-size-build.yml` and `binary-size-comment.yml` committed under `.github/workflows/` with all third-party actions SHA-pinned.
2. `size-gate.yml` retains only `dep-tree-audit`, `bloat`, `unused-deps`.
3. A throwaway PR opened against `main` triggers `binary-size-build`. Within ~5 minutes the sticky `## Binary Sizes` comment appears on the PR with three rows (minimal / default / full) showing bytes, delta vs main, and status. Pushing a second commit replaces the comment in place rather than appending.
4. Hard-fail behavior preserved: a synthetic PR that pushes the default variant over 15 MB still fails the build job.

## Risks

- **`workflow_run` race on rapid push.** Two-workflow patterns can race if `peter-evans/find-comment` runs between two `binary-size-build` invocations. The `body-includes` filter is idempotent and the worst case is a transient duplicate comment that the next push reconciles.
- **Build cost doubled per PR (PR head + main baseline).** Rust-cache amortizes the cost; the second build is fast on a cache hit. If cache miss rates rise, future iteration can compute baseline from a stored CI artifact instead of rebuilding main per PR.
- **Linux numbers diverge from the macOS public claim.** kernex.dev advertises platform-generic binary-size claims; the comment surface presents Linux numbers explicitly. Macros differ but the regression-detection signal is preserved.
- **Action SHA pin maintenance.** SHA pins lock the specific reviewed code path; major-version refs (`@v3`, `@v4`) are not used. Updating action versions requires a follow-up PR with the new SHA documented.
