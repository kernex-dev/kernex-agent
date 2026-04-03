---
name: uxui-evaluator
description: Evaluate designs against 168 UX/UI principles across cognition, heuristics, design systems, patterns, and accessibility. Use for design reviews and pre-launch checks. Returns severity-graded findings.
metadata:
  author: kernex-dev
  version: "1.0"
  domain: review
---

# UX/UI Evaluator

Evaluate interfaces and designs against the 168-principle taxonomy from UXUIPrinciples — organized into 6 evidence-backed domains with 2,098+ academic citations. Every finding must cite the principle code and a quantified business impact where available. No vague feedback.

## Taxonomy Overview

| Part | Code | Domain | Principles |
|------|------|--------|------------|
| I | F.1.x | Cognitive Psychology (Chunking, Cognitive Load, Hick's Law, Miller's Law, Gestalt) | 32 |
| II | C.1.x | Core Heuristics (Nielsen's 10, Fitts's Law, Error Prevention, Feedback) | 34 |
| III | D.1.x | Design Systems (Visual Hierarchy, Typography, Color, Navigation, Progressive Disclosure) | 22 |
| IV | I.4.x | Interface Patterns (Forms, Data Viz, AI Assistance, Conversational UX) | 23 |
| V | S.1.x | Specialized Domains (AI Ethics, Transparency, Bias, Trust Calibration, Enterprise) | 44 |
| VI | H.x.x | Human-Centered (WCAG, Ethical Design, Dark Patterns, Inclusive Wellbeing) | 13 |

## Core Rules

- Cite the principle code for every finding (e.g., F.1.1.02 for Cognitive Load).
- Severity must match the principle's research-backed weight. Do not inflate.
- Every fix must be actionable: a specific component change, not "improve hierarchy."
- AI-related patterns (Part V) apply to any interface that shows, generates, or interacts with AI output.
- Do not evaluate what was not provided. List gaps explicitly.

## Severity Scale

- **Critical (8-10)**: Directly impairs task completion. Fix before launch. Cognitive Load (10), Visual Hierarchy (9), Consistency (9), Hick's Law (9), Error Prevention (9), Fitts's Law (9).
- **High (6-7)**: Measurably degrades performance or trust. Fix in current sprint.
- **Medium (4-5)**: Noticeable friction. Schedule for next iteration.
- **Low (1-3)**: Best-practice gaps without direct task impact.

## Workflow

1. Identify what was provided: screenshot, code, wireframe, description, or combination.
2. Determine the interface type: form, dashboard, onboarding flow, AI assistant, navigation, content page.
3. Apply the relevant principle domains. Not all 168 apply to every interface.
4. For each violation: cite code, describe the observable evidence, state the fix, note the business impact.
5. Identify strengths: what the design does well (evidence-backed, not praise).
6. List gaps: what could not be evaluated from the provided context.
7. Return structured output.

## Output Format

```json
{
  "interface_type": "form | dashboard | onboarding | ai-assistant | navigation | content | other",
  "scope": "what was provided for evaluation",
  "findings": [
    {
      "principle_code": "F.1.1.02",
      "principle_name": "Cognitive Load",
      "part": "Foundations",
      "severity": "critical | high | medium | low",
      "severity_score": 10,
      "observation": "specific, observable issue in the provided design",
      "fix": "concrete component-level change",
      "business_impact": "quantified outcome from research (e.g., '30-40% reduced error rate')"
    }
  ],
  "strengths": [
    {"principle_code": "C.1.2.01", "observation": "evidence-backed positive finding"}
  ],
  "priority_fixes": ["top 3 fixes ordered by severity score * user impact"],
  "gaps": ["what could not be evaluated and why"],
  "overall_grade": "A | B | C | D | F",
  "summary": "2-3 sentence honest assessment referencing the most critical findings"
}
```

## Key Principles Reference

**Cognitive (Part I — highest impact):**
- F.1.1.02 Cognitive Load: 3 load types — intrinsic (task), extraneous (bad design), germane (learning). Optimized interfaces improve productivity 500%.
- F.1.1.01 Chunking: 7±2 working memory slots. Chunk related items 8-16px within, 24-48px between groups.
- F.1.2.03 Hick's Law: T = a + b log₂(n). Doubling choices adds ~150ms. Cap primary navigation at 5-7 items.

**Core Heuristics (Part II):**
- C.1.1.01 Consistency: 30-40% reduced cognitive load. Same action, same pattern, every time.
- C.1.2.01 Visibility of System Status: Real-time feedback on what the system is doing.
- F.2.1.02 Fitts's Law: Touch targets minimum 44px (WCAG 2.5.5). Primary CTAs 50px+.
- F.2.3.01 Error Prevention: Four levels — warnings, constraints, confirmations, recovery.

**Design Systems (Part III):**
- D.1.2.03 Visual Hierarchy: Max 3-5 levels. One clear focal point per screen.
- D.1.1.01 Progressive Disclosure: Show vital few first. Complex options on demand. 20-30% higher completion.
- D.2.2.01 Typography: Line height 1.6 body, 1.2 headings. 45-75 character line length.

**AI-Specific (Part V):**
- S.1.3.01 AI Transparency: Explicit "AI-generated" labels, confidence indicators, "why this?" expandables.
- I.4.2.01 Confidence Indicators: Show AI certainty levels (high/medium/low). 40-60% better decision accuracy.
- S.1.3.02 AI User Control: Override/reject for consequential decisions. Feedback mechanisms.
- S.1.3.07 Efficient AI Dismissal: Easy close/dismiss — never force AI interaction.

**Accessibility (Part VI):**
- H.1.1.01 WCAG Perceivable: 4.5:1 contrast ratio for normal text.
- H.1.1.02 WCAG Operable: All functionality keyboard-accessible. 44px touch targets.
- H.2.2.03 Dark Patterns: Roach motel, confirmshaming, disguised ads are F-grade violations.

## Examples

**Example 1:**
Input: "Dashboard with 15 KPI cards, 4 filter dropdowns, a data table, and 3 sidebar actions all visible simultaneously."
Output:
```json
{
  "interface_type": "dashboard",
  "scope": "Dashboard description — no screenshot provided",
  "findings": [
    {
      "principle_code": "F.1.1.02",
      "principle_name": "Cognitive Load",
      "part": "Foundations",
      "severity": "critical",
      "severity_score": 10,
      "observation": "15 simultaneous KPI cards plus 4 filter controls exceeds 7±2 working memory capacity. The extraneous load from layout processing competes with intrinsic task load.",
      "fix": "Group KPIs into 3-5 semantic categories. Show top 5 by default, collapse remainder behind 'More'. Separate filter controls into a drawer or top bar.",
      "business_impact": "Optimized dashboards improve task completion speed 40-50% and reduce error rates 30-40%"
    },
    {
      "principle_code": "I.4.2.02",
      "principle_name": "Dashboard Information Density",
      "part": "Interface Patterns",
      "severity": "high",
      "severity_score": 7,
      "observation": "No mention of metric grouping headers or drill-down affordances.",
      "fix": "Add group headers (e.g., 'Revenue', 'Acquisition', 'Retention'). Each card should be clickable to a detail view.",
      "business_impact": "Grouped dashboards reduce time-to-insight 25-35%"
    }
  ],
  "strengths": [],
  "priority_fixes": ["Reduce visible KPIs to 5 with progressive disclosure", "Group with semantic headers", "Separate filters into drawer"],
  "gaps": ["No screenshot — cannot evaluate visual hierarchy, contrast, or typography"],
  "overall_grade": "D",
  "summary": "Critical cognitive overload violation with 15 simultaneous KPIs. Core issues are extraneous load (F.1.1.02) and information density (I.4.2.02). Cannot grade accessibility or visual design without screenshot."
}
```

## Edge Cases

- **AI assistant interfaces**: Always apply Part V principles (S.1.3.01-S.1.3.14). Confidence indicators and user override are non-negotiable.
- **No visual provided, description only**: Evaluate information architecture and stated interaction patterns. Flag visual properties as gaps.
- **Mobile-only**: Apply Fitts's Law (44px targets) and WCAG Operable (H.1.1.02) before visual principles.

## References

- uxuiprinciples.com — 168-principle taxonomy, 2,098 academic citations
- Nielsen (1990) — 10 usability heuristics
- Sweller (1988) — Cognitive Load Theory
- WCAG 2.1 — Accessibility standard
