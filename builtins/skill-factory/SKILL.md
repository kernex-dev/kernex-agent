---
name: skill-factory
description: Create and iterate on SKILL.md files for any domain. Use when building skills, turning workflows into skills, or on "make this a skill" or "skill template".
metadata:
  author: jose-hurtado
  version: "1.0"
  domain: ops
---

# Skill Factory

Create production-grade Agent Skills that work autonomously across Claude.ai, Claude Code, Kernex/kx, and 26+ platforms without babysitting.

## When This Skill Activates

- User wants to create a new skill from scratch
- User wants to turn an existing conversation/workflow into a skill
- User wants to improve an existing skill (triggering, instructions, output quality)
- User wants to validate or package a skill for distribution
- User mentions "SKILL.md", "agent skill", or "make this repeatable"

## Core Philosophy

Skills are onboarding documents for AI. Write them the way you'd brief a brilliant new hire: explain the *why* behind every instruction, provide clear examples, and define what "done" looks like. Avoid heavy-handed MUSTs and oppressive constraints — explain reasoning so the agent can generalize beyond the specific examples.

## Creation Workflow

### Step 1: Capture Intent

Ask or extract from conversation context:

1. **What should the skill enable?** Get specific actions, not vague goals.
2. **What triggers it?** List phrases, contexts, and task types a real user would say.
3. **What is the output?** File type, format, structure, length.
4. **What makes this different from native agent capability?** If the agent handles it fine without a skill, don't build one.

If the conversation already contains a workflow the user wants to capture (e.g., "turn this into a skill"), extract the answers from conversation history first: tools used, step sequence, corrections made, input/output formats observed. Confirm with the user before proceeding.

### Step 2: Classify the Skill Domain

Determine which domain this skill belongs to. This affects the template, the output conventions, and the reference materials to include.

| Domain | Characteristics | Template |
|--------|----------------|----------|
| product | Feature specs, PRDs, analytics setup, content architecture | See `references/templates.md` §product |
| client | Deliverables, brand application, vendor onboarding, strategy reports | See `references/templates.md` §client |
| design | Components, tokens, audits, Figma-to-code, icon sets | See `references/templates.md` §design |
| content | Proposals, contracts, blog posts, LinkedIn, case studies | See `references/templates.md` §content |
| ops | Admin tasks, compliance, invoicing, logistics, sprint planning | See `references/templates.md` §ops |

### Step 3: Draft the SKILL.md

Use this exact structure. Every field and section exists for a reason.

```markdown
---
name: [lowercase-hyphenated, matches directory name, max 64 chars]
description: [What it does + when to trigger. Max 200 chars. Be specific and pushy.]
metadata:
  author: jose-hurtado
  version: "0.1"
  domain: [product|client|design|content|ops]
---

# [Skill Name — Human Readable]

[2-3 sentences: what this skill does and the philosophy behind it.]

## Workflow

[Imperative, numbered steps. Each step is a clear action.]

1. [Action with specific instruction]
2. [Action with specific instruction]
3. [Action with specific instruction]

## Output Format

[Exact template or structure of expected output. Show the actual format.]

## Examples

**Example 1:**
Input: [realistic user prompt — casual, with context]
Output: [what the agent should produce]

## Edge Cases

- [Pitfall]: [How to handle it — always provide a path forward, never "ask the user"]

## References

- For [scenario], see [references/specific-file.md]
- Run [scripts/helper.py] when [condition]
```

### Step 4: Apply Autonomy Standards

Before considering the draft complete, verify every instruction against the autonomy checklist:

```
□ Every step has a clear, unambiguous action
□ No step requires user input to proceed (or has a default fallback)
□ Output format is fully specified with a template
□ Edge cases have explicit handling — not "ask the user"
□ Validation/completion criteria are defined
□ Scripts handle all deterministic operations
□ Error states have recovery paths, not dead ends
□ Description triggers reliably on natural-language requests
```

If any item fails, revise the SKILL.md before proceeding to testing.

### Step 5: Write Test Cases

Create 2-3 realistic test prompts. These should sound like real people asking for real things — not clean, formal requests.

Good test prompts include:
- Casual language, abbreviations, maybe a typo
- Context about the situation ("my boss wants this by friday")
- Ambiguous inputs that test edge case handling
- Different phrasings of the same intent

Save to `evals/evals.json`:
```json
{
  "skill_name": "[skill-name]",
  "evals": [
    {
      "id": 1,
      "prompt": "[realistic messy prompt]",
      "expected_output": "[description of expected result]",
      "files": []
    }
  ]
}
```

### Step 6: Test and Iterate

Run each test case using the skill's instructions. For each result:

1. Does the output match the expected format exactly?
2. Did the agent follow the workflow without improvising?
3. Did edge cases get handled per the instructions?
4. Would a client/stakeholder accept this output without revision?

**Iteration principles:**
- **Generalize from failures.** Don't overfit to specific test cases — the skill will run thousands of times on different inputs.
- **Keep the prompt lean.** Remove instructions that aren't pulling their weight.
- **Explain the why.** If you're tempted to write ALWAYS or NEVER in caps, reframe as reasoning the agent can internalize.
- **Bundle repeated work.** If every test case independently creates the same helper logic, put it in `scripts/`.

### Step 7: Optimize Description

After the skill works well, stress-test the triggering:

1. Write 10 should-trigger queries (diverse phrasings, casual to formal)
2. Write 10 should-not-trigger queries (tricky near-misses, not obvious irrelevant)
3. Test trigger rate — the description should fire on all 10 positives and none of the 10 negatives
4. Revise and retest until stable

### Step 8: Package

**Directory structure for delivery:**
```
skill-name/
├── SKILL.md
├── scripts/          (if applicable)
├── references/       (if applicable)
├── assets/           (if applicable)
└── evals/
    └── evals.json
```

**Deploy to target platforms:**
- Claude.ai: Upload via UI or `/v1/skills` API
- Claude Code: Copy to `.claude/skills/`
- Kernex/kx: Copy to `.kx/skills/`
- GitHub: Publish for cross-platform installation

## Quality Criteria

A skill is ready for autonomous operation when:

1. **It triggers reliably** — 10/10 positive matches, 0/10 false positives
2. **It produces consistent output** — 3 runs on the same prompt yield structurally identical results
3. **It handles edge cases** — every foreseeable failure has a recovery path
4. **It composes well** — doesn't conflict with other installed skills
5. **It's portable** — works on Claude.ai, Claude Code, and Kernex/kx without modification
6. **A human would accept its output** — no revision needed for the target audience

## Anti-Patterns to Avoid

| Anti-Pattern | Why It Fails | Fix |
|-------------|-------------|-----|
| Vague description ("helps with documents") | Won't trigger, too generic | Name the specific action and context |
| "Ask the user for clarification" | Breaks autonomy | Provide a sensible default and note the assumption |
| Giant monolithic SKILL.md (800+ lines) | Floods context, dilutes signal | Split into SKILL.md + references/ |
| No examples | Agent improvises output format | Include 2+ input/output examples |
| Overfitting to test cases | Fails on real-world variation | Generalize instructions, explain reasoning |
| Keyword-stuffed description | Triggers on unrelated queries | Write natural language a user would actually say |
| Platform-specific assumptions | Breaks portability | Stick to the agentskills.io spec |

## Updating Existing Skills

When improving an existing skill:
1. Preserve the original `name` — don't rename
2. Copy to a writeable location before editing if the original is read-only
3. Increment the version in metadata
4. Run the same test cases against both old and new versions
5. Focus revision on the specific feedback — don't rewrite sections that work

## Reference Files

- See `references/templates.md` for domain-specific SKILL.md templates
- See `references/description-patterns.md` for effective description examples
- See `references/autonomy-guide.md` for detailed autonomous operation patterns
