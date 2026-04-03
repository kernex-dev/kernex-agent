# kx serve — VPS Implementation Plan

> Tracks all planned and completed work for headless server deployment,
> skills infrastructure, and workflow management.

---

## Status Overview

| Area | Status | Notes |
|---|---|---|
| `kx serve` core (HTTP daemon) | DONE | Axum 0.8, Bearer auth, job queue |
| Docker deployment files | DONE | Dockerfile, docker-compose, Caddyfile |
| Security hardening | DONE | Non-root, read-only FS, TLS 1.3, rate limiting |
| Skills loading in serve mode | NOT STARTED | `run_agent` uses hardcoded system prompt |
| Workflow file system | NOT STARTED | No named workflow support |
| Webhook HMAC verification | NOT STARTED | All webhooks share the same bearer token |
| Job persistence | NOT STARTED | In-memory only, lost on restart |
| `/health` job stats | NOT STARTED | Returns static `{status: ok}` only |

---

## Research Findings: Skills Format

### Official Anthropic Skills Spec

From `platform.claude.com/docs/en/agents-and-tools/agent-skills/overview` and
`github.com/anthropics/skills`:

**Minimal required schema:**
```
---
name: skill-name          # max 64 chars, lowercase + hyphens only
description: ...          # max 1024 chars, what it does AND when to use it
---

# Skill content in markdown
```

**Discovery model (3 levels):**
- Level 1 (always): `name` + `description` injected into system prompt (~100 tokens per skill)
- Level 2 (on trigger): full SKILL.md body read by agent via bash
- Level 3 (as needed): bundled files (FORMS.md, REFERENCE.md, scripts/) read on demand

**Trust model:**
- No formal permission metadata in the spec
- Trust is determined by source reputation and manual audit
- Skills can execute bash, run scripts, make network calls (surface-dependent)

**`anthropics/skills` repo:**
- 25 document + example skills (docx, pdf, pptx, xlsx, claude-api, etc.)
- Python-heavy (84.4%)
- Apache 2.0 for most; source-available for document skills

### kx Skills Format vs Anthropic Spec

kx uses an extended SKILL.md format that is a **superset** of the official spec:

```toml
---
name = "devops-automator"
description = "..."
version = "0.1.0"                  # kx extension
trigger = "devops|ci/cd|pipeline"  # kx extension: auto-activation keywords
[permissions]                       # kx extension: permission model
files = ["read:src/**", ...]
commands = ["docker", "kubectl"]
[toolbox.docker_build]             # kx extension: structured tool definitions
description = "Build a Docker image"
command = "docker"
---
```

**Parser gap (`src/skills/parser.rs`):**

| Field | Parsed? | Used? |
|---|---|---|
| `name` | YES | YES |
| `description` | YES | YES |
| `permissions` | YES (list format) | YES (sandboxed/standard/trusted) |
| `version` | NO (silently ignored) | NO |
| `trigger` | NO (silently ignored) | NO |
| `[toolbox.*]` | NO (silently ignored) | NO |

The `trigger` and `toolbox` fields are defined in all 12 builtins but never parsed.
The official Anthropic format uses `name` + `description` only — kx's extended
fields are not cross-compatible with Claude.ai or the Anthropic API surface.

### Key Decision

kx's extended format (trigger, toolbox, permissions) is valuable for the CLI use
case. For serve mode, the priority is:
1. Get skills loaded into `run_agent` at all (current gap)
2. Implement `trigger` for auto-activation in serve mode
3. `toolbox` can wait — the agent can infer tool use from skill instructions

---

## Proposed Architecture: Serve Mode Skills + Workflows

### 1. Skills Loading in `run_agent`

`src/serve/mod.rs` — `run_agent` needs to:
1. Load installed skills from `~/.kx/skills/` (global) and the project skills dir
2. Build a system prompt that includes skill metadata (Level 1 discovery)
3. Pass the full skill content when a trigger matches or a skill is explicitly named

```rust
// Current (hardcoded):
let system_prompt = "You are a helpful AI assistant running in headless server mode.";

// Target:
let skills = load_serve_skills(&data_dir, &req.skills);
let system_prompt = build_serve_system_prompt(&skills);
```

### 2. Workflow File System

Named, reusable job configurations stored in `~/.kx/workflows/` or the
volume-mounted `deploy/workflows/` directory.

**Schema (`workflows/<name>.toml`):**
```toml
[workflow]
name        = "deploy-verify"
description = "Run after every production deployment"
skills      = ["devops-automator", "security-engineer"]
project     = "production"
channel     = "deploy"
max_turns   = 15
system_prompt = """
Optional override. If omitted, uses the standard serve prompt
augmented by the listed skills.
"""
```

**`RunBody` change needed (`src/serve/routes.rs`):**
```rust
pub struct RunBody {
    pub message: String,
    pub workflow: Option<String>,   // NEW: name of workflow to load
    pub skills: Option<Vec<String>>, // NEW: explicit skill list (overrides workflow)
    pub provider: Option<String>,
    pub model: Option<String>,
    pub project: Option<String>,
    pub channel: Option<String>,
    pub max_turns: Option<usize>,
}
```

**Priority/override chain:**
```
request.skills > workflow.skills > channel default > none
request.project > workflow.project > serve default
request.max_turns > workflow.max_turns > serve default
```

### 3. Webhook-to-Workflow Auto-Mapping

`POST /webhook/deploy` already sets `channel = "webhook-deploy"`.
With workflows, the handler looks up `workflows/webhook-deploy.toml` automatically.
No code change needed — the convention does the routing.

### 4. Volume Structure in Docker

```
deploy/
├── skills/                     mounted :ro at /home/kx/.kx/skills/
│   ├── devops-automator/SKILL.md
│   ├── agents-orchestrator/SKILL.md
│   ├── security-engineer/SKILL.md
│   ├── backend-architect/SKILL.md
│   ├── api-tester/SKILL.md
│   ├── reality-checker/SKILL.md
│   └── senior-developer/SKILL.md
└── workflows/                  mounted :ro at /home/kx/.kx/workflows/
    ├── deploy-verify.toml
    ├── security-audit.toml
    ├── service-health.toml
    └── incident-response.toml
```

---

## Pre-Configured Skills Set for Serve Mode

From the 12 existing builtins, these 7 are appropriate for headless automation:

| Skill | Role in Serve Mode |
|---|---|
| `agents-orchestrator` | Coordinates multi-step, multi-skill workflows |
| `devops-automator` | Deployment checks, CI/CD, infra management |
| `security-engineer` | Security audits, vulnerability scanning |
| `backend-architect` | API design, service architecture reviews |
| `api-tester` | Endpoint testing, integration verification |
| `reality-checker` | Validates outputs before acting (quality gate) |
| `senior-developer` | General-purpose code and logic tasks |

**Excluded from serve mode:**
- `frontend-developer` — UI/DOM context not available headlessly
- `accessibility-auditor` — requires rendered UI
- `performance-benchmarker` — needs direct system access
- `project-manager` — human-facing planning, not useful for automated jobs
- `ai-engineer` — overlap with senior-developer in serve context

**New skill needed: `workflow-runner`**

None of the 12 builtins is aware of the kx serve context (job IDs, channels,
webhook events, project names). A `workflow-runner` skill would:
- Understand that it's running as a background job
- Know the channel and project context
- Format outputs for programmatic consumption (JSON-friendly)
- Know when to call `reality-checker` as a gate before reporting done

---

## Implementation Roadmap

### Phase 1: Skills in Serve Mode (unblocks everything)

| Step | File | Change |
|---|---|---|
| Add `skills` field to `RunBody` and `JobRequest` | `routes.rs`, `jobs.rs` | Small |
| Load skills from volume in `run_agent` | `serve/mod.rs` | Medium |
| Build serve system prompt with skill metadata | new `serve/skills.rs` | Medium |
| Add `deploy/skills/` directory with 7 skill files | `deploy/skills/` | Small |
| Update `docker-compose.yml` with skills volume | `docker-compose.yml` | Small |

### Phase 2: Workflow System

| Step | File | Change |
|---|---|---|
| Add `workflow` field to `RunBody` | `routes.rs` | Small |
| Workflow TOML schema + loader | new `serve/workflow.rs` | Medium |
| Merge workflow into JobRequest in `handle_run` | `routes.rs` | Small |
| Add `deploy/workflows/` with 4 starter workflows | `deploy/workflows/` | Small |
| Update `docker-compose.yml` with workflows volume | `docker-compose.yml` | Small |

### Phase 3: Operational Improvements

| Step | File | Effort | Priority |
|---|---|---|---|
| `/health` with job counts (queued/running/done/failed) | `routes.rs` | Small | High |
| `workflow-runner` skill (serve-aware) | `deploy/skills/workflow-runner/` | Medium | High |
| Webhook HMAC verification (per-source secrets) | `routes.rs`, `jobs.rs` | Medium | Medium |
| Job persistence (SQLite in data volume) | `jobs.rs`, `serve/mod.rs` | Large | Medium |
| `trigger` keyword parsing in skill parser | `skills/parser.rs` | Small | Low |
| `toolbox` parsing and registration | `skills/parser.rs`, `types.rs` | Large | Low |

### Phase 4: GitHub Actions (Maintainer, Not User)

For publishing the official Docker image to GHCR on release tags.
Only needed if kernex-agent is published publicly as a Docker image.
Not required for private VPS deployment — build locally and deploy.

---

## Open Questions / Decisions

1. **Skill loading scope in serve**: Load all installed skills always, or only
   those listed in the workflow/request? Loading all is simpler but increases
   token usage per job.

2. **Workflow storage location**: `deploy/workflows/` (version-controlled with the
   deploy config) vs `~/.kx/workflows/` (user-managed, not in repo). Both via
   Docker volume. Recommend version-controlled in `deploy/`.

3. **`trigger` field activation**: Auto-activate skills when message contains
   trigger keywords? Or require explicit skill listing per job? Explicit is safer
   and more predictable for automated workflows.

4. **Job persistence format**: SQLite (already used by kx for memory) is the
   natural fit. Schema: jobs table with id, status, output, error, timestamps,
   job metadata. Survives container restarts.

5. **Webhook HMAC secrets**: Per-event secrets (`/webhook/deploy` has its own
   `WEBHOOK_DEPLOY_SECRET`) or a single global webhook secret? Per-event is more
   secure and matches GitHub/Slack patterns.
