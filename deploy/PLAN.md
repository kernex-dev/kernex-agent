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
| Parser bug (TOML vs YAML) | CRITICAL | Skills silently fail to load in CLI and serve |
| Skills loading in serve mode | NOT STARTED | `run_agent` uses hardcoded system prompt |
| Two-mode skill architecture | NOT STARTED | Task skills vs persona/evaluation skills |
| Workflow file system | NOT STARTED | No named workflow support |
| `workflow-runner` skill | NOT STARTED | Serve-context orchestrator and validation gate |
| Validation gate (anti-hallucination) | NOT STARTED | No output validation before job completion |
| Webhook HMAC verification | NOT STARTED | All webhooks share the same bearer token |
| Job persistence | NOT STARTED | In-memory only, lost on restart |
| `/health` job stats | NOT STARTED | Returns static `{status: ok}` only |

---

## Critical Bug: Parser Format Mismatch

### The Problem

`src/skills/parser.rs` expects YAML-style key-value pairs:
```yaml
name: skill-name
description: What this skill does
```

All 12 builtins use TOML-style assignment:
```toml
name = "skill-name"
description = "What this skill does"
```

`extract_key_value()` at `parser.rs:48` looks for `:` as separator. It never finds `:` in
`name = "..."`, so `name` stays `None`, `parse_skill_md()` returns `MissingField("name")`,
and `load_skills()` silently skips every builtin with a warning. Skills are installed by
`kx init` (which bypasses parsing entirely) but **never injected into the system prompt**.

### The Fix

Extend `parse_frontmatter()` to handle both `key: value` and `key = "value"` scalar forms.
One targeted change in `parser.rs`. All existing builtins start working immediately. No
skill files need to change.

New skills (`deploy/skills/`, `workflow-runner`) will use the official YAML-style format
aligned with the Anthropic spec. Both formats will be supported indefinitely for
forward-compatibility.

**This fix must land before any other skills work. Everything below depends on it.**

---

## Skill Architecture: Two Modes

Skills in kx operate in one of two distinct modes. Mixing them in the same job degrades
both quality and token efficiency.

### Mode 1: Task Execution

The agent asks: **"What should I produce?"**

The skill grants the agent a role, capabilities, and procedures. Output is an artifact:
code, a report, a configuration file, a structured analysis.

All 12 existing builtins are task skills. Examples:
- `devops-automator` produces pipelines, Dockerfiles, infra config
- `security-engineer` produces audit reports and vulnerability findings
- `senior-developer` produces code changes and reviews

### Mode 2: Persona Evaluation

The agent asks: **"How would this person respond?"**

The skill installs a behavioral archetype with a specific perspective, cognitive biases,
and decision framework. The agent does not produce or improve content. It evaluates
content as that persona would and returns structured feedback.

This is the architectural pattern behind agentic persona systems used in market research,
UX testing, and pre-launch content review. The agent simulates an audience member, not
an author.

### Why the Separation Matters

| Concern | Impact of mixing modes |
|---|---|
| Token consumption | Task context + persona context = 2x overhead for half the quality |
| Accuracy | A persona skill told to "also fix it" loses perspective anchoring |
| Hallucination risk | Open-ended generation in an evaluation context produces invented data |
| Reproducibility | Mixed-mode jobs produce different outputs on identical inputs |

**The `workflow-runner` skill is the enforcer.** It knows which mode is active and
prevents cross-contamination. A task workflow cannot accidentally call a persona skill
in generation mode, and an evaluation workflow cannot slip into content production.

### Persona Skill Format

Persona skills use the same SKILL.md format. The difference is in the body content.

```yaml
---
name: enterprise-buyer-reviewer
description: Evaluates proposals, docs, or product copy from the perspective of an
  enterprise procurement buyer focused on vendor risk, compliance, and TCO. Returns
  structured feedback only -- does not rewrite or improve content.
---

# Enterprise Buyer Reviewer

You are simulating how a senior enterprise buyer evaluates the content they receive.
You do NOT rewrite, improve, or generate content. You read it as a buyer would.

## Profile

- 12+ years procurement and vendor management at Fortune 500 companies
- Primary concern: reducing vendor risk, not adopting new technology
- Decision blockers: unclear SLAs, missing compliance documentation, pricing ambiguity
- Trust signals: audit certifications, named references, clear escalation paths

## Evaluation Output (always return this schema)

```json
{
  "first_impression": "One sentence: what did you understand this to be?",
  "trust_signals": ["..."],
  "risk_flags": ["..."],
  "clarity_gaps": ["..."],
  "decision": "advance | pause | reject",
  "decision_rationale": "..."
}
```

## Behavioral Constraints

- You never suggest edits or improvements
- Your feedback is specific, not generic
- If the content is outside your role, say so explicitly
- You do not validate or praise -- you evaluate
```

### Serve Mode Persona Skills (Starter Set)

| Skill | Archetype | Use Case |
|---|---|---|
| `enterprise-buyer-reviewer` | Enterprise procurement buyer | Pre-launch messaging review |
| `developer-dx-reviewer` | Developer evaluating API/SDK quality | API design and docs review |
| `security-skeptic-reviewer` | Security-first engineer reviewing proposals | Pre-publish security posture review |
| `non-technical-stakeholder` | Business owner reading a technical document | Executive summary and proposal review |

---

## Token Efficiency Strategy

Token consumption is the primary operational cost for automated workflows. Every
architectural decision should be evaluated against token impact.

### Skill Loading Policy: Explicit Only

Load no skills unless explicitly requested. Never load all installed skills for every job.

Priority chain for skill resolution:
```
request.skills > workflow.skills > none
```

A job with no `skills` field runs with the base system prompt only. This is correct for
simple tasks that do not need specialist context.

### Level 1 Discovery: Metadata Only

When skills are loaded, only inject `name` and `description` into the system prompt
initially (~100 tokens per skill). The full skill body (Level 2) is made available on
request -- the agent reads it via the bash/file tool when it determines the skill is
relevant.

This means: 5 skills injected at Level 1 costs ~500 tokens. The same 5 skills injected
at Level 2 (full body) costs ~5000-15000 tokens. For most jobs, Level 1 is sufficient.

### Skill Description Quality

The description field is the most token-efficient part of a skill. A well-written
description prevents the agent from loading the full skill body unnecessarily.

Rules for description quality:
- Max 200 characters for the summary line
- State explicitly what the skill does AND when NOT to use it
- For persona skills: state that it evaluates, does not generate
- No filler phrases ("powerful", "comprehensive", "advanced")

### Workflow Skill Budgets

Each workflow TOML should specify a `max_skills` count. This acts as a hard ceiling
on how many skills can be active simultaneously, preventing token overrun from
misconfigured workflows.

```toml
[workflow]
name        = "deploy-verify"
max_skills  = 3
skills      = ["devops-automator", "security-engineer", "reality-checker"]
```

---

## Quality and Anti-Hallucination Architecture

The reliability architecture has three layers. Each layer is independent and composable.

### Layer 1: Bounded Context (Structural)

Serve mode is inherently bounded:
- `no_memory: true` on every job -- no cross-job contamination
- Skills provide vertical context, not open retrieval
- No internet access unless a skill explicitly grants it (none of the planned skills do)
- Persona skills add a second constraint: evaluation mode, not generation mode

This means the agent reasons from the skill content and the job message only.
"Disciplined, bounded reasoning rather than noise."

### Layer 2: Structured Output (Schematic)

Unstructured prose output is the primary source of drift and invented detail.
Structured output forces the agent to conform to a schema, making validation
mechanical rather than interpretive.

For task jobs: the `workflow-runner` skill instructs the agent to return a structured
summary alongside any artifacts produced.

For evaluation jobs: persona skills define a mandatory JSON schema (see example above).
The agent cannot return narrative feedback -- it must populate the schema fields.

The job response should carry a `mode` field so callers know what schema to expect:
```json
{
  "job_id": "...",
  "status": "done",
  "mode": "evaluate",
  "output": { ... }   // structured, schema-validated
}
```

### Layer 3: Validation Gate (Active)

The `reality-checker` builtin acts as a validation gate before a job is marked `done`.

In any workflow that produces artifacts or evaluation outputs, `reality-checker` runs
as the final step. It verifies:
- Claims in the output are grounded in the provided context
- No invented references, URLs, version numbers, or statistics
- The output format matches what was requested
- For evaluation jobs: all schema fields are populated with specific, not generic, content

If `reality-checker` flags an issue, the job status is set to `flagged` (new status),
not `done`. The caller receives the output with a `validation_warnings` array.

This requires adding `flagged` to the `JobStatus` enum in `src/serve/jobs.rs`.

---

## Architecture: Serve Mode Skills + Workflows

### 1. Parser Fix (Pre-Phase 1)

File: `src/skills/parser.rs` -- `parse_frontmatter()`

Add handling for `key = "value"` (strip quotes, treat as equivalent to `key: value`).
New skills use YAML-style. Both forms supported permanently.

### 2. Skills Loading in `run_agent`

File: `src/serve/mod.rs`

```rust
// Current (hardcoded):
let system_prompt = "You are a helpful AI assistant running in headless server mode.";

// Target:
let skills = load_serve_skills(&data_dir, req.skills.as_deref()).await?;
let system_prompt = build_serve_system_prompt(&skills, req.mode.as_deref());
```

`build_serve_system_prompt()` injects Level 1 metadata only. The agent reads full
skill bodies via tool calls when needed.

### 3. `RunBody` Changes

File: `src/serve/routes.rs`

```rust
pub struct RunBody {
    pub message: String,
    pub workflow: Option<String>,         // NEW: named workflow to load
    pub skills:   Option<Vec<String>>,    // NEW: explicit skill list
    pub mode:     Option<String>,         // NEW: "task" | "evaluate" (default: "task")
    pub provider: Option<String>,
    pub model:    Option<String>,
    pub project:  Option<String>,
    pub channel:  Option<String>,
    pub max_turns: Option<usize>,
}
```

### 4. Workflow File System

Named, reusable job configurations stored in `deploy/workflows/` (version-controlled).

Schema (`workflows/<name>.toml`):
```toml
[workflow]
name        = "deploy-verify"
description = "Run after every production deployment"
mode        = "task"
skills      = ["devops-automator", "security-engineer", "reality-checker"]
max_skills  = 3
project     = "production"
channel     = "deploy"
max_turns   = 15
```

Priority chain:
```
request.skills > workflow.skills > none
request.mode   > workflow.mode   > "task"
request.project > workflow.project > serve default
request.max_turns > workflow.max_turns > serve default
```

### 5. `workflow-runner` Skill

This is the only skill that understands the kx serve context. It:
- Knows the job ID, channel, project, and mode
- Enforces mode separation (task vs evaluate)
- Instructs the agent to use structured output
- Invokes `reality-checker` as the final gate before reporting done
- Formats all output for programmatic consumption

Without this skill, modes bleed into each other and output is unpredictable.

### 6. `deploy/skills/` Strategy

Do not copy builtin skill files into `deploy/skills/`. Mount `builtins/` directly.

In `docker-compose.yml`:
```yaml
volumes:
  - ./skills:/home/kx/.kx/skills/serve:ro     # serve-specific persona skills
  - ../builtins:/home/kx/.kx/skills/builtins:ro  # task skills (no duplication)
```

`deploy/skills/` contains only serve-specific skills that do not belong in builtins:
persona skills, `workflow-runner`, and any domain-specific task skills.

---

## Implementation Roadmap

### Pre-Phase: Parser Fix (unblocks all skills work)

| Step | File | Change |
|---|---|---|
| Handle `key = "value"` in `parse_frontmatter` | `skills/parser.rs` | Small |
| Update parser tests for both formats | `skills/parser.rs` | Small |
| Verify all 12 builtins parse cleanly | manual test | Verify |

### Phase 1: Skills Loading in Serve Mode

| Step | File | Change |
|---|---|---|
| Add `skills`, `mode` to `RunBody` and `JobRequest` | `routes.rs`, `jobs.rs` | Small |
| New `serve/skills.rs`: load named skills from data dir | `serve/skills.rs` | Medium |
| Build serve system prompt (Level 1 metadata only) | `serve/skills.rs` | Medium |
| Update `run_agent` to use loaded skills + mode | `serve/mod.rs` | Small |
| Update `docker-compose.yml` with skills volume mounts | `docker-compose.yml` | Small |

### Phase 2: Workflow System + Mode Enforcement

| Step | File | Change |
|---|---|---|
| New `serve/workflow.rs`: TOML schema + loader | `serve/workflow.rs` | Medium |
| Add `workflow` field to `RunBody`, merge in `handle_run` | `routes.rs` | Small |
| Add `flagged` to `JobStatus` enum | `jobs.rs` | Small |
| Write `workflow-runner` skill | `deploy/skills/workflow-runner/SKILL.md` | Medium |
| Write 4 persona/evaluation skills | `deploy/skills/reviewers/` | Medium |
| Create `deploy/workflows/` with 4 starter workflows | `deploy/workflows/` | Small |
| Update `docker-compose.yml` with workflows volume | `docker-compose.yml` | Small |

### Phase 3: Operational Improvements

| Step | File | Effort | Priority |
|---|---|---|---|
| `/health` with job counts (queued/running/done/flagged/failed) | `routes.rs` | Small | High |
| Webhook HMAC verification (per-source secrets) | `routes.rs`, `jobs.rs` | Medium | Medium |
| Job persistence (SQLite in data volume) | `jobs.rs`, `serve/mod.rs` | Large | Medium |
| `trigger` keyword parsing in skill parser | `skills/parser.rs` | Small | Low |
| `toolbox` parsing and registration | `skills/parser.rs`, `types.rs` | Large | Low |

### Phase 4: Maintainer Only

GitHub Actions release pipeline for GHCR image publishing on release tags.
Not required for private VPS deployment.

---

## Architectural Decisions (Closed)

| # | Question | Decision | Reason |
|---|---|---|---|
| 1 | Load all skills or listed only? | Listed only | Cheaper, predictable, no token overrun |
| 2 | Workflow storage location? | `deploy/workflows/` (version-controlled) | Reproducible, auditable |
| 3 | `trigger` activation? | Explicit listing only | Safer for automated workflows |
| 4 | Job persistence format? | SQLite, Phase 3 | Not needed to ship Phase 1-2 |
| 5 | Webhook HMAC secrets? | Per-event secrets | Matches GitHub/Slack pattern |
| 6 | `deploy/skills/` vs symlink builtins? | Mount builtins directly | No drift, no duplication |
| 7 | Skill loading levels? | Level 1 (metadata) by default, Level 2 on demand | Token efficiency |
| 8 | Structured output for eval jobs? | Mandatory JSON schema in persona skill bodies | Anti-hallucination |
| 9 | Validation gate? | `reality-checker` as final step in all workflows | Grounded outputs |
| 10 | Mixed task/persona in one job? | Not allowed -- mode is set per job/workflow | Quality separation |

---

## Skills Reference

### Task Skills (7 of 12 builtins, serve-appropriate)

| Skill | Role |
|---|---|
| `agents-orchestrator` | Multi-step, multi-skill workflow coordination |
| `devops-automator` | Deployment checks, CI/CD, infra management |
| `security-engineer` | Security audits, vulnerability scanning |
| `backend-architect` | API design, service architecture reviews |
| `api-tester` | Endpoint testing, integration verification |
| `reality-checker` | Validation gate -- runs last in every workflow |
| `senior-developer` | General code and logic tasks |

### Evaluation Skills (serve-specific, `deploy/skills/reviewers/`)

| Skill | Archetype | When to use |
|---|---|---|
| `enterprise-buyer-reviewer` | Enterprise procurement buyer | Messaging and proposal review |
| `developer-dx-reviewer` | Developer evaluating API or SDK | API design and documentation review |
| `security-skeptic-reviewer` | Security-first engineer | Pre-publish security posture review |
| `non-technical-stakeholder` | Business owner reading technical content | Executive summary and proposal review |

### Orchestration Skills (serve-specific, `deploy/skills/`)

| Skill | Role |
|---|---|
| `workflow-runner` | Serve context orchestrator: mode enforcement, output structure, validation gate |

---

## Research Findings: Official Anthropic Skills Format

### Minimal Required Schema

```yaml
---
name: skill-name          # max 64 chars, lowercase + hyphens only
description: ...          # max 1024 chars, what it does AND when to use it
---

# Skill content in markdown
```

### Discovery Model (3 Levels)

- Level 1 (always): `name` + `description` injected into system prompt (~100 tokens per skill)
- Level 2 (on trigger): full SKILL.md body read by agent via bash/file tool
- Level 3 (as needed): bundled files (FORMS.md, REFERENCE.md, scripts/) read on demand

### kx Extended Format vs Anthropic Spec

kx builtins use a superset with TOML-style frontmatter. These are kx extensions:

| Field | Parsed? | Used? | Plan |
|---|---|---|---|
| `name` | YES (after parser fix) | YES | Fix parser |
| `description` | YES (after parser fix) | YES | Fix parser |
| `permissions` | YES (list format) | YES | Keep |
| `version` | NO (silently ignored) | NO | Leave unparsed |
| `trigger` | NO (silently ignored) | NO | Low priority, Phase 3 |
| `[toolbox.*]` | NO (silently ignored) | NO | Low priority, Phase 3 |

New skills (persona, workflow-runner) use YAML-style format only. Both TOML and
YAML-style frontmatter are supported after the parser fix.
