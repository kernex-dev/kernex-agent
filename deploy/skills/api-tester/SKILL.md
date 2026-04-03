---
name: api-tester
description: Generate API test cases, validate contracts, and surface coverage gaps. Use for REST/GraphQL testing, integration test planning, and response schema validation. Returns structured test suite with assertions.
metadata:
  author: kernex-dev
  version: "1.0"
  domain: task
---

# API Tester

API test suite generation for headless agent workflows. Write tests that actually break things — not happy-path-only scripts. Every test must include at least one failure scenario. Assertions must be specific: check status codes, response shapes, and side effects.

## Core Rules

- Every endpoint gets a failure scenario. A test suite with only 200 OK cases is useless.
- Assertions must be precise. Check exact fields, not just "response is not null."
- Auth must be tested separately from business logic. Mixed tests obscure failure causes.
- Idempotency matters. Mark tests that mutate state and specify teardown requirements.

## Workflow

1. Identify the API surface: endpoints, methods, auth scheme, request/response shapes.
2. Classify endpoints by risk: auth-sensitive, state-mutating, public, admin-only.
3. Generate test cases: happy path, validation errors, auth failures, edge cases, rate limits.
4. Specify assertions: exact status codes, response field checks, side effect verification.
5. Flag coverage gaps: endpoints or scenarios that cannot be tested without additional context.
6. Return structured output.

## Output Format

```json
{
  "api_surface": "brief description of what was analyzed",
  "test_cases": [
    {
      "id": "TC-001",
      "name": "descriptive test name",
      "endpoint": "METHOD /path",
      "auth": "bearer | api-key | none",
      "request": {"headers": {}, "body": {}, "params": {}},
      "expected_status": 200,
      "assertions": ["response.data.id is a UUID", "response.data.created_at is ISO8601"],
      "teardown": "none | description of state to clean up",
      "tags": ["happy-path | error | auth | edge-case | rate-limit"]
    }
  ],
  "coverage_gaps": ["endpoint or scenario not covered and why"],
  "summary": "X test cases covering Y endpoints. Key risk areas: ..."
}
```

## Examples

**Example 1:**
Input: "Generate tests for POST /api/jobs — creates a job, requires bearer auth, returns {id, status: 'queued'}."
Output:
```json
{
  "api_surface": "POST /api/jobs — job creation endpoint with bearer auth",
  "test_cases": [
    {
      "id": "TC-001",
      "name": "creates job with valid payload",
      "endpoint": "POST /api/jobs",
      "auth": "bearer",
      "request": {"headers": {"Authorization": "Bearer <valid_token>"}, "body": {"message": "test task", "provider": "claude-code"}, "params": {}},
      "expected_status": 202,
      "assertions": ["response.id is a UUID", "response.status equals 'queued'"],
      "teardown": "DELETE /api/jobs/{id} or allow expiry",
      "tags": ["happy-path"]
    },
    {
      "id": "TC-002",
      "name": "rejects request without auth token",
      "endpoint": "POST /api/jobs",
      "auth": "none",
      "request": {"headers": {}, "body": {"message": "test"}, "params": {}},
      "expected_status": 401,
      "assertions": ["response body contains error field"],
      "teardown": "none",
      "tags": ["auth", "error"]
    },
    {
      "id": "TC-003",
      "name": "rejects empty message body",
      "endpoint": "POST /api/jobs",
      "auth": "bearer",
      "request": {"headers": {"Authorization": "Bearer <valid_token>"}, "body": {}, "params": {}},
      "expected_status": 422,
      "assertions": ["response body contains validation error for message field"],
      "teardown": "none",
      "tags": ["error", "edge-case"]
    }
  ],
  "coverage_gaps": ["Rate limit behavior not specified — cannot generate rate limit test without threshold"],
  "summary": "3 test cases covering 1 endpoint. Key risk areas: auth bypass, input validation."
}
```

## Edge Cases

- **No schema provided**: Infer from field names and conventions. Document all assumptions in coverage_gaps.
- **GraphQL**: Use operation names as test identifiers. Treat mutations as state-mutating.
- **Webhooks**: Generate both delivery and signature verification test cases.

## References

- See `references/templates.md` for standard API test patterns
