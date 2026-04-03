# Designing Skills for Autonomous Operation

How to write skills that agents execute without human intervention.

## The Autonomy Spectrum

```
Level 0: Agent asks for clarification at every step          ← useless
Level 1: Agent completes the task but requires format review  ← common
Level 2: Agent produces output that needs no revision         ← target
Level 3: Agent handles multi-step workflows end-to-end        ← ideal
```

Most skills start at Level 1. The goal is Level 2-3.

## Five Rules for No-Babysit Skills

### Rule 1: No Ambiguous Decisions

Every branch in the workflow must have a clear resolution.

Bad: "Choose an appropriate tone for the audience."
Good: "Use professional-casual tone: direct sentences, no jargon, contractions OK. If the deliverable is for a C-level audience, shift to formal: no contractions, structured paragraphs, data-first."

Bad: "Format the output appropriately."
Good: "Output as a markdown file with: H1 title, H2 section headers, bullet points for lists, code blocks for technical content. Save to `/outputs/[skill-name]-[timestamp].md`."

### Rule 2: Default Forward, Never Block

The agent should always have a path forward. Never write "ask the user" as the only option.

Bad: "If the client name is not specified, ask which client this is for."
Good: "If the client name is not specified, use 'General' as the client identifier and note 'Client: Unspecified — update before delivery' in the output header."

Bad: "If the file format is unsupported, inform the user."
Good: "If the file format is unsupported, convert to the nearest supported format (e.g., .webp → .png, .rtf → .txt) and note the conversion in the output metadata. If no conversion is possible, save the output as markdown."

### Rule 3: Define "Done" Explicitly

The agent needs to know when the task is complete. Without completion criteria, it might over-elaborate, stop too early, or loop.

Add a completion section to every skill:

```markdown
## Completion

The task is complete when:
1. The output file exists at the specified path
2. All required sections are present (check against the template)
3. No placeholder text remains in the output
4. File size is > 0 bytes
```

For skills with scripts, add validation:

```markdown
## Validation

Run `scripts/validate.py [output-path]` before marking complete.
Expected: "PASS" with 0 warnings. If warnings appear, fix them and rerun.
```

### Rule 4: Handle Every Input Variant

List all foreseeable input types and how to handle each.

```markdown
## Input Handling

| Input Type | How to Process |
|-----------|---------------|
| PNG/JPG screenshot | Analyze visually, extract UI elements |
| Figma URL | Fetch via Figma API if available, otherwise ask for export |
| Verbal description | Generate wireframe-level interpretation, note assumptions |
| PDF mockup | Extract pages as images, process each |
| No input provided | Use the default template and note "No input provided — using template" |
```

### Rule 5: Scripts for Determinism

Anything that should produce identical output every time belongs in a script.

Agent instructions are probabilistic — the same instruction might produce slightly different output each run. Scripts are deterministic. Use them for:

- File format conversions
- Data validation and cleaning
- Template rendering with variables
- Calculations and metrics
- File path operations and naming

Keep scripts stdlib-only Python when possible (maximum portability across platforms). Include error handling and helpful error messages.

## Composition Patterns for Multi-Skill Workflows

When multiple skills activate simultaneously, they should enhance each other, not conflict.

### Pattern: Layered Application
```
Base skill:    service-proposal     → generates the proposal structure
Modifier skill: client-brand-apply  → applies client-specific branding
Quality skill:  anti-ai-design      → ensures visual quality
```

Each skill operates on its domain without overriding the others. Design skills to be additive — they should add constraints and context, not replace previous skill output.

### Pattern: Pipeline Handoff
```
Skill A output → becomes Skill B input
product-feature-spec → design-system-component → product-launch-checklist
```

For pipeline skills, define clear output contracts — the output format of Skill A must match the expected input format of Skill B.

### Pattern: Conditional Activation
```
If input is a screenshot → design-audit activates
If input is a Figma URL → figma-to-code activates
If input is a text description → product-feature-spec activates
```

The agent routes to the right skill based on input type. Descriptions should make these boundaries clear.

## Testing for Autonomy

After writing a skill, run this simulation:

1. Give the agent the prompt with NO additional context
2. Do not answer any follow-up questions
3. Wait for the full output
4. Evaluate: did the agent complete the task end-to-end?

If the agent stopped to ask a question → the skill has an ambiguity gap. Fix it.
If the output is incomplete → the skill is missing a workflow step. Add it.
If the output format is wrong → the template section is unclear. Rewrite it.

Repeat until the agent completes 3 different test prompts without any human input.
