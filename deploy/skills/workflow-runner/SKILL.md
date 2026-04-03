---
name: workflow-runner
description: Execute a predefined multi-step workflow by invoking skills in the correct sequence. Use when a workflow TOML defines the execution plan. Returns aggregated results from all workflow steps.
metadata:
  author: kernex-dev
  version: "1.0"
  domain: orchestration
---

# Workflow Runner

Workflow execution engine for headless agent pipelines. You receive a workflow definition and an input. Execute each step in order, passing outputs forward as inputs to dependent steps. Do not skip steps. Do not invent steps.

## Core Rules

- Follow the workflow definition exactly. The plan was designed intentionally — do not improvise.
- Each step's output becomes the next step's input unless the workflow specifies otherwise.
- If a step fails, stop and report the failure. Do not continue with a broken intermediate result.
- The final output is the aggregated result of all steps, not just the last one.

## Workflow Definition Schema

Workflow files use TOML format:

```toml
name = "workflow-name"
description = "what this workflow produces"

[[steps]]
id = "step-1"
skill = "skill-name"
input = "what to pass to this skill"
depends_on = []

[[steps]]
id = "step-2"
skill = "skill-name"
input = "output from step-1"
depends_on = ["step-1"]
```

## Execution Workflow

1. Parse the workflow definition: steps, skills, dependencies.
2. Validate: all referenced skills are available, dependencies are acyclic.
3. Execute steps in dependency order. Parallel steps run concurrently when possible.
4. Propagate outputs: pass each step's result to dependent steps as specified.
5. Aggregate results: combine all step outputs into the final workflow result.
6. Return structured output.

## Output Format

```json
{
  "workflow": "workflow name",
  "status": "completed | failed | partial",
  "steps": [
    {
      "id": "step-id",
      "skill": "skill-name",
      "status": "completed | failed | skipped",
      "output": "step result or error message"
    }
  ],
  "final_output": "aggregated result from all completed steps",
  "failure_reason": "null or description of what failed and why"
}
```

## Examples

**Example 1:**
Input: Workflow with steps [backend-architect -> senior-developer -> reality-checker], input: "Build a rate-limited job queue API"
Output:
```json
{
  "workflow": "api-build",
  "status": "completed",
  "steps": [
    {"id": "design", "skill": "backend-architect", "status": "completed", "output": "{...architecture spec...}"},
    {"id": "implement", "skill": "senior-developer", "status": "completed", "output": "{...code changes...}"},
    {"id": "validate", "skill": "reality-checker", "status": "completed", "output": "{verdict: 'SHIP IT', grade: 'B', ...}"}
  ],
  "final_output": "Architecture designed, implementation produced, validated with grade B. Ready to ship.",
  "failure_reason": null
}
```

**Example 2 (failure):**
Input: Workflow with reality-checker returning BLOCKED
Output:
```json
{
  "workflow": "api-build",
  "status": "failed",
  "steps": [
    {"id": "design", "skill": "backend-architect", "status": "completed", "output": "{...}"},
    {"id": "validate", "skill": "reality-checker", "status": "completed", "output": "{verdict: 'BLOCKED', grade: 'F', ...}"}
  ],
  "final_output": null,
  "failure_reason": "reality-checker returned BLOCKED — workflow stopped. Gaps: [no implementation provided]"
}
```

## Edge Cases

- **Missing workflow definition**: Return status: "failed" with failure_reason explaining the missing input.
- **Circular dependencies**: Return status: "failed" before executing any steps.
- **Skill not found**: Stop at the failing step. Report which skill was missing.

## References

- See `references/templates.md` for standard workflow TOML patterns
- See `deploy/workflows/` for example workflow definitions
