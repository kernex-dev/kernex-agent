# kx serve — VPS Implementation Plan

> Single source of truth for all planned and completed work.
> Last updated: 2026-04-03

---

## Status Overview

| Area | Status | Notes |
|---|---|---|
| `kx serve` core (HTTP daemon) | DONE | Axum 0.8, Bearer auth, job queue |
| Docker deployment files | DONE | Dockerfile, docker-compose, Caddyfile |
| Security hardening | DONE | Non-root, read-only FS, TLS 1.3, rate limiting |
| Parser bug (TOML vs YAML) | DONE | Both formats handled; `metadata.domain` extracted |
| `skill-factory` builtin | DONE | 13th builtin, YAML format, registered in builtins.rs |
| Skills loading in serve mode | DONE | `src/serve/skills.rs`, Level 1 prompt, two-mode base |
| Two-mode skill architecture | DONE | `task` / `evaluate`+`review` modes in `build_serve_system_prompt` |
| 7 task skills in `deploy/skills/` | DONE | reality-checker, senior-developer, backend-architect, agents-orchestrator, devops-automator, security-engineer, api-tester |
| 4 reviewer persona skills | DONE | enterprise-buyer, developer-dx, security-skeptic, non-technical-stakeholder |
| `workflow-runner` skill | DONE | `deploy/skills/workflow-runner/SKILL.md` |
| evals/evals.json | DONE | 29 test cases across all 13 deploy skills |
| Skills volume mount in docker-compose | DONE | `deploy/skills` mounted at `/home/kx/.kx/skills` |
| Builtin description rewrites | DONE | 13 builtins, 200-char formula, no em-dashes |
| Domain skills (uxui, geo) | DONE | 4 skills: uxui-evaluator, interface-auditor, geo-auditor, geo-schema-generator |
| Workflow file system | DONE | src/serve/workflow.rs, 4 starter TOML workflows, docker volume mount |
| Webhook HMAC verification | DONE | Per-event secret via `KERNEX_WEBHOOK_SECRET_{EVENT}`, `X-Hub-Signature-256` |
| Job persistence | DONE | Write-through SQLite (`jobs.db`), crash recovery on startup |
| `/health` job stats | DONE | Returns queued/running/done/flagged/failed/total counts |
| `kx skills lint` subcommand | DONE | Content validation: required fields, sections, anti-patterns |
| Validation gate (Layer 3) | DONE | `reality-checker` auto-runs as final step in all workflows; `Flagged` status on NEEDS WORK/BLOCKED verdict |

---

## Pre-Phase: Standards and Artifacts

### Skill Format Standard (YAML-style only for new skills)

The official Agent Skills spec (`agentskills.io`, adopted by Anthropic and 26+ platforms)
uses YAML-style frontmatter. All new skills — including everything in `deploy/skills/` —
use this format:

```yaml
---
name: skill-name
description: What it does and when to trigger. Max 200 chars.
metadata:
  author: kernex-dev
  version: "1.0"
  domain: ops
---

# Skill Name

2-3 sentence overview.

## Workflow
## Output Format
## Examples
## Edge Cases
```

The `metadata.domain` field uses a fixed taxonomy: `task`, `review`, `ops`, `orchestration`.
In serve mode this enables routing: evaluation workflows load `domain: review` skills only.

Existing builtins use TOML-style (`name = "..."`) which the parser cannot read. They are
not changing format — the parser will be extended to handle both. New skills use YAML only.

### Skill Directory Structure

```
skill-name/
├── SKILL.md          # Required
├── scripts/          # Optional: deterministic Python/Bash operations
├── references/       # Optional: domain guides, templates, API refs
├── assets/           # Optional: static files
└── evals/
    └── evals.json    # Required for deploy/ skills: test cases
```

Scripts are token-efficient: the code never enters context, only execution output does.
Any operation that should produce identical output every time belongs in a script.

### evals/evals.json Format

Every skill in `deploy/skills/` ships with test cases:

```json
{
  "skill_name": "skill-name",
  "evals": [
    {
      "id": 1,
      "prompt": "realistic messy user prompt with context",
      "expected_output": "description of expected result",
      "files": []
    }
  ]
}
```

### Description Formula (200-char budget)

```
First 80 chars: what it does (agent reads this first)
Next 60 chars:  when to use it (trigger contexts)
Last 60 chars:  key phrases or explicit NOT-trigger boundaries
```

Pattern: `[Action verb] + [specific output] + [trigger contexts/phrases]`

All 12 existing builtin descriptions need to be rewritten to this formula after the parser fix.
Current descriptions are 60-90 chars and too abstract to trigger reliably.

### Autonomy Checklist (required before shipping any skill)

```
[ ] Every step has a clear, unambiguous action
[ ] No step requires user input to proceed (or has a default fallback)
[ ] Output format is fully specified with a template
[ ] Edge cases have explicit handling — not "ask the user"
[ ] Completion criteria are defined
[ ] Deterministic operations are in scripts/, not instructions
[ ] Error states have recovery paths
[ ] Tested with 3+ realistic prompts without human input
[ ] Description triggers reliably on natural-language requests
[ ] evals/evals.json exists with 2+ test cases
```

### skills_research/ Artifact Disposition

`skills_research/` is a scratch directory with finished artifacts. Move them:

| File | Move to |
|---|---|
| `SKILL.md` (skill-factory) | `builtins/skill-factory/SKILL.md` |
| `validate_skill.py` | `deploy/skills/validate_skill.py` |
| `autonomy-guide.md` | `deploy/skills/references/autonomy-guide.md` |
| `description-patterns.md` | `deploy/skills/references/description-patterns.md` |
| `templates.md` | `deploy/skills/references/templates.md` |
| `skills-playbook.md` | Keep as reference, do not deploy |

---

## Critical Bug: Parser Format Mismatch

### The Problem

`src/skills/parser.rs:48` — `extract_key_value()` looks for `:` as separator:

```rust
fn extract_key_value(line: &str) -> Option<(&str, &str)> {
    let colon_pos = line.find(':')?;
```

All 12 builtins use TOML-style assignment:
```toml
name = "devops-automator"
description = "DevOps and infrastructure..."
```

`line.find(':')` returns `None` on `name = "..."`. `name` stays `None`.
`parse_skill_md()` returns `MissingField("name")`. `load_skills()` emits a warning
and skips every builtin silently. **Skills are installed but never loaded into any prompt.**

Running `skills_research/validate_skill.py` on any existing builtin confirms this:
every one fails with "Missing required 'name' field."

The install path (`builtins.rs:86`) bypasses parsing entirely — it hardcodes the name
from the struct literal. This is why `kx init` succeeds but skills never activate.

### The Fix

Extend `parse_frontmatter()` to handle both scalar forms:

```rust
// Handle: key: value  (YAML-style)
// Handle: key = "value"  (TOML-style, existing builtins)
fn extract_key_value(line: &str) -> Option<(&str, &str)> {
    if let Some(pos) = line.find(':') {
        // YAML: key: value
        let key = line[..pos].trim();
        if !key.is_empty() && !key.contains(' ') && !key.contains('=') {
            return Some((key, line[pos + 1..].trim()));
        }
    }
    if let Some(pos) = line.find('=') {
        // TOML: key = "value"
        let key = line[..pos].trim();
        if !key.is_empty() && !key.contains(' ') {
            let val = line[pos + 1..].trim().trim_matches('"').trim_matches('\'');
            return Some((key, val));
        }
    }
    None
}
```

Also add `metadata` as a parseable block key (for `domain` extraction). Add
`metadata.domain` to `SkillManifest` as `pub domain: Option<String>`.

Update parser tests to cover both formats. Verify all 12 builtins parse cleanly.

**This fix must land before any other skills work.**

---

## Skill Architecture: Two Modes

Skills operate in one of two modes. Mixing them in the same job degrades quality,
wastes tokens, and breaks validation.

### Mode 1: Task Execution

Agent asks: **"What should I produce?"**

The skill grants a role, capabilities, and procedures. Output is an artifact:
code, report, config file, analysis. All 12 existing builtins are task skills.

### Mode 2: Persona Evaluation

Agent asks: **"How would this person respond?"**

The skill installs a behavioral archetype. The agent evaluates content as that persona
would and returns structured feedback. It does not produce or improve content.

This is the architectural pattern behind agentic persona systems: simulating audience
response before content goes live, stress-testing proposals, pre-launch messaging review.
The agent simulates an audience member, not an author.

### Why the Separation Matters

| Concern | Impact of mixing modes |
|---|---|
| Token consumption | Task context + persona context = 2x overhead for half the quality |
| Accuracy | A persona skill told to "also fix it" loses perspective anchoring |
| Hallucination risk | Open-ended generation in evaluation context produces invented data |
| Reproducibility | Mixed-mode jobs produce different outputs on identical inputs |

### Persona Skill Format

Same SKILL.md structure. The body content makes the mode explicit:

```yaml
---
name: enterprise-buyer-reviewer
description: Evaluates proposals and messaging from the perspective of an enterprise
  procurement buyer focused on vendor risk and compliance. Returns structured JSON
  feedback only -- does not rewrite or generate content.
metadata:
  author: kernex-dev
  version: "1.0"
  domain: review
---

# Enterprise Buyer Reviewer

Simulate how a senior enterprise buyer evaluates the content received.
Do NOT rewrite, improve, or generate content. Evaluate as a buyer would.

## Profile

- 12+ years procurement and vendor management at Fortune 500 companies
- Primary concern: reducing vendor risk, not adopting new technology
- Decision blockers: unclear SLAs, missing compliance docs, pricing ambiguity
- Trust signals: audit certifications, named references, clear escalation paths

## Output Schema (always return this exact JSON)

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

- Never suggest edits or improvements
- Feedback must be specific, not generic ("the pricing section", not "some areas")
- If content is outside your role, say so explicitly
- Do not validate or praise — evaluate
```

---

## Token Efficiency Architecture

Token cost is the primary operational cost of automated workflows. Every design
decision should be evaluated against token impact.

### Skill Loading: Explicit Only

Load no skills unless explicitly requested. Never load all installed skills per job.

Resolution chain:
```
request.skills > workflow.skills > none
```

A job with no `skills` field runs with the base system prompt only.

### Level 1/2/3 Discovery

| Level | Content | Token cost | When |
|---|---|---|---|
| 1 | `name` + `description` only | 30-100 per skill | Always, for listed skills |
| 2 | Full SKILL.md body | 2,000-5,000 per skill | Agent reads on demand via tool |
| 3 | `scripts/` output, `references/` | Variable, output only | Agent invokes scripts |

Default: inject Level 1 metadata only. The agent reads the full body (Level 2) via
a file/bash tool when it determines the skill is needed. Script code (Level 3) never
enters context — only the execution output does.

Example: 5 skills at Level 1 = ~250-500 tokens. Same 5 at Level 2 = ~10,000-25,000 tokens.

### Workflow Skill Budgets

Each workflow TOML defines a `max_skills` ceiling:

```toml
[workflow]
name       = "deploy-verify"
max_skills = 3
skills     = ["devops-automator", "security-engineer", "reality-checker"]
```

This prevents token overrun from misconfigured or over-specified workflows.

### Description Quality as Token Optimization

A precise description prevents unnecessary Level 2 loads. If the agent can determine
from the description that a skill is not relevant, it never reads the body.

Rules:
- Max 200 characters, use the full budget
- State explicitly what the skill does AND when NOT to use it
- For persona skills: state that it evaluates, does not generate
- No filler: "powerful", "comprehensive", "advanced" waste characters

---

## Quality and Anti-Hallucination Architecture

Three independent layers. Each is composable with the others.

### Layer 1: Bounded Context (Structural)

Serve mode is inherently bounded:
- `no_memory: true` per job — no cross-job contamination
- Skills provide vertical context, not open retrieval
- No internet access unless a skill explicitly enables it (none of the planned skills do)
- Persona skills add a second constraint: evaluation mode, not generation mode

The agent reasons from skill content and the job message only.

### Layer 2: Structured Output (Schematic)

Unstructured prose is the primary source of drift and invented detail. Structured output
forces conformance to a schema, making validation mechanical not interpretive.

For task jobs: `workflow-runner` instructs the agent to return a structured summary
alongside artifacts.

For evaluation jobs: persona skills define a mandatory JSON output schema. The agent
cannot return narrative feedback — it must populate schema fields.

Job response carries a `mode` field so callers know what schema to expect:
```json
{
  "job_id": "...",
  "status": "done",
  "mode": "evaluate",
  "output": { ... }
}
```

### Layer 3: Validation Gate (Active)

`reality-checker` runs as the final step in every workflow. It verifies:
- Claims are grounded in the provided context
- No invented references, URLs, version numbers, or statistics
- Output format matches what was requested
- For evaluation jobs: all schema fields are populated with specific content

If `reality-checker` flags an issue, the job status is set to `flagged` (new status),
not `done`. The caller receives output with a `validation_warnings` array.

This requires adding `Flagged` to `JobStatus` in `src/serve/jobs.rs`.

### Autonomy as Anti-Hallucination

Skills written to the autonomy standard (see checklist above) prevent hallucination at
the skill level. Specific instructions beat vague ones: the more concrete the expected
output format, the less room the model has to invent. Scripts handle deterministic
operations — script output is ground truth, not model inference.

---

## Implementation Roadmap

### Pre-Phase 0: Move Artifacts

| Step | Action |
|---|---|
| Copy `skills_research/SKILL.md` | to `builtins/skill-factory/SKILL.md` |
| Copy `skills_research/validate_skill.py` | to `deploy/skills/validate_skill.py` |
| Copy `skills_research/*.md` (3 guides) | to `deploy/skills/references/` |
| Confirm `skills_research/` is in `.gitignore` or remove it | cleanup |

### Pre-Phase 1: Parser Fix

| Step | File | Change |
|---|---|---|
| Extend `extract_key_value()` for `key = "value"` | `skills/parser.rs` | Small |
| Add `domain: Option<String>` to `SkillManifest` | `skills/types.rs` | Small |
| Parse `metadata:` block, extract `domain` | `skills/parser.rs` | Small |
| Update parser tests for both formats + domain | `skills/parser.rs` | Small |
| Run all 12 builtins through `validate_skill.py` | manual | Verify |
| Rewrite all 12 builtin descriptions to 200-char formula | `builtins/*/SKILL.md` | Medium |

### Phase 1: Skills Loading in Serve Mode

| Step | File | Change |
|---|---|---|
| Add `skills`, `mode` fields to `RunBody` and `JobRequest` | `routes.rs`, `jobs.rs` | Small |
| New `serve/skills.rs`: load named skills, build Level 1 prompt | new file | Medium |
| Update `run_agent` to use loaded skills and mode | `serve/mod.rs` | Small |
| Add `Flagged` variant to `JobStatus` | `jobs.rs` | Small |
| Create `deploy/skills/` directory structure | `deploy/skills/` | Small |
| Write 7 task skill files (from builtins, YAML format) | `deploy/skills/` | Medium |
| Add skills volume mounts to `docker-compose.yml` | `docker-compose.yml` | Small |

### Phase 2: Workflow System + Mode Enforcement

| Step | File | Change |
|---|---|---|
| New `serve/workflow.rs`: TOML schema + loader | new file | Medium |
| Add `workflow` field to `RunBody`, merge in `handle_run` | `routes.rs` | Small |
| Write `workflow-runner` skill | `deploy/skills/workflow-runner/SKILL.md` | Medium |
| Write 4 persona/evaluation skills | `deploy/skills/reviewers/` | Medium |
| Create `deploy/workflows/` with 4 starter workflows | `deploy/workflows/` | Small |
| Add workflows volume mount to `docker-compose.yml` | `docker-compose.yml` | Small |

### Phase 3: Operational Improvements

| Step | File | Effort | Priority |
|---|---|---|---|
| `/health` with job counts (queued/running/done/flagged/failed) | `routes.rs` | Small | High |
| Webhook HMAC verification (per-source secrets) | `routes.rs`, `jobs.rs` | Medium | Medium |
| Job persistence (SQLite in data volume) | `jobs.rs`, `serve/mod.rs` | Large | Medium |
| Port `validate_skill.py` to `kx skills verify` Rust command | `skills/cli_handler.rs` | Medium | Medium |
| `trigger` keyword parsing | `skills/parser.rs` | Small | Low |
| `toolbox` parsing and registration | `skills/parser.rs`, `types.rs` | Large | Low |

### Phase 4: Maintainer Only

GitHub Actions release pipeline for GHCR image publishing on release tags.
Not required for private VPS deployment.

### Phase 5: Community Skills (publishable to Skills.sh + awesome-agent-skills)

Market research on the top 20 agent skills by install count (2026) reveals:
- Meta-skills and opinionated framework-specific skills dominate. Generic "help me code" skills have low repeat usage.
- The top skills share one trait: they encode a concrete protocol, not open-ended behavior.
- Three opportunity gaps with no dominant player: UX/UI audit, design-to-code with anti-AI aesthetics, GEO optimization.

These skills are publish-ready candidates. All require evals/evals.json and a passing autonomy checklist before submission.

| Step | Skill | Source | Status |
|------|-------|--------|--------|
| Write | `uxui-evaluator` | uxuiprinciples-web (168 principles, 6 domains) | DONE |
| Write | `interface-auditor` | interfaceaudit-web (antipattern taxonomy, 1-10 severity) | DONE |
| Write | `geo-auditor` | GEOAutopilot (5-tier weighted scoring model) | DONE |
| Write | `geo-schema-generator` | GEOAutopilot (Schema.org generation engine) | DONE |
| Write evals | All 4 above | evals/evals.json | DONE |
| Publish | All 4 | Skills.sh + awesome-agent-skills repo | PENDING |

---

## Skills Reference

### Task Skills (7 of 12 builtins, serve-appropriate)

| Skill | Domain | Role |
|---|---|---|
| `agents-orchestrator` | task | Multi-step, multi-skill coordination |
| `devops-automator` | task | Deployment checks, CI/CD, infra |
| `security-engineer` | task | Security audits, vulnerability scanning |
| `backend-architect` | task | API design, service architecture |
| `api-tester` | task | Endpoint testing, integration verification |
| `reality-checker` | task | Validation gate -- last step in every workflow |
| `senior-developer` | task | General code and logic tasks |

### Domain-Specific Skills (Phase 5, community-publishable)

| Skill | Domain | Source | Differentiator |
|---|---|---|---|
| `uxui-evaluator` | review | uxuiprinciples-web | 168-principle taxonomy, principle codes, business impact metrics |
| `interface-auditor` | review | interfaceaudit-web | Antipattern detection, 1-10 severity scoring, UX smell patterns |
| `geo-auditor` | task | GEOAutopilot | 5-tier weighted GEO model, AI crawler audit, action plan |
| `geo-schema-generator` | task | GEOAutopilot | Schema.org JSON-LD generation, quality scoring, deployment-ready |

### Evaluation Skills (`deploy/skills/reviewers/`)

| Skill | Domain | Archetype |
|---|---|---|
| `enterprise-buyer-reviewer` | review | Enterprise procurement buyer |
| `developer-dx-reviewer` | review | Developer evaluating API or SDK quality |
| `security-skeptic-reviewer` | review | Security-first engineer |
| `non-technical-stakeholder` | review | Business owner reading technical content |

### Orchestration Skills (`deploy/skills/`)

| Skill | Domain | Role |
|---|---|---|
| `workflow-runner` | orchestration | Mode enforcement, structured output, validation gate |

### Meta-Skills (new builtin)

| Skill | Domain | Role |
|---|---|---|
| `skill-factory` | ops | Builds new skills following the 8-step authoring workflow |

---

## Architectural Decisions (Closed)

| # | Decision | Rationale |
|---|---|---|
| 1 | YAML-style for new skills, both formats in parser | Spec alignment + no breakage of existing builtins |
| 2 | `metadata.domain` parsed as optional field | Enables serve mode routing without breaking existing skills |
| 3 | Level 1 metadata by default, Level 2 on demand | Token efficiency: 30-100 vs 2,000-5,000 tokens per skill |
| 4 | Scripts for deterministic ops (code out of context) | Script output = ground truth, not model inference |
| 5 | Explicit skill loading only (no all-skills-always) | Predictable, no token overrun |
| 6 | Workflows version-controlled in `deploy/workflows/` | Reproducible, auditable |
| 7 | `deploy/skills/` for serve-specific skills, builtins mounted directly | No drift, no duplication |
| 8 | `evals/evals.json` as test standard for deploy/ skills | Consistent test format across skills |
| 9 | Mandatory JSON output schema in persona skill bodies | Anti-hallucination: schema forces grounded responses |
| 10 | `reality-checker` as final step in all workflows | Active validation gate |
| 11 | Mixed task/persona mode not allowed per job | Quality separation, reproducibility |
| 12 | `Flagged` status for validation failures | Caller gets output + warnings, not silent pass/fail |
| 13 | Per-event HMAC secrets for webhooks | Matches GitHub/Slack pattern, more secure |
| 14 | SQLite for job persistence, Phase 3 | Not needed to ship Phase 1-2 |
| 15 | `skill-factory` as 13th builtin | Enables users to build their own skills within kx |

---

## docker-compose Volume Strategy

```yaml
volumes:
  # Serve-specific skills (persona, workflow-runner)
  - ./skills:/home/kx/.kx/skills/serve:ro
  # Task skills from builtins (no duplication)
  - ../builtins:/home/kx/.kx/skills/builtins:ro
  # Named workflow definitions
  - ./workflows:/home/kx/.kx/workflows:ro
  # Persistent job data
  - kx_data:/home/kx/.kx
  # Claude subscription credentials
  - type: bind
    source: ${CLAUDE_CREDENTIALS_PATH}
    target: /home/kx/.claude/.credentials.json
    read_only: true
```

---

## Token Budget Reference

| Scenario | Estimated Token Cost |
|---|---|
| 5 skills at Level 1 (metadata only) | 150-500 tokens |
| 1 skill at Level 2 (full body) | 2,000-5,000 tokens |
| 5 skills at Level 2 | 10,000-25,000 tokens |
| Script execution (Level 3) | Output only, typically 100-500 tokens |
| Persona evaluation job (4 skills Level 1 + 1 Level 2) | ~700-1,200 tokens overhead |
| Task job (3 skills Level 1 + 1 triggered Level 2) | ~400-800 tokens overhead |
