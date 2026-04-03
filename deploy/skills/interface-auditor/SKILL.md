---
name: interface-auditor
description: Audit interfaces for antipatterns using 168 principles with 1-10 severity scoring. Identify cognitive overload, inconsistency, visual hierarchy failures. Use for pre-launch and redesign scoping.
metadata:
  author: kernex-dev
  version: "1.0"
  domain: review
---

# Interface Auditor

Antipattern detection engine for UI/UX audits. Scan the provided interface against 168 observable violations organized by severity. Every finding needs a specific symptom, not a category name. "Cognitive overload" is not a finding — "14 visible action items with no grouping or hierarchy" is.

## Core Rules

- Every finding requires observable evidence. No theoretical violations.
- Severity score (1-10) must match the principle's research-backed weight.
- Fix effort must be realistic: quick (CSS/copy change), medium (component refactor), complex (architecture change).
- Critical findings (8-10) block launch. List them first, unconditionally.
- Do not invent violations for completeness. Absence of evidence is a gap, not a finding.

## Severity Scale (1-10)

**Critical (8-10) — Fix before launch:**
- Cognitive Load violation (10): Simultaneous information exceeds working memory
- Consistency failure (9): Different UI patterns for identical actions
- Visual Hierarchy absent (9): No clear focal point, equal visual weight everywhere
- Choice Overload (9): 10+ equivalent options without structure
- Hick's Law violation (9): Navigation with 15+ flat options
- Error Prevention missing (9): Destructive action with no confirmation
- Jakob's Law violation (9): Breaks established platform conventions
- Fitts's Law failure (9): Touch targets below 44px
- Doherty Threshold (9): Response time exceeds 400ms with no feedback

**High (6-7) — Fix in current sprint:**
- Progressive Disclosure absent (7): Complex features all visible at once
- Form graveyard (7): 10+ unlabeled or unchunked fields
- Silent errors (7): No error state, validation, or failure feedback
- Missing system status (6): No loading, saving, or completion indicators
- Split-attention design (6): Legend far from chart, help text distant from field

**Medium (4-5) — Next iteration:**
- Weak typography hierarchy (5): 2+ font sizes with similar visual weight
- Missing breadcrumbs or wayfinding (5): No location indicators in deep navigation
- Inconsistent spacing (4): No consistent spacing rhythm across components

**Low (1-3) — Backlog:**
- Missing OG/meta for sharing (3)
- Non-semantic heading order (2)
- Missing focus indicators (3, escalates to Critical if accessibility-required)

## Workflow

1. Identify the interface type and provided context (screenshot, code, HTML, description).
2. Scan for Critical violations first. Stop and report if any are present.
3. Continue through High and Medium categories.
4. Tally the issue count by severity.
5. Calculate an overall grade based on severity distribution.
6. Flag what could not be audited from the provided context.
7. Return structured output.

## Output Format

```json
{
  "interface_type": "form | dashboard | onboarding | navigation | content | ai-assistant | other",
  "scope": "what was audited",
  "score": {
    "critical": 0,
    "high": 0,
    "medium": 0,
    "low": 0,
    "total": 0
  },
  "findings": [
    {
      "id": "AUD-001",
      "principle_code": "F.1.1.02",
      "principle_name": "Cognitive Load",
      "severity": "critical | high | medium | low",
      "severity_score": 10,
      "symptom": "exact observable issue in the provided UI",
      "evidence": "specific element, selector, or location",
      "fix": "concrete change with effort estimate",
      "effort": "quick | medium | complex",
      "business_impact": "quantified metric from research"
    }
  ],
  "ux_smells": [
    "Overloaded Screen | Form Graveyard | Silent Errors | Mystery Navigation | Cognitive Overload | Lost in Space | Analysis Paralysis | Hidden Complexity"
  ],
  "gaps": ["what could not be audited from the provided context"],
  "grade": "A | B | C | D | F",
  "launch_blocker": true,
  "summary": "X critical, Y high issues found. Top blocker: [specific issue]. Grade [X]."
}
```

## UX Smell Patterns (fast detection)

These are compound patterns — one smell typically maps to 3-5 individual violations:

| Smell | Symptoms | Primary violations |
|-------|----------|--------------------|
| Overloaded Screen | 7+ action areas, no visual hierarchy | F.1.1.02, D.1.2.03, F.2.2.01 |
| Form Graveyard | 20+ fields, no chunking, no progress | C.1.1.01, F.1.1.01, F.2.3.02 |
| Silent Errors | No error states, vague messages | F.2.3.01, C.1.2.01, F.2.3.02 |
| Mystery Navigation | Unclear structure, no "you are here" | D.3.2.01, D.3.1.01, C.1.2.02 |
| Cognitive Overload | Too many simultaneous choices | F.1.1.02, F.2.2.01, F.1.2.03 |
| Lost in Space | No breadcrumbs, unclear context | D.3.2.01, D.1.2.02, C.1.2.01 |
| Analysis Paralysis | 10+ equivalent options, no defaults | F.2.2.01, F.1.2.03, D.1.1.01 |
| Hidden Complexity | Features buried, discovery broken | D.1.1.01, D.3.1.01, C.1.2.03 |

## Grading Rubric

| Grade | Criteria |
|-------|----------|
| A | 0 critical, 0-1 high, 0-3 medium. Consistent system, clear hierarchy. |
| B | 0 critical, 2-4 high. Core flows work cleanly. Minor polish gaps. |
| C | 0-1 critical (minor), 5-8 high. Usable but friction-heavy. |
| D | 2-4 critical. Core flows blocked or confusing. Launch risky. |
| F | 5+ critical. Task completion impaired. Do not launch. |

## Examples

**Example 1:**
Input: "Checkout form with 22 fields (billing + shipping + payment on one screen), no inline validation, submit button at bottom, no progress indicator."
Output:
```json
{
  "interface_type": "form",
  "scope": "Checkout form description",
  "score": {"critical": 2, "high": 2, "medium": 1, "low": 0, "total": 5},
  "findings": [
    {
      "id": "AUD-001",
      "principle_code": "F.1.1.02",
      "principle_name": "Cognitive Load",
      "severity": "critical",
      "severity_score": 10,
      "symptom": "22 fields on a single screen with no chunking. Extraneous load from layout processing competes with task completion.",
      "evidence": "Full checkout on one page — billing, shipping, payment",
      "fix": "Split into 3 steps: Shipping > Payment > Review. 5-7 fields per step.",
      "effort": "medium",
      "business_impact": "Multi-step checkout improves completion rates 20-35%"
    },
    {
      "id": "AUD-002",
      "principle_code": "F.2.3.02",
      "principle_name": "Form Validation",
      "severity": "critical",
      "severity_score": 9,
      "symptom": "No inline validation. Errors only surfaced on submit — users fix the last thing they see, not the actual first error.",
      "evidence": "No mention of real-time validation",
      "fix": "Add real-time validation on blur for email, card number, ZIP. Show success state (green checkmark) per field.",
      "effort": "medium",
      "business_impact": "Inline validation reduces form abandonment 22%"
    }
  ],
  "ux_smells": ["Form Graveyard"],
  "gaps": ["Cannot verify visual hierarchy, contrast, or touch target sizes without screenshot"],
  "grade": "D",
  "launch_blocker": true,
  "summary": "2 critical, 2 high issues. Launch blocker: 22-field single-page form with no inline validation. Grade D. Split into steps and add real-time validation before launch."
}
```

## Edge Cases

- **Screenshot provided**: Identify specific elements. Use visual cues (color, size, position) as evidence.
- **Code provided**: Check for `min-height`/`padding` on interactive elements (Fitts's Law), error handling, aria attributes.
- **AI assistant interface**: Apply S.1.3.x principles. Missing confidence indicators are a High violation.
- **Mobile**: Fitts's Law failures (targets below 44px) escalate to Critical on mobile.

## References

- interfaceaudit.design — 168-principle antipattern taxonomy, severity scoring system
- Nielsen (1990) — Usability heuristics
- Brignull (2010) — Dark patterns taxonomy
- WCAG 2.1 — Accessibility standards
