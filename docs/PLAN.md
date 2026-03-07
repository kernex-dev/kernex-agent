# kx — CLI Dev Agent Plan

## Overview

`kx` is a CLI agent built on top of Kernex that serves as a project-aware
development assistant for independent developers managing multiple projects
across different stacks.

## User Profile

- ~90 repos across JS/TS, Python, Rust, Flutter, PHP, HTML emails
- ~15 active projects at any time
- Mix of own products (uxuiprinciples, interfaceaudit, vesti, pagafly, kernex)
  and client work (Florastor, Farmasi, pasadena-chamber, lasernivelacion)
- Heavy context switching between projects
- Extensive documentation that is often outdated or from pre-pivot versions
- ~30 repos inactive for 1+ year, candidates for archival

## Three Modes

### 1. `kx dev` — Daily Development

Run tasks in the current project directory:
```
cd pagafly && kx "add validation to the checkout form"
cd kernex-dev && kx "add retry logic to the anthropic provider"
```

- Detects stack from project files (package.json, Cargo.toml, etc.)
- Maintains per-project memory (conversation history, decisions, context)
- Activates stack-specific skills automatically
- Uses Claude Code CLI as provider (Max subscription, no API key needed)
- Sandbox protection always on

### 2. `kx audit` — Repository Health

Scan and audit the project landscape:
```
kx audit ~/Documents/GitHub/
```

- Classifies repos by activity, stack, and health
- Detects: inactive repos, duplicates/backups, exposed secrets, missing .gitignore
- Suggests actions: archive, clean, update, delete
- Generates markdown report

### 3. `kx docs` — Documentation Audit

Verify documentation against actual code:
```
cd vesti-app && kx docs
```

- Reads existing docs (README, docs/, wiki, inline)
- Compares against current codebase
- Detects: stale features, changed APIs, obsolete configs, pivot remnants
- Generates gap analysis or updates docs directly (with confirmation)

## Architecture

```
kernex-agent (this repo)
  depends on: kernex-runtime (from crates.io)

kx binary
  ├── CLI parser (clap)
  ├── Project detector (stack, config, git status)
  ├── RuntimeBuilder → Runtime
  │     ├── Store (per-project memory)
  │     ├── Skills (stack-specific, loaded from ~/.kernex/skills/)
  │     └── Provider (Claude Code CLI)
  └── Output (terminal, markdown reports)
```

## Phased Implementation

### Phase 1: kx dev (MVP)
- Repo: kernex-agent
- Basic CLI: `kx "message"` runs in pwd
- Claude Code as provider
- Per-directory memory isolation
- Stack detection (read-only, for context in prompt)

### Phase 2: kx audit
- Skill that scans a directory tree of repos
- Classification by activity, stack, health
- Markdown report generation
- Suggested actions (archive, clean, update)

### Phase 3: kx docs
- Skill that reads docs/ and README files
- Compares against source code
- Gap analysis report
- Direct doc updates with confirmation

## Landscape Snapshot (2026-03-07)

### Active Projects (last 48h)
| Project | Stack | Last Activity |
|---------|-------|---------------|
| uxuiprinciples-web | JS/TS | 14 min |
| uxuiprinciples-chatgpt-app | JS/TS | 22 min |
| kernex-dev | Rust | 32 min |
| gpt-researcher-tool | Python | 81 min |
| visualbrands-reports-builder | Other | 2h |
| Doli-Producer | Rust | 19h |
| omega-fork | Other | 22h |
| VPS1/VPS2 | Config | 28h |
| interfaceaudit-web | JS/TS | 28h |
| jhurtado-portfolio | JS/TS | 29h |
| GEOAutopilot | Other | 27h |

### By Stack
| Stack | Count |
|-------|-------|
| JS/TS | 25 |
| Python | 8 |
| Rust | 2 |
| Flutter | 1 |
| Other (emails, WP, configs) | ~55 |

### Archival Candidates
- ~20 Florastor/Vitaflo email repos (1+ year inactive)
- 3+ explicit backups (The-UX-Playbook-backup, dx_kulture_V2-backup)
- ~30 repos with no activity in 1+ year
