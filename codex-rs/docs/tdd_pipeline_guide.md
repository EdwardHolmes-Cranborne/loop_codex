# Using the 7-Stage TDD Pipeline with Wiggum Loops

## Overview

The codex-rs agent now exposes three swarm/orchestrator tools the model can call:

| Tool | Purpose |
|---|---|
| `create_subagent` | Register a specialist agent in the LaborMarket |
| `task_dispatch` | Dispatch a task to a registered agent |
| `feature_pipeline` | Initialize a TDD pipeline with 8 automated phases |

---

## Quick Start

### 1. Create specialist agents

```
create_subagent({
  "name": "code-explorer",
  "description": "Explores codebase for patterns and dependencies",
  "system_prompt": "You are a code explorer agent. Analyze files, patterns, and dependencies..."
})
```

### 2. Dispatch tasks to them

```
task_dispatch({
  "agent_name": "code-explorer",
  "task": "Find all authentication-related files and patterns in the project"
})
```

### 3. Run the full TDD pipeline

```
feature_pipeline({
  "implementation_plan": "## Add Authentication\nJWT-based auth for all API endpoints.\n\n## Add Rate Limiting\nToken bucket rate limiter on public endpoints.",
  "max_iterations_per_loop": 25,
  "max_test_fix_cycles": 5
})
```

---

## The 8-Phase Pipeline

Each feature passes through these phases in order:

```
SpecGeneration → TestWrite → TestReview → Implement →
TestRun → FixIssues (cycles) → ReviewCommit → DocUpdate
```

| # | Phase | What happens | Completion signal |
|---|---|---|---|
| 1 | **Spec Generation** | Create feature spec, requirements, implementation plan, subtasks | `SPEC_GENERATION_COMPLETE` |
| 2 | **Test Write** | Write tests before implementation (TDD) | `TEST_WRITING_COMPLETE` |
| 3 | **Test Review** | Review tests for quality and completeness | `REVIEW_PASSED` |
| 4 | **Implement** | Write code to make tests pass | `IMPLEMENTATION_COMPLETE` |
| 5 | **Test Run** | Execute the test suite | exit code (pass/fail) |
| 6 | **Fix Issues** | Fix failing tests (cycles back to Test Run) | `FIX_COMPLETE` |
| 7 | **Review & Commit** | Review changes and create git commit | `REVIEW_APPROVED` |
| 8 | **Doc Update** | Update implementation log with summary and lessons | `DOC_UPDATE_COMPLETE` |

### TDD Loop (Phases 5–6)

Phases 5 and 6 form an iterative loop:
- Run tests → if they fail → fix issues → re-run tests
- This cycles up to `max_test_fix_cycles` times (default: 5)
- If tests still fail after all cycles, the pipeline halts

### Wiggum Loop Pattern

Each phase runs as a **Wiggum Loop** — an autonomous iteration pattern:
1. Send the phase prompt to the agent
2. Check the response for the phase's completion signal
3. If not complete, re-submit with iteration context (e.g., "iteration 3/25")
4. Repeat until the completion signal is found or max iterations are reached
5. Move to the next phase, passing prior outputs as read-only context

---

## Instructing the Model

To use these tools effectively, include instructions like:

> When implementing a feature, use the `feature_pipeline` tool to structure your work.
> Format your implementation plan as markdown with `## Feature Name` headings.
> For each feature, the pipeline will guide you through TDD phases automatically.
> Use `create_subagent` to create specialist agents for exploration, architecture, and review.
> Use `task_dispatch` to delegate subtasks to those agents.

### Example System Prompt Addition

```
You have access to swarm orchestration tools:
- create_subagent: Register specialist agents (explorer, architect, reviewer)
- task_dispatch: Delegate tasks to registered agents
- feature_pipeline: Run the full TDD pipeline

For any feature request:
1. Call feature_pipeline with a structured implementation plan
2. Follow the 8-phase pipeline output
3. Create specialist agents as needed for each phase
4. Use task_dispatch for parallel exploration or review
5. Include completion signals in your responses when phases are done
```
