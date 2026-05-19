# Codex CLI adapter

`kx install --agent codex --preset solo-dev` wires kernex into [OpenAI Codex CLI](https://github.com/openai/codex). After install, Codex sees a kernex MCP server in its server registry and reads the same per-project instruction surface that other shell-CLI agents use.

This page covers what the install writes, where, and how to roll back. The configurator runs the same 7-stage pipeline as every other adapter (DETECT, RESOLVE, REVIEW, BACKUP, APPLY, VERIFY, REPORT); only the per-file logic differs.

## Prerequisites

- Codex CLI on your `PATH`. If it is missing, the configurator surfaces the canonical install one-liner:

  ```bash
  npm install -g @openai/codex
  ```

- Your `$HOME` is writable for `~/.codex/`. The configurator creates the directory if absent.

## What the install writes

| Component       | Path                            | Format     | Merge behaviour                                                                 |
|-----------------|---------------------------------|------------|---------------------------------------------------------------------------------|
| `config-toml`   | `~/.codex/config.toml`          | TOML       | `[mcp_servers.kernex]` is upserted by name. Existing `[mcp_servers.*]` entries and unrelated tables are preserved byte-for-byte (formatting via `toml_edit`). |
| `output-style`  | `~/.codex/output-style.md`      | Markdown   | Overwritten with the kernex voice template. The previous content is captured in the BACKUP tarball before APPLY writes.                                       |
| `agents-md`     | `<cwd>/AGENTS.md`               | Markdown   | A `<!-- kernex:begin -->` / `<!-- kernex:end -->` block is inserted (or replaced in-place if already present). Content outside the block is left untouched.   |

The `<cwd>` for `agents-md` is the working directory at the moment `kx install` ran. That directory becomes the project-local allowlisted root for Stage 5 writes per ADR-001 in the codex-cli-adapter openspec change.

## Rollback

Every install run drops a tarball under `~/.kx/backups/` named after the run's timestamp. Two ways to roll back:

1. Automatic rollback. If APPLY fails midway, the configurator walks the receipt list in reverse and restores from the backup tarball before exiting non-zero. No manual step needed.

2. Manual rollback. To revert a successful install, extract the most recent backup tarball over `~/.codex/` and the project root:

   ```bash
   tar -xzf ~/.kx/backups/<run-id>.tar.gz -C /
   ```

   The tarball's internal paths are absolute, so this restores both home-rooted and project-rooted files in one extract.

3. Surgical rollback for `AGENTS.md` only. Delete the marker block manually:

   ```bash
   sed -i '' '/<!-- kernex:begin -->/,/<!-- kernex:end -->/d' AGENTS.md
   ```

   This leaves the rest of your `AGENTS.md` intact.

## Verifying the install

Stage 6 VERIFY checks file presence and SHA256 against the receipts emitted by APPLY. Re-run the check at any time:

```bash
kx install --agent codex --preset solo-dev --verify-deep --dry-run
```

`--dry-run` exits cleanly after REVIEW; combined with `--verify-deep`, the run reports any drift between what kernex thinks it wrote and what is currently on disk.

## What this adapter does NOT change

- Codex's own login state (`~/.codex/auth.json` and similar) is never read or written.
- Other `[mcp_servers.*]` entries you have configured are preserved.
- Files outside the three components above are not touched.

## Feature flag

This adapter ships behind `--features agent-codex`:

```bash
cargo install kernex-agent --features agent-codex
```

The default `cargo install kernex-agent` build does NOT include this adapter so the binary stays under the 15 MiB macOS aarch64 product commitment. Per F-LOCK-03, enabling `agent-codex` adds at most 800 KiB on macOS aarch64.
