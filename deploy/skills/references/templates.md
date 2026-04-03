# Domain-Specific SKILL.md Templates

Templates organized by Jose's five operational domains. Copy the relevant template and customize for your specific skill.

---

## §product — Product Development Skills

Use for uxuiprinciples.com, interfaceaudit.com, geoautopilot.com, and any product feature work.

```markdown
---
name: [product-skill-name]
description: [Action verb] + [specific output] + [trigger contexts]. Example: "Generate feature specifications with user stories and acceptance criteria. Use when planning features, writing PRDs, or defining requirements for any product in the portfolio."
metadata:
  author: jose-hurtado
  version: "0.1"
  domain: product
---

# [Skill Name]

[What this skill produces and why it matters for the product.]

## Context

Tech stack: Next.js 14+, React, Supabase, GSAP, Tailwind CSS, MDX.
Products: uxuiprinciples.com (112-principle UX framework), interfaceaudit.com (diagnostic tool), geoautopilot.com (geo-automation).

## Workflow

1. [Identify the product and feature scope]
2. [Generate the output using the specified format]
3. [Validate against the product's existing architecture]
4. [Save to the appropriate output path]

## Output Format

[Exact structure — paste the actual template here]

## Examples

**Example 1:**
Input: "add a freemium tier comparison table to uxuiprinciples — show what's free vs paid for the icon set and components"
Output: [MDX component with pricing comparison, responsive, using existing design tokens]

## Edge Cases

- If the target product is unclear, default to uxuiprinciples.com
- If the feature conflicts with existing architecture, note the conflict and propose a migration path
```

---

## §client — Client Work Skills

Use for DXagency, DXkulture, Fortune 500 client deliverables, vendor onboarding, and strategy reports.

```markdown
---
name: [client-skill-name]
description: [Action verb] + [deliverable type] + [client context]. Example: "Create client-facing strategy reports with executive summaries, data visualizations, and recommendations. Use for DXagency deliverables, vendor onboarding checklists, or client presentations."
metadata:
  author: jose-hurtado
  version: "0.1"
  domain: client
---

# [Skill Name]

[What this skill produces and the quality standard it enforces.]

## Brand Loading

Before generating any output, check if client-specific brand guidelines exist:
- If `references/brands/[client-name].md` exists, load and apply it
- If no client brand file exists, use the neutral professional template
- Never mix brand guidelines between clients

## Workflow

1. [Identify the client and deliverable type]
2. [Load the relevant brand guidelines from references/]
3. [Generate the deliverable using the brand-appropriate template]
4. [Apply quality gate — see Quality section]
5. [Export in the requested format (.docx, .pptx, .pdf)]

## Quality Gate

Before finalizing any client deliverable:
- Does it use the correct client brand colors and typography?
- Is the executive summary under 200 words?
- Are recommendations numbered and actionable?
- Would you present this to a Fortune 500 CMO without changes?

## Output Format

[Client deliverable template — header, sections, footer]

## Examples

**Example 1:**
Input: "create the Nestlé GA4 vendor onboarding checklist — include access provisioning, tag setup, and training schedule"
Output: [Branded checklist document with Nestlé colors, numbered steps, responsible parties, and deadlines]
```

---

## §design — Design Systems & Code Generation Skills

Use for component creation, Figma-to-code workflows, design tokens, icon sets, and interface audits.

```markdown
---
name: [design-skill-name]
description: [Action verb] + [design output] + [trigger contexts]. Example: "Build React components from Figma designs with proper design tokens, accessibility, and anti-AI aesthetics. Use when converting designs to code, creating component libraries, or building design system elements."
metadata:
  author: jose-hurtado
  version: "0.1"
  domain: design
---

# [Skill Name]

[What this skill produces and the design philosophy it follows.]

## Design Standards

All visual output must pass the anti-AI quality gate:
- No purple-blue gradients (#667eea → #764ba2)
- No glassmorphism on regular cards
- No floating orbs/blobs/gradient spheres
- No uniform 3×2 feature grids — create hierarchy
- No "Revolutionize/Transform/Unleash" headlines
- No infinite float/pulse/bounce animations
- Solid surfaces with subtle shadows for elevation
- Specific benefits with numbers in headlines

## Workflow

1. [Analyze the design input (screenshot, Figma URL, verbal description)]
2. [Determine the component architecture (atomic design level)]
3. [Generate the code with proper tokens and accessibility]
4. [Run the visual quality gate]
5. [Export as .jsx, .tsx, or .html depending on context]

## Code Standards

- Use Tailwind CSS utility classes (core classes only — no compiler)
- CSS variables for design tokens
- WCAG 2.1 AA contrast ratios (4.5:1 minimum for text)
- Semantic HTML elements
- Respect prefers-reduced-motion
- Default export, no required props (or provide defaults)

## Examples

**Example 1:**
Input: "build a pricing card component for uxuiprinciples — 3 tiers, free/pro/team, highlight pro as recommended"
Output: [React component with visual hierarchy (pro tier larger), brand colors, no AI clichés, accessible]
```

---

## §content — Content, Proposals & Service OS Skills

Use for proposals, contracts, SOWs, blog posts, LinkedIn content, case studies, and Service OS documentation.

```markdown
---
name: [content-skill-name]
description: [Action verb] + [content type] + [trigger contexts]. Example: "Draft client proposals with scope, timeline, pricing, and terms. Use when creating SOWs, proposals, contracts, or any formal business document for Visual Brands LLC or User Centric Studio."
metadata:
  author: jose-hurtado
  version: "0.1"
  domain: content
---

# [Skill Name]

[What this skill produces and the business context.]

## Entity Context

- Business: Visual Brands LLC / User Centric Studio (Florida LLC)
- Role: Senior UX/UI Design Director, independent contractor
- Services: UX/UI design, design systems, product strategy, AI-native development
- Rates and terms: See references/pricing.md

## Workflow

1. [Identify the content type and target audience]
2. [Load the relevant template from references/]
3. [Generate the content with proper business language]
4. [Apply the appropriate legal/compliance notes]
5. [Export in the requested format]

## Tone Guidelines

- Professional but not corporate — direct, confident, specific
- Lead with outcomes and evidence, not buzzwords
- Reference the 112-principle UX framework when relevant
- Bilingual capability: generate in English or Spanish as requested

## Examples

**Example 1:**
Input: "write a proposal for a UX audit of a fintech checkout flow — 3 week timeline, include the interface audit methodology"
Output: [Branded proposal with scope, methodology (referencing interfaceaudit.com), timeline, deliverables, pricing, terms]
```

---

## §ops — Operational & Administrative Skills

Use for logistics, compliance, invoicing, sprint planning, and recurring administrative tasks.

```markdown
---
name: [ops-skill-name]
description: [Action verb] + [operational task] + [trigger contexts]. Example: "Generate weekly sprint plans with task priorities, time estimates, and dependencies. Use when planning sprints, organizing tasks, or creating project timelines."
metadata:
  author: jose-hurtado
  version: "0.1"
  domain: ops
---

# [Skill Name]

[What this skill automates and why it saves time.]

## Workflow

1. [Identify the operational context]
2. [Execute the task using the standardized process]
3. [Validate the output against the completion criteria]
4. [Save/send the output to the appropriate destination]

## Completion Criteria

[Define exactly what "done" looks like — file saved, email sent, checklist completed]

## Examples

**Example 1:**
Input: [realistic operational request]
Output: [expected output with specific format]
```
