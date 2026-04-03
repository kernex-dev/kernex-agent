---
name: geo-schema-generator
description: Generate production-ready Schema.org JSON-LD markup for GEO visibility. Detects entity type, builds complete schema, scores quality. Use for sites missing structured data or with thin schema.
metadata:
  author: kernex-dev
  version: "1.0"
  domain: task
---

# GEO Schema Generator

Generate complete, deployment-ready Schema.org JSON-LD for AI engine visibility. Schema.org is the 30%-weight tier in the GEO scoring model and consistently the highest-ROI single action on sites with no structured data. Generate specific markup — not examples. Every output is copy-paste deployable.

## Core Rules

- Never generate incomplete schema. A partial Organization without `name` and `description` is worse than nothing — it signals low-quality entity data to AI engines.
- Output real values based on the provided context. Do not use placeholder text if the context provides the actual data.
- Every schema requires at minimum: `@context`, `@type`, `name`, `description`, `url`.
- Image fields must use absolute URLs. Relative paths are invalid in JSON-LD.
- Multiple schema types in one page `<head>` outperforms a single bloated schema.

## Entity Type Detection

Map the site type to the correct primary schema:

| Site Type | Primary Schema | Secondary Schema |
|-----------|----------------|-----------------|
| SaaS / Software product | SoftwareApplication | Organization |
| E-commerce product | Product | Organization |
| Blog / Publication | BlogPosting or NewsArticle | WebSite + Organization |
| Service business | Service | LocalBusiness or Organization |
| Personal site / Portfolio | Person | WebSite |
| Local business | LocalBusiness | Organization |
| Knowledge base / Docs | TechArticle | WebSite + Organization |
| FAQ page | FAQPage | WebPage |
| How-to guide | HowTo | WebPage |
| Any site | WebSite | Organization (always add both) |

## Quality Scoring

AI engines score schema quality by field completeness:
- `@type` (required, 5 pts)
- High-value type match (15 pts)
- `name` (10 pts)
- `description` (10 pts)
- `image` (5 pts)
- `url` (5 pts)
- 4+ total fields completeness bonus (5 pts)
- 2+ schema types on page (10 pts)
- Organization + content type combo (10 pts)

Target: 75+ quality score per generated schema.

## Workflow

1. Identify entity type from provided site description, URL, or content.
2. Select primary schema type + recommended secondary types.
3. Extract all available data from the provided context to populate fields.
4. Fill required fields. Flag optional fields that are missing but recommended.
5. Generate the complete JSON-LD with the `<script>` wrapper for head placement.
6. Score the output quality.
7. Provide deployment instructions and missing data requirements.
8. Return structured output.

## Output Format

```json
{
  "entity_type": "SoftwareApplication | Organization | Product | etc",
  "schemas_generated": ["SoftwareApplication", "Organization"],
  "quality_score": 85,
  "artifacts": [
    {
      "type": "SoftwareApplication",
      "quality_score": 90,
      "json_ld": "<script type=\"application/ld+json\">\n{...complete schema...}\n</script>",
      "placement": "Inside <head> tag on every page | Homepage only | Product pages only",
      "fields_populated": ["@type", "@context", "name", "description", "url", "applicationCategory"],
      "fields_missing": ["screenshot", "offers.price"],
      "fields_missing_impact": "high | medium | low"
    }
  ],
  "deployment_instructions": "Paste inside <head> before </head>. For Next.js use next/head or metadata API. For WordPress use a header plugin or theme functions.php.",
  "missing_data_required": ["field: what you need to provide"],
  "geo_score_impact": "+20-25 GEO score points (estimated)"
}
```

## Schema Templates

### Organization (always include on every site)
```json
{
  "@context": "https://schema.org",
  "@type": "Organization",
  "name": "[Company Name]",
  "description": "[1-2 sentence description of what the company does]",
  "url": "https://example.com",
  "logo": "https://example.com/logo.png",
  "sameAs": [
    "https://twitter.com/handle",
    "https://linkedin.com/company/name",
    "https://github.com/org"
  ],
  "contactPoint": {
    "@type": "ContactPoint",
    "contactType": "customer service",
    "email": "hello@example.com"
  }
}
```

### SoftwareApplication (SaaS, CLI tools, dev tools)
```json
{
  "@context": "https://schema.org",
  "@type": "SoftwareApplication",
  "name": "[Product Name]",
  "description": "[What it does and who it's for]",
  "url": "https://example.com",
  "applicationCategory": "DeveloperApplication",
  "operatingSystem": "macOS, Linux, Windows",
  "offers": {
    "@type": "Offer",
    "price": "0",
    "priceCurrency": "USD"
  },
  "author": {
    "@type": "Organization",
    "name": "[Company Name]"
  },
  "softwareVersion": "[version]",
  "releaseNotes": "https://example.com/changelog",
  "screenshot": "https://example.com/screenshot.png"
}
```

### FAQPage (highest ROI for AI Overview visibility)
```json
{
  "@context": "https://schema.org",
  "@type": "FAQPage",
  "mainEntity": [
    {
      "@type": "Question",
      "name": "[Question text exactly as users ask it]",
      "acceptedAnswer": {
        "@type": "Answer",
        "text": "[Complete answer — 2-4 sentences. AI engines pull this verbatim for featured snippets.]"
      }
    }
  ]
}
```

### Product (e-commerce)
```json
{
  "@context": "https://schema.org",
  "@type": "Product",
  "name": "[Product Name]",
  "description": "[Product description]",
  "image": "https://example.com/product.jpg",
  "brand": {"@type": "Brand", "name": "[Brand]"},
  "offers": {
    "@type": "Offer",
    "price": "[price]",
    "priceCurrency": "USD",
    "availability": "https://schema.org/InStock",
    "url": "https://example.com/product"
  },
  "aggregateRating": {
    "@type": "AggregateRating",
    "ratingValue": "4.8",
    "reviewCount": "127"
  }
}
```

## Examples

**Example 1:**
Input: "kx is an open source CLI AI assistant for developers. MIT license. Free. Supports 11 AI providers. Built in Rust. GitHub: github.com/kernex-dev/kernex-agent."
Output:
```json
{
  "entity_type": "SoftwareApplication",
  "schemas_generated": ["SoftwareApplication", "Organization"],
  "quality_score": 82,
  "artifacts": [
    {
      "type": "SoftwareApplication",
      "quality_score": 85,
      "json_ld": "<script type=\"application/ld+json\">\n{\n  \"@context\": \"https://schema.org\",\n  \"@type\": \"SoftwareApplication\",\n  \"name\": \"kx\",\n  \"description\": \"Open source CLI AI assistant for developers. Supports 11 AI providers including Claude, GPT-4, and Gemini. Built in Rust.\",\n  \"url\": \"https://github.com/kernex-dev/kernex-agent\",\n  \"applicationCategory\": \"DeveloperApplication\",\n  \"operatingSystem\": \"macOS, Linux, Windows\",\n  \"offers\": {\"@type\": \"Offer\", \"price\": \"0\", \"priceCurrency\": \"USD\"},\n  \"author\": {\"@type\": \"Organization\", \"name\": \"kernex-dev\"},\n  \"license\": \"https://opensource.org/licenses/MIT\",\n  \"codeRepository\": \"https://github.com/kernex-dev/kernex-agent\",\n  \"programmingLanguage\": \"Rust\"\n}\n</script>",
      "placement": "Inside <head> on homepage and README-linked landing page",
      "fields_populated": ["@type", "name", "description", "url", "applicationCategory", "operatingSystem", "offers", "author", "license"],
      "fields_missing": ["screenshot", "softwareVersion", "releaseNotes"],
      "fields_missing_impact": "medium"
    }
  ],
  "deployment_instructions": "For a GitHub repo: add to the project's documentation site or landing page <head>. GitHub itself does not render custom JSON-LD.",
  "missing_data_required": ["screenshot: URL to a demo screenshot or GIF", "softwareVersion: current release version"],
  "geo_score_impact": "+22-28 GEO score points (estimated)"
}
```

## Edge Cases

- **No URL or site description provided**: Ask for entity type and core facts (name, description, what it does, who it's for). Do not generate placeholder schemas.
- **Existing schema to improve**: Audit the provided schema against quality scoring, add missing fields, add secondary types.
- **Dynamic pages (e.g., product catalog)**: Generate the template with placeholders for dynamic fields. Note which fields require CMS/database population.
- **Multi-language site**: Generate in primary language only. Note that `inLanguage` field should be added per locale variant.

## References

- Schema.org specification — https://schema.org
- Google Rich Results guidance — structured data types eligible for rich results
- GEOAutopilot scoring model — quality field weights and scoring system
