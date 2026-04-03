# Effective Description Patterns

The `description` field (max 200 characters) is the single most important line in a SKILL.md. It determines whether the skill triggers. Write it with the same care you'd write an ad headline.

## Formula

```
[Action verb] + [specific output] + [trigger contexts/phrases]
```

## Proven Patterns

### Pattern 1: Action + Output + Context List
```
Generate UX audit reports from screenshots or URLs. Use when reviewing interfaces, identifying usability issues, or creating audit deliverables.
```
Why it works: Starts with the action, names the output, then lists three trigger contexts.

### Pattern 2: Action + Output + Negative Boundary
```
Build React components with Tailwind and design tokens. Use for UI components, not full pages or layouts.
```
Why it works: Tells the agent what to trigger on AND what not to trigger on.

### Pattern 3: Action + Domain + Phrases
```
Create client proposals and SOWs for design services. Triggers on "proposal", "scope of work", "project quote", or "send a bid".
```
Why it works: Explicitly lists the natural-language phrases users would say.

### Pattern 4: Conditional Trigger
```
Apply brand guidelines to any visual output. Auto-activates when creating presentations, documents, or marketing materials for named clients.
```
Why it works: Describes both the action and the automatic activation condition.

## Anti-Patterns

| Bad Description | Why It Fails | Better Version |
|----------------|-------------|----------------|
| "Handles documents" | Too vague, matches everything | "Convert uploaded docs to structured markdown with metadata extraction" |
| "Advanced multi-modal enterprise pipeline" | Jargon, no trigger phrases | "Extract text and tables from PDFs, fill forms, merge documents" |
| "AI-powered content creation tool" | Every skill is AI-powered | "Write blog posts for uxuiprinciples.com with SEO metadata and MDX formatting" |
| "Useful for many things" | No specificity at all | Pick ONE primary use case and name it |

## Character Budget Strategy

You have 200 characters. Spend them wisely:

- First 80 chars: What it does (the agent reads this first)
- Next 60 chars: When to use it (trigger contexts)
- Last 60 chars: Key phrases or boundaries

Count your characters. A description at 195 characters that covers all three zones will outperform a 50-character description every time.

## Testing Your Description

Ask yourself: "If a user typed [common phrase], would this description make the agent load my skill?"

Run through these tests mentally:
1. Would the casual version trigger? ("hey can you review this UI")
2. Would the formal version trigger? ("Please conduct a heuristic evaluation")
3. Would a near-miss NOT trigger? ("review this code for bugs" — not a UI review)
