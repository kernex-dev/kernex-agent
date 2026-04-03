# Skills Infrastructure Playbook
## A Complete Workflow for Building Agent Skills Across Claude.ai, Claude Code, Kernex/kx, and 26+ Platforms

**Author:** Jose Hurtado · Visual Brands LLC  
**Version:** 1.0 · April 2026  
**Scope:** Claude.ai, Claude Code, Kernex/kx, Codex, Copilot, and any Agent Skills-compatible platform

---

## 1. What Skills Actually Are

A skill is a **folder** containing, at minimum, a `SKILL.md` file. That file has YAML frontmatter (metadata) followed by Markdown instructions. Optionally, the folder includes scripts, reference docs, and assets.

Skills are not prompts. Prompts are conversation-level, one-off instructions. Skills are **persistent, portable, version-controlled expertise** that agents load on demand. Think of them as onboarding documents for AI — you write the workflow once, and every agent instance that encounters a matching task follows the same playbook without you re-explaining anything.

The Agent Skills specification (agentskills.io) is an open standard created by Anthropic and adopted by 26+ platforms: Claude Code, OpenAI Codex, Gemini CLI, GitHub Copilot, Cursor, VS Code, Windsurf, Aider, Kilo Code, OpenCode, Augment, Antigravity IDE, and Kernex/kx among others. A skill you write once works identically across all of them.

```
skill-name/
├── SKILL.md          # Required: YAML frontmatter + markdown instructions
├── scripts/          # Optional: executable code (Python, Bash, JS)
├── references/       # Optional: additional documentation
├── assets/           # Optional: templates, icons, fonts, images
└── LICENSE.txt       # Optional: license file
```

---

## 2. How Progressive Disclosure Works

Progressive disclosure is the architecture that makes skills efficient. It solves the context window problem — you can have hundreds of skills installed without consuming tokens until one is actually needed.

**Level 1 — Metadata (~30-100 tokens per skill)**  
At startup, the agent loads only the `name` and `description` from every installed skill's YAML frontmatter. This is the "shelf label" — just enough for the agent to know what the skill does and when to consider using it. All installed skills are scanned at this level simultaneously.

**Level 2 — Full SKILL.md body (~2,000-5,000 tokens)**  
When a user's request matches a skill's description, the agent reads the entire SKILL.md file into context. This contains the procedural instructions, workflows, examples, and references to deeper resources. Only the triggered skill loads at this level.

**Level 3 — Bundled resources (as needed)**  
Scripts, reference files, and assets load only when the instructions reference them during execution. A skill can contain dozens of files, but if the current task only needs one script, only that script's output enters context. Script *code* never enters context — only the execution output.

**Why this matters for autonomous operation:**  
An agent with 50 skills installed uses roughly 50 × 50 tokens = 2,500 tokens for metadata scanning. When one triggers, it adds 2-5K tokens. Resources load surgically. This means you can build a comprehensive skill library without degrading agent performance.

---

## 3. How Triggering Works

Understanding triggering is critical for building skills that activate reliably without babysitting.

**The description field is the primary trigger mechanism.** The agent reads all skill descriptions, evaluates relevance to the current task, and decides whether to load the full skill. This happens automatically — no slash command needed (though slash commands work as explicit invocation).

**Key triggering behaviors:**

- Agents only consult skills for tasks they can't easily handle on their own. A simple "read this file" won't trigger a skill even with a perfect description match, because the agent can handle it directly.
- Complex, multi-step, or specialized queries reliably trigger skills when the description matches.
- The description should sound like the way a user would naturally ask for that task, not abstract jargon.
- Skills tend to "undertrigger" — Claude is conservative about loading them. Descriptions should be slightly "pushy" — explicitly listing contexts and phrases that should activate the skill.

**Triggering across platforms:**

| Platform | Auto-trigger | Explicit invocation | Notes |
|----------|-------------|-------------------|-------|
| Claude.ai | Yes (description match) | N/A | Skills in `available_skills` list |
| Claude Code | Yes (description match) | `/skill-name` slash command | Watches `.claude/skills/` live |
| Kernex/kx | Yes (description match) | `/skill-name` | Loaded at `kx init` |
| Codex | Yes (description match) | `/skills` or `$skill-name` | |
| Copilot | Yes (description match) | `/skill-name` in chat | |

**Writing effective descriptions (max 200 characters):**

Bad: `"Handles documents"` — too vague, won't trigger reliably.

Bad: `"An advanced multi-modal document processing and extraction pipeline for enterprise-grade workflows"` — too abstract, keyword-stuffed.

Good: `"Generate UX audit reports from screenshots or Figma URLs. Use when reviewing interfaces, identifying usability issues, or creating client-facing audit deliverables."` — specific, action-oriented, includes trigger phrases a user would actually say.

---

## 4. SKILL.md Anatomy — The Universal Format

### 4.1 YAML Frontmatter (Required)

```yaml
---
name: skill-name
description: What this skill does and when to use it. Max 200 chars.
license: Apache-2.0                    # Optional
metadata:                              # Optional
  author: jose-hurtado
  version: "1.0"
  domain: design                       # Custom field for your taxonomy
compatibility:                         # Optional — only if non-standard deps
  dependencies:
    - python3
    - pip:python-pptx
---
```

**Frontmatter rules:**
- `name`: 1-64 characters, lowercase alphanumeric + hyphens only, must match the parent directory name. No consecutive hyphens, no leading/trailing hyphens.
- `description`: Max 200 characters. This is the trigger mechanism — write it with care.
- `license`: Keep short. Use license name or reference bundled file.
- `metadata`: Free-form key-value pairs. Use for author, version, domain tagging.
- `compatibility`: Only include if the skill requires specific tools/packages beyond basic agent capabilities. Most skills don't need this.

### 4.2 Markdown Body (Instructions)

The body follows the frontmatter and contains the actual expertise. Structure it for scannability — the agent will read the entire file when triggered, so every line should earn its place.

**Recommended sections:**
- **Overview** — 2-3 sentences on what the skill does and the workflow philosophy
- **Workflow steps** — Imperative instructions in logical order
- **Output format** — Templates or examples of expected output
- **Examples** — Input/output pairs showing real usage
- **Edge cases** — Common pitfalls and how to handle them
- **File references** — Pointers to scripts/ and references/ with guidance on when to load them

**Target length:** Under 500 lines. If approaching this limit, split detail into reference files and add clear pointers from SKILL.md about when to read them.

### 4.3 Supporting Directories

**`scripts/`** — Executable code for deterministic or repetitive tasks. Scripts should be self-contained, include error handling, and document dependencies. The agent runs them via bash and only sees the output — the code itself never enters context. This is token-efficient.

**`references/`** — Additional documentation loaded on demand. Use for domain-specific guides, API references, detailed templates. Keep individual files focused and under 300 lines. If longer, add a table of contents.

**`assets/`** — Static resources: templates, fonts, icons, images. Used in output generation but not read into context as text.

---

## 5. Cross-Platform Compatibility Matrix

The SKILL.md format is the universal constant. Platform differences are in *where* skills live and *how* they're invoked.

### 5.1 File Locations by Platform

| Platform | Project-level skills | Global/user skills |
|----------|--------------------|--------------------|
| Claude.ai | Uploaded via UI or API | User skills in settings |
| Claude Code | `.claude/skills/` in project root | `~/.claude/skills/` |
| Kernex/kx | `.kx/skills/` in project root | `~/.kx/skills/` |
| Codex | `.codex/skills/` or configured path | `~/.codex/config.toml` |
| Copilot | `.github/skills/` | VS Code settings |

### 5.2 Platform-Specific Considerations

**Claude.ai:**
- Skills run in a sandboxed Linux container with code execution
- Can install packages from PyPI and npm at skill load time
- File system resets between tasks — skills must be self-contained
- No subagents — test cases run inline, sequentially
- Skills uploaded via the web UI or the `/v1/skills` API endpoint

**Claude Code:**
- Filesystem-based, no upload needed
- Live change detection — edit a SKILL.md mid-session and changes apply immediately
- Supports subagents for parallel test execution
- Skills from `--add-dir` directories are loaded automatically
- Slash commands (`/skill-name`) for explicit invocation

**Kernex/kx:**
- Compatible with the Agent Skills standard (agentskills.io)
- Loaded at `kx init` with project stack detection
- SQLite-backed memory means skills can reference persistent facts
- OS-level sandboxing (Seatbelt/Landlock) constrains skill execution
- Community skills install with one command
- Provider-agnostic — same skill works with Claude, OpenAI, Ollama, etc.

### 5.3 Writing Skills That Work Everywhere

To maximize portability:

1. **Stick to the spec.** Use only fields defined at agentskills.io: `name`, `description`, `license`, `metadata`, `compatibility`.
2. **Keep scripts to Python, Bash, or JS.** These are supported by all major platforms.
3. **Use stdlib-only Python.** If you can avoid `pip install` dependencies, your skill works everywhere without compatibility fields.
4. **Reference files with relative paths.** `See [REFERENCE.md](references/REFERENCE.md)` — not absolute paths.
5. **Don't assume platform-specific features.** Don't rely on Claude.ai's container persistence or Claude Code's subagents in the core SKILL.md. Add platform-specific notes in a separate section if needed.

---

## 6. Skill Authoring Workflow — Step by Step

This is the repeatable process for creating a new skill from scratch, designed for autonomous agent operation.

### Phase 1: Define Intent

Before writing anything, answer four questions:

1. **What should this skill enable the agent to do?** Be specific. Not "help with design" but "generate a UX audit report with heuristic scores, annotated screenshots, and prioritized recommendations."

2. **When should this skill trigger?** List the phrases, contexts, and task types. "When the user uploads screenshots and asks for a review." "When they mention 'audit', 'usability review', or 'interface analysis'." "When they reference interfaceaudit.com."

3. **What is the expected output format?** A markdown report? A .docx file? A React component? An email draft? Define this precisely — it's the skill's contract.

4. **What makes this skill different from what the agent can do without it?** If the agent can handle it natively, you don't need a skill. Skills add value when they enforce specific workflows, output formats, domain knowledge, or multi-step processes that the agent wouldn't follow consistently on its own.

### Phase 2: Research and Interview

Before writing the SKILL.md, gather the domain knowledge:

- What are the edge cases and common pitfalls?
- What are the input/output examples that define success?
- What reference materials, templates, or scripts will the skill need?
- Are there existing skills that overlap? Check installed skills and community repos.

If building for a client domain (DXagency, Nestlé), pull in brand guidelines, style guides, technical constraints, and approval workflows.

### Phase 3: Draft the SKILL.md

Write the first version following this template:

```markdown
---
name: [lowercase-hyphenated-name]
description: [What it does + when to trigger. Max 200 chars. Be pushy.]
metadata:
  author: jose-hurtado
  version: "0.1"
  domain: [design|product|client|content]
---

# [Skill Name]

[2-3 sentence overview of what this skill does and the philosophy behind it.]

## Workflow

[Step-by-step imperative instructions. Number them if sequential.]

1. [First step]
2. [Second step]
3. [Third step]

## Output Format

[Template or structure of expected output. Use exact formatting.]

## Examples

**Example 1:**
Input: [realistic user prompt]
Output: [what the agent should produce]

**Example 2:**
Input: [edge case or variant]
Output: [expected handling]

## Edge Cases

- [Common pitfall and how to handle it]
- [Another edge case]

## References

- For [specific scenario], see [references/specific-guide.md]
- Run [scripts/helper.py] when [specific condition]
```

### Phase 4: Write Test Cases

Create 2-3 realistic test prompts — the kind of thing you or a client would actually type. Not clean, formal requests, but real-world messy ones with context, abbreviations, maybe a typo.

Save them as `evals/evals.json`:

```json
{
  "skill_name": "ux-audit-report",
  "evals": [
    {
      "id": 1,
      "prompt": "hey can you review these screens I uploaded? the client wants a UX report by friday, focus on checkout flow issues",
      "expected_output": "Structured UX audit report with heuristic scores and prioritized findings",
      "files": ["checkout-screen-1.png", "checkout-screen-2.png"]
    },
    {
      "id": 2,
      "prompt": "run an interface audit on this dashboard design, use the standard scoring system",
      "expected_output": "Dashboard audit with all 10 heuristic categories scored",
      "files": ["dashboard-mockup.fig"]
    }
  ]
}
```

### Phase 5: Test and Iterate

**In Claude.ai:**
- Read the SKILL.md, then follow its instructions to accomplish each test prompt yourself
- Present results inline for review
- No subagents — run sequentially
- Focus on qualitative feedback

**In Claude Code:**
- Spawn subagents for parallel execution (with-skill and baseline)
- Use the eval-viewer for structured review
- Run quantitative assertions against outputs
- Grade with the grader agent

**In Kernex/kx:**
- Run `kx init` to load the skill
- Execute test prompts through `kx`
- Check SQLite memory for fact persistence
- Verify sandbox profile doesn't block needed operations

**The iteration loop:**
1. Run test cases → collect outputs
2. Review outputs (you + agent)
3. Identify failures — generalize from specific feedback, don't overfit
4. Revise the SKILL.md — explain the *why*, not just add MUSTs
5. Look for repeated work across test cases — if every run writes the same helper script, bundle it in `scripts/`
6. Rerun → review → repeat until satisfied

### Phase 6: Optimize the Description

After the skill works well, optimize the description for triggering accuracy:

1. Generate 20 eval queries — 10 should-trigger, 10 should-not-trigger
2. Make should-trigger queries diverse: formal, casual, with context, without
3. Make should-not-trigger queries tricky near-misses, not obvious irrelevant ones
4. Test the trigger rate across 3 runs per query
5. Iterate the description based on failures

### Phase 7: Package and Distribute

**For Claude.ai:** Upload via the UI or the `/v1/skills` API endpoint.

**For Claude Code:** Place in `.claude/skills/` — it's detected live.

**For Kernex/kx:** Place in `.kx/skills/` or install via community command.

**For cross-platform distribution:** Publish to GitHub and reference in awesome-claude-skills or the SkillsMP marketplace.

**Packaging command (from skill-creator tooling):**
```bash
python -m scripts.package_skill path/to/skill-folder
```
This produces a `.skill` file ready for upload or sharing.

---

## 7. Domain-Specific Skill Templates

### 7.1 Product Development (uxuiprinciples.com, interfaceaudit.com, geoautopilot.com)

```yaml
---
name: product-feature-spec
description: Generate feature specifications for product development. Use when planning new features, writing PRDs, creating user stories, or defining acceptance criteria for uxuiprinciples, interfaceaudit, or geoautopilot.
metadata:
  author: jose-hurtado
  version: "1.0"
  domain: product
---
```

**Skill body pattern:** Include the product context (tech stack: Next.js, React, Supabase), the feature spec template, the acceptance criteria format, and references to brand/product-specific guidelines in `references/`.

**Recommended sub-skills for this domain:**
- `product-feature-spec` — PRDs and user stories
- `product-launch-checklist` — Pre-launch validation
- `product-analytics-setup` — GA4/tracking implementation guides
- `product-mdx-content` — MDX content creation for uxuiprinciples.com

### 7.2 Client Work (DXagency, Fortune 500)

```yaml
---
name: client-deliverable
description: Create client-facing deliverables with proper formatting, branding, and approval workflows. Use for reports, presentations, proposals, or any external-facing document for DXagency or client projects.
metadata:
  author: jose-hurtado
  version: "1.0"
  domain: client
---
```

**Skill body pattern:** Include the deliverable template, client brand reference loading (from `references/brands/`), approval workflow steps, and quality gates. Reference brand-specific files conditionally — load only the relevant client's guidelines.

**Recommended sub-skills:**
- `client-deliverable` — Reports, decks, proposals
- `client-onboarding-checklist` — Vendor onboarding (GA4, tools access)
- `client-brand-apply` — Apply specific client brand guidelines
- `client-geo-strategy` — GEO/AI search strategy reports (like Vitaflo)

### 7.3 Design Systems & Code Generation

```yaml
---
name: design-system-component
description: Build design system components with Figma-to-code workflow. Use when creating React/Next.js components, design tokens, icon sets, or when converting Figma designs to production code. Triggers on mentions of design systems, component libraries, or Figma exports.
metadata:
  author: jose-hurtado
  version: "1.0"
  domain: design
---
```

**Skill body pattern:** Include the component architecture (atomic design), naming conventions, token system, accessibility requirements, and the anti-AI-design quality gate. Reference the existing anti-ai-design skill principles.

**Recommended sub-skills:**
- `design-system-component` — Individual component creation
- `design-token-generator` — Color, spacing, typography token files
- `figma-to-code` — Screenshot/Figma to React conversion
- `svg-icon-set` — SVG icon creation and optimization
- `design-audit` — Interface audit with heuristic scoring

### 7.4 Content, Proposals & Service OS

```yaml
---
name: service-proposal
description: Generate client proposals, contracts, SOWs, and service documentation. Use when creating proposals, writing contracts, building playbooks, or assembling Service OS documents for Visual Brands LLC or User Centric Studio.
metadata:
  author: jose-hurtado
  version: "1.0"
  domain: content
---
```

**Skill body pattern:** Include the proposal template, pricing framework, contract clause library (in `references/`), and the Service OS structure. Reference specific playbooks and brief templates from the GitHub repo.

**Recommended sub-skills:**
- `service-proposal` — Proposals and SOWs
- `service-contract` — Contractor agreements and amendments
- `content-linkedin` — LinkedIn posts and job search content
- `content-blog` — Blog posts for product sites
- `content-case-study` — Client case study generation

---

## 8. Designing Skills for Autonomous Operation

The goal is agents that execute without babysitting. This requires skills that are unambiguous, self-recovering, and complete.

### 8.1 Principles for No-Babysit Skills

**Be explicit about decisions.** Don't write "choose an appropriate format" — write "output as a markdown file with H2 section headers." Every ambiguity is a point where the agent might stop and ask, breaking autonomy.

**Include fallback behaviors.** Instead of "if the file is missing, ask the user," write "if the file is missing, create a placeholder with [template] and note the missing input in the output header." The agent should always have a path forward.

**Define completion criteria.** The agent needs to know when it's done. "The task is complete when the output file exists at the specified path, contains all required sections, and passes the validation checks in `scripts/validate.py`."

**Handle edge cases in the instructions, not as exceptions.** If an input might be a PNG or a PDF or a Figma URL, handle all three in the workflow — don't leave it to the agent to improvise.

**Use scripts for deterministic work.** Anything that should produce identical output every time (data transformation, file format conversion, validation) belongs in a script, not in natural language instructions. Scripts are faster, more reliable, and token-efficient.

### 8.2 The Autonomy Checklist

Before shipping a skill for autonomous use, verify:

```
□ Every step has a clear, unambiguous action
□ No step requires user input to proceed (or has a default fallback)
□ Output format is fully specified with a template
□ Edge cases have explicit handling instructions
□ Validation/completion criteria are defined
□ Scripts handle all deterministic operations
□ Error states have recovery paths
□ The skill has been tested with 3+ realistic prompts
□ The description triggers reliably on natural-language requests
□ The skill works on the target platform(s) without modification
```

### 8.3 Skill Composition

Skills can work together without explicitly referencing each other. The agent loads multiple relevant skills simultaneously and combines their instructions. This is powerful for compound tasks.

**Example composition:**
A user says "create a proposal for Client X with our standard branding."
- `service-proposal` skill loads — provides the proposal template and structure
- `client-brand-apply` skill loads — provides the branding guidelines
- `anti-ai-design` skill loads — prevents generic AI aesthetics in any visual elements

Design your skills to be composable: each one handles its domain completely, with clear output contracts that other skills can build on.

### 8.4 Skills + MCP Integration

Skills teach agents *what to do*. MCP (Model Context Protocol) gives agents *access to external tools*. They're complementary.

A skill can instruct the agent to use MCP servers for specific operations:
- "Use the Google Calendar MCP to check availability before scheduling"
- "Query the Supabase MCP for user data before generating the report"
- "Push the completed file to Google Drive via the Drive MCP"

For Kernex/kx, MCP servers are configured in `.kx.toml` and skills reference them in instructions. The OS-level sandbox controls which network endpoints the agent can reach.

---

## 9. Skill Taxonomy for Your Ecosystem

Based on your four domains, here's a recommended skill library structure:

```
skills/
├── product/
│   ├── product-feature-spec/
│   ├── product-launch-checklist/
│   ├── product-analytics-setup/
│   ├── product-mdx-content/
│   └── product-remotion-video/
├── client/
│   ├── client-deliverable/
│   ├── client-onboarding-checklist/
│   ├── client-brand-apply/
│   ├── client-geo-strategy/
│   └── client-ghl-development/
├── design/
│   ├── design-system-component/
│   ├── design-token-generator/
│   ├── figma-to-code/
│   ├── svg-icon-set/
│   ├── design-audit/
│   └── anti-ai-design/          # Already built
├── content/
│   ├── service-proposal/
│   ├── service-contract/
│   ├── content-linkedin/
│   ├── content-blog/
│   └── content-case-study/
└── ops/
    ├── move-logistics/          # Austin relocation workflows
    ├── llc-compliance/          # Florida LLC admin tasks
    ├── invoice-generator/       # Client billing
    └── sprint-planning/         # Weekly sprint structure
```

Each skill follows the same SKILL.md format. Each works across Claude.ai, Claude Code, and Kernex/kx. Each is designed for autonomous operation.

---

## 10. Quick Reference Card

### Frontmatter Constraints
| Field | Required | Max Length | Format |
|-------|----------|-----------|--------|
| name | Yes | 64 chars | lowercase, hyphens only, matches directory |
| description | Yes | 200 chars | Natural language, trigger-oriented |
| license | No | — | License name or filename |
| metadata | No | — | String key-value pairs |
| compatibility | No | — | Dependencies list |

### Size Targets
| Component | Recommended Max | Notes |
|-----------|----------------|-------|
| SKILL.md body | 500 lines / <5K tokens | Split into references if longer |
| Individual reference files | 300 lines | Add TOC if approaching limit |
| Description | 200 characters | Every word must earn its place |
| Metadata scan cost | ~30-100 tokens/skill | Multiplied by total installed skills |

### The Creation Loop (Summary)
```
Define Intent → Research → Draft SKILL.md → Write Test Cases →
Test & Review → Iterate → Optimize Description → Package → Ship
```

### Platform Deployment
```
Claude.ai  →  Upload via UI or /v1/skills API
Claude Code →  Copy to .claude/skills/ (live detection)
Kernex/kx  →  Copy to .kx/skills/ + kx init
Cross-plat →  Publish to GitHub, use npx skills add
```

---

*This playbook is itself a living document. Update it as the Agent Skills specification evolves and as your skill library grows.*
