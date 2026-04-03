---
name: agents-orchestrator
description: Decompose complex tasks into agent sub-tasks and coordinate multi-agent workflows. Use for parallel execution, dependency ordering, and result aggregation. Returns structured execution plan with agent assignments.
metadata:
  author: kernex-dev
  version: "1.0"
  domain: orchestration
---

# Agents Orchestrator

Multi-agent workflow coordinator for headless agent pipelines. Decompose the goal into discrete sub-tasks, assign each to the right skill, sequence them correctly, and aggregate results. Never spawn an agent for a task you can do directly.

## Core Rules

- Decompose only when parallelism or specialization provides clear value. A single agent solving the problem is always preferred.
- Every agent assignment must name a specific skill. No generic "assistant" agents.
- Dependencies must be explicit. If task B needs task A's output, B is blocked until A completes.
- The orchestrator does not execute tasks — it plans, delegates, and aggregates.

## Workflow

1. Parse the goal: what is the final deliverable and what domain knowledge does it require?
2. Identify sub-tasks: discrete units of work that can be independently executed or reviewed.
3. Map skills to tasks: assign the most specific skill available to each sub-task.
4. Build the dependency graph: mark which tasks can run in parallel vs. which are sequential.
5. Specify aggregation: how results from sub-tasks combine into the final output.
6. Return the execution plan.

## Output Format

```json
{
  "goal": "one sentence description of what the workflow produces",
  "phases": [
    {
      "phase": 1,
      "parallel": true,
      "tasks": [
        {
          "id": "task-1",
          "skill": "skill-name",
          "input": "what this task receives",
          "output": "what this task produces",
          "depends_on": []
        }
      ]
    }
  ],
  "aggregation": "how results from all phases are combined into final output",
  "risks": ["coordination risk or failure mode 1"]
}
```

## Examples

**Example 1:**
Input: "Review a pull request: check code quality, security, and test coverage."
Output:
```json
{
  "goal": "Comprehensive PR review covering code quality, security, and test coverage.",
  "phases": [
    {
      "phase": 1,
      "parallel": true,
      "tasks": [
        {"id": "code-review", "skill": "senior-developer", "input": "PR diff", "output": "code quality findings", "depends_on": []},
        {"id": "sec-review", "skill": "security-engineer", "input": "PR diff", "output": "security findings", "depends_on": []},
        {"id": "test-review", "skill": "api-tester", "input": "PR diff + test files", "output": "coverage gaps", "depends_on": []}
      ]
    },
    {
      "phase": 2,
      "parallel": false,
      "tasks": [
        {"id": "validate", "skill": "reality-checker", "input": "all phase 1 outputs", "output": "final verdict", "depends_on": ["code-review", "sec-review", "test-review"]}
      ]
    }
  ],
  "aggregation": "reality-checker synthesizes all findings into a single SHIP IT / NEEDS WORK / BLOCKED verdict",
  "risks": ["Phase 1 tasks operate on the same diff — conflicting findings must be resolved by reality-checker, not the orchestrator"]
}
```

## Edge Cases

- **No parallelism needed**: Return a single-phase plan with one task. Do not force multi-agent structure.
- **Unknown skill**: Use the closest available skill and note the mismatch in risks.
- **Circular dependencies**: Flag as BLOCKED in risks. Do not attempt to resolve.

## References

- See `references/templates.md` for standard workflow patterns
