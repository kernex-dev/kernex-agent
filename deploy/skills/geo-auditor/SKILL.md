---
name: geo-auditor
description: Audit a website's GEO (Generative Engine Optimization) readiness across 5 tiers: AI crawler access, schema.org, content, llms.txt. Use for sites needing visibility in ChatGPT, Perplexity, Claude.
metadata:
  author: kernex-dev
  version: "1.0"
  domain: task
---

# GEO Auditor

GEO (Generative Engine Optimization) audit engine. GEO optimizes for AI-powered answer engines (ChatGPT, Perplexity, Google AI Overviews, Claude) — not traditional SERP rankings. A site with perfect SEO can have a GEO score of 0 if its content is inaccessible to AI crawlers or lacks structured entity data.

The goal: your entity, product, or brand gets cited in AI-generated answers. Not ranked — cited.

## GEO vs SEO: Core Distinction

| Dimension | Traditional SEO | GEO |
|-----------|----------------|-----|
| Target | Google/Bing SERP position | AI chatbot citations |
| Primary signal | Backlinks, keywords, domain authority | Content factuality, structured data, crawler access |
| Tactics | Keyword research, link building | Schema.org, robots.txt, directory presence, factual density |
| Speed | 3-6 months | 2-4 weeks for visibility changes |

## Scoring Model (weighted composite)

```
Overall Score = (robots_txt × 0.25) + (schema_org × 0.30) + (content_quality × 0.30) + (ai_readiness × 0.15)
```

**Score bands:** 75-100 Excellent | 50-74 Good | 25-49 Needs Work | 0-24 Poor
**Real-world baseline:** Most sites score 25-40 before GEO optimization.

## Five-Tier Framework

### Tier 1: robots.txt AI Crawler Access (25% weight)

Nine AI crawlers to check:

| Crawler | Bot Name | Represents |
|---------|----------|------------|
| `gptbot` | ChatGPT/OpenAI | ChatGPT knowledge, Bing AI |
| `claudebot`, `claude-web` | Anthropic | Claude |
| `perplexitybot` | Perplexity | Perplexity AI |
| `google-extended` | Google | AI Overviews |
| `ccbot` | Cohere | Cohere AI |
| `anthropic-ai` | Anthropic | Anthropic training |
| `chatgpt-user` | OpenAI | Live ChatGPT browsing |
| `bytespider` | ByteDance | TikTok AI |
| `applebot-extended` | Apple | Apple AI |

**Critical issues:** Blocking gptbot, claudebot, or perplexitybot is a launch blocker.

### Tier 2: Schema.org Structured Data (30% weight — highest ROI)

High-value entity types AI engines prioritize:
- Organization / LocalBusiness
- Product / SoftwareApplication / Service
- Article / BlogPosting / NewsArticle
- FAQPage / HowTo
- Person / WebSite / WebPage

Quality signals per schema: `@type` (required), `name`, `description`, `image`, `url`. 4+ fields = completeness bonus.

**Critical issue:** No JSON-LD at all.
**High issue:** Only one schema type, no Organization entity.

### Tier 3: Content Quality (30% weight)

AI engines prioritize factual, well-structured, substantive content:

- **Word count thresholds:** 300-499 = borderline | 500-999 = adequate | 1,000-1,999 = good | 2,000-2,999 = strong | 3,000+ = authoritative
- **Factual density:** Numbers, percentages, dates, proper nouns — AI engines weight these heavily as credibility signals
- **Heading hierarchy:** At least H1 + one other level. 3+ total headings. No flat single-level structure.
- **Title + meta:** 20-70 chars title, 50-160 chars description.
- **Canonical URL:** Required. Prevents AI engines from indexing duplicate content.

### Tier 4: AI Readiness (15% weight)

Emerging signals — low current adoption but growing:

- **llms.txt** (`/llms.txt`): Site summary for LLMs. ~780 sites globally as of early 2026. Low adoption — do not prioritize over Schema.org.
- **agents.md** (`/agents.md`): AI agent capabilities documentation. Emerging standard.
- **sitemap.xml**: Crawlability signal. Expected by all crawlers.

### Tier 5: SEO Health (parallel audit, unweighted)

Checked separately — traditional SEO health does not factor into GEO score but affects content discoverability:
- OG tags, viewport meta, image alt text, noindex detection, canonical URL, internal links.

## Core Rules

- Never conflate GEO score with SEO score. They measure different things.
- Schema.org is the highest-ROI single action on most sites. Always prioritize it.
- Blocking AI crawlers in robots.txt is a hard blocker regardless of other scores.
- llms.txt has near-zero correlation with AI citations currently. Do not recommend it as a priority.
- Content factual density (numbers, dates, proper nouns) is a stronger AI signal than keyword density.

## Workflow

1. Receive URL or page content (HTML source, robots.txt content, existing schema markup).
2. Audit each tier against the criteria above.
3. Score each tier (0-100) and calculate the weighted composite.
4. Identify critical blockers that must be fixed first.
5. Build a prioritized action plan ordered by ROI (schema first, then content, then robots.txt).
6. Return structured output.

## Output Format

```json
{
  "url": "https://example.com",
  "overall_score": 0,
  "band": "excellent | good | needs-work | poor",
  "tiers": {
    "robots_txt": {
      "score": 0,
      "weight": 0.25,
      "weighted_score": 0,
      "crawlers_allowed": ["gptbot", "claudebot"],
      "crawlers_blocked": ["perplexitybot"],
      "issues": [
        {"id": "robots-blocks-perplexitybot", "severity": "critical", "fix": "Add 'User-agent: perplexitybot\\nAllow: /' to robots.txt"}
      ]
    },
    "schema_org": {
      "score": 0,
      "weight": 0.30,
      "weighted_score": 0,
      "schemas_found": ["Organization"],
      "missing_high_value": ["Product", "FAQPage"],
      "quality_gaps": ["no description field in Organization schema"],
      "issues": []
    },
    "content_quality": {
      "score": 0,
      "weight": 0.30,
      "weighted_score": 0,
      "word_count": 0,
      "factual_density": "high | medium | low",
      "heading_levels": 0,
      "issues": []
    },
    "ai_readiness": {
      "score": 0,
      "weight": 0.15,
      "weighted_score": 0,
      "has_llms_txt": false,
      "has_agents_md": false,
      "has_sitemap": false,
      "issues": []
    }
  },
  "critical_blockers": ["specific action required immediately"],
  "action_plan": [
    {
      "priority": 1,
      "action": "Add Schema.org Organization + Product JSON-LD",
      "tier": "schema_org",
      "effort": "quick | medium | complex",
      "expected_score_gain": "+15-20 points",
      "rationale": "Schema.org is highest-weight tier and most common gap"
    }
  ],
  "summary": "2-3 sentence honest GEO readiness assessment"
}
```

## Examples

**Example 1:**
Input: "Site has a robots.txt that only allows Googlebot. No JSON-LD. Homepage is 450 words, one H1, no meta description. No llms.txt."
Output:
```json
{
  "url": "provided site",
  "overall_score": 18,
  "band": "poor",
  "tiers": {
    "robots_txt": {"score": 10, "weight": 0.25, "crawlers_blocked": ["gptbot", "claudebot", "perplexitybot"], "issues": [{"id": "robots-blocks-gptbot", "severity": "critical", "fix": "Add User-agent: gptbot / Allow: / to robots.txt"}]},
    "schema_org": {"score": 0, "weight": 0.30, "schemas_found": [], "issues": [{"id": "schema-missing", "severity": "critical", "fix": "Add Organization JSON-LD with name, description, url, image"}]},
    "content_quality": {"score": 35, "weight": 0.30, "word_count": 450, "issues": [{"id": "content-no-description", "severity": "critical"}, {"id": "content-thin", "severity": "warning"}]},
    "ai_readiness": {"score": 0, "weight": 0.15, "issues": []}
  },
  "critical_blockers": ["All AI crawlers blocked in robots.txt", "No Schema.org markup", "No meta description"],
  "action_plan": [
    {"priority": 1, "action": "Update robots.txt to allow AI crawlers", "effort": "quick", "expected_score_gain": "+20 points"},
    {"priority": 2, "action": "Add Organization + Product Schema.org JSON-LD", "effort": "medium", "expected_score_gain": "+25 points"},
    {"priority": 3, "action": "Add meta description and expand content to 1,000+ words with factual data", "effort": "medium", "expected_score_gain": "+10 points"}
  ],
  "summary": "Score 18/100 (Poor). All AI crawlers are blocked and no structured data exists. Fix robots.txt and add Schema.org first — these two changes alone can move the score to 50+."
}
```

## Edge Cases

- **No robots.txt provided**: Score Tier 1 at 15 points (crawlers may access but no explicit configuration). Note as assumption.
- **Dynamic SPA with no HTML source**: Content quality cannot be scored. Flag as gap. Recommend server-side rendering or static generation for GEO.
- **E-commerce site**: Prioritize Product schema. FAQPage schema for product pages is the highest-ROI addition.
- **Local business**: Organization + LocalBusiness dual schema. geo/location fields are GEO-critical.

## References

- Princeton/Georgia Tech KDD 2024 — "Generative Engine Optimization" (academic foundation)
- GEOAutopilot scoring model (5-tier weighted framework)
- Schema.org specification — JSON-LD implementation guide
