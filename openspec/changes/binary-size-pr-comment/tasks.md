# Tasks: Binary-size PR-comment surface

> **Reference:** [proposal.md](proposal.md). Each task is sized at roughly two focused hours.

---

## Step 0 — Pre-execution audit

### P0-1. Confirm baseline build is clean

- `cargo build --release --bin kx --locked` succeeds.
- `cargo test --locked` passes.
- `cargo clippy --locked -- -D warnings` clean.
- `cargo fmt --check` clean.

**Verification:** all four commands exit 0 on a freshly checked-out `main`.

### P0-2. Confirm current size-gate.yml shape

- `cat .github/workflows/size-gate.yml | grep -E '^  [a-z-]+:' | head` returns `binary-size`, `feature-matrix`, `dep-tree-audit`, `bloat`, `unused-deps` as the existing job names.

**Verification:** the workflow has the expected jobs before edits begin.

---

## Step 1 — Author new workflows

### P1-1. Create `binary-size-build.yml`

- New file `.github/workflows/binary-size-build.yml`.
- Triggers on `pull_request` to `main`.
- Permissions: `contents: read`.
- `build` matrix job with 3 variants (`minimal`, `default`, `full`); each variant builds PR head, runs hard-ceiling check (default only), then builds main baseline, computes delta. Writes per-variant fragment to artifact `size-fragment-${name}`.
- `report` job depends on `build` (`if: always()`); downloads fragments, composes `size_report.md` with table `| Variant | Bytes | Δ vs main | Status |`, captures `pr_number.txt`, uploads `size-report` artifact (retention 7 days).
- All third-party actions SHA-pinned with version comment.

**Verification:** `actionlint` clean. The matrix variants and ceilings match the previous `feature-matrix` job in `size-gate.yml`.

### P1-2. Create `binary-size-comment.yml`

- New file `.github/workflows/binary-size-comment.yml`.
- Triggers on `workflow_run` of `binary-size-build` with `types: [completed]`.
- Permissions: `pull-requests: write`, `contents: read`.
- Single `comment` job: skips if `event.workflow_run.event` is not `pull_request`; downloads `size-report` artifact via `actions/download-artifact@v4` with explicit `run-id` and `github-token`; reads `pr_number.txt`; finds existing sticky comment via `peter-evans/find-comment@v3` filtering on `comment-author: github-actions[bot]` and `body-includes: "## Binary Sizes"`; creates or updates via `peter-evans/create-or-update-comment@v4` with `edit-mode: replace`.

**Verification:** `actionlint` clean. The `workflow_run` trigger reads from the right workflow name.

### P1-3. Retire moved jobs from `size-gate.yml`

- Edit `.github/workflows/size-gate.yml`.
- Remove the `binary-size` job (macOS-aarch64 default-variant hard-ceiling check).
- Remove the `feature-matrix` job (Linux 3-variant matrix).
- Update the leading job comment block to describe the remaining scope (`dep-tree-audit`, `bloat`, `unused-deps`) and reference `binary-size-build.yml` for the size matrix.
- Keep `dep-tree-audit`, `bloat`, `unused-deps` unchanged.

**Verification:** `cat .github/workflows/size-gate.yml | grep -E '^  [a-z-]+:'` returns only `dep-tree-audit`, `bloat`, `unused-deps`. No edits inside those jobs.

---

## Step 2 — Verification gate

### P2-1. Workflow lint

- `actionlint` (or equivalent) over all three workflow files (`binary-size-build.yml`, `binary-size-comment.yml`, `size-gate.yml`).

**Verification:** all three parse and reference valid action SHAs.

### P2-2. Manual end-to-end test on throwaway PR

- After merge, open a throwaway PR with a no-op change.
- Confirm `binary-size-build` runs on the PR. All three matrix variants succeed (or default variant hard-fails if the change happens to push over 15 MB).
- Within 3-5 minutes, confirm the sticky `## Binary Sizes` comment appears on the PR.
- Push a second commit; confirm the comment is replaced in place rather than appended.
- Close the PR without merging.

**Verification:** sticky comment shows three rows with bytes, delta vs main, and per-variant status. Workflow timing recorded for the post-merge note.

### P2-3. Hard-fail regression check

- Open a synthetic PR that intentionally pushes the default variant over 15 MB (e.g., adds a large dep behind a default-on feature).
- Confirm `binary-size-build`'s `build` matrix job for the `default` variant fails at the `Hard ceiling check` step.
- Close the PR without merging.

**Verification:** ceiling-breach hard-fails at the build step, not just at the comment step.

---

## Step 3 — Archive

### P3-1. Move the change directory to archive

- After merge, move `openspec/changes/binary-size-pr-comment/` to `openspec/archive/2026-MM-binary-size-pr-comment/`.
- Add a "Post-merge notes" section to the archived `proposal.md` recording the merge SHA, the throwaway PR numbers used for verification, and any drifts vs the spec.

**Verification:** `ls openspec/archive/ | grep binary-size-pr-comment` returns the archived directory. `ls openspec/changes/binary-size-pr-comment/` no longer exists.

---

## Done criteria

- `binary-size-build.yml` and `binary-size-comment.yml` committed and `actionlint`-clean.
- `size-gate.yml` retains only `dep-tree-audit`, `bloat`, `unused-deps`.
- Sticky `## Binary Sizes` comment demonstrably appears on a throwaway PR with all three variant rows.
- Hard-fail behavior preserved at the default variant's 15 MB ceiling.
- Change directory archived under `openspec/archive/2026-MM-binary-size-pr-comment/`.
