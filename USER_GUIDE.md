# Loop Codex — User Guide

`loop_codex` is a fork of OpenAI Codex with added multi-agent orchestration, autonomous development loops, structured code review, and safety features.

---

## Installation

```bash
# Build and install
cd codex-rs && cargo build --release
cp target/release/codex ~/.cargo/bin/loop_codex
```

> **Note:** This installs as `loop_codex` so it doesn't conflict with `brew install codex`.

---

## 1. Swarm System

A multi-agent swarm where a lead agent delegates tasks to specialized subagents.

### Agent Specs (YAML)

Agents are defined in YAML files with inheritance:

```yaml
# default.yaml — the lead agent
name: default-agent
description: Default Codex agent with swarm dispatch capabilities
system_prompt: |
  You are Codex, a powerful AI coding assistant. You have access to a team
  of specialized subagents you can delegate tasks to.
tools:
  - shell
  - file_read
  - file_write
  - task_dispatch
  - create_subagent
subagents:
  - name: coder
    path: coder.yaml
    description: Focused coding agent for implementation tasks
  - name: reviewer
    path: reviewer.yaml
    description: Code review agent for quality assessment
```

```yaml
# coder.yaml — inherits from default, stripped-down for focused work
extends: default.yaml
name: coder
excluded_tools:
  - task_dispatch
  - create_subagent
subagents: []
```

```yaml
# reviewer.yaml — read-only agent for code review
extends: default.yaml
name: reviewer
excluded_tools:
  - task_dispatch
  - create_subagent
  - file_write
  - shell
subagents: []
```

### Key Concepts

| Component | What it does |
|-----------|-------------|
| **LaborMarket** | Registry of available agents (fixed specs + dynamic runtime agents) |
| **TaskDispatch** | Tool for delegating tasks to subagents with isolated context |
| **CreateSubagent** | Tool for creating new specialized agents at runtime |
| **AgentSpec** | YAML-based agent definitions with inheritance (`extends:`) |

---

## 2. Wiggum Loop (Autonomous Iteration)

Iterative development loop: submits a prompt, checks output, re-submits until done.

### How it works

1. Submits your task as a prompt
2. Waits for the agent to complete
3. Checks if the output contains the **completion promise** (`WIGGUM_LOOP_COMPLETE`)
4. If not done → re-submits with iteration context
5. Continues up to **max_iterations** (default: 25)

### Configuration

```json
{
  "prompt": "Refactor the auth module to use JWT",
  "max_iterations": 25,
  "completion_promise": "WIGGUM_LOOP_COMPLETE"
}
```

---

## 3. Feature Development Pipeline

TDD-driven multi-phase pipeline. Each feature passes through 8 gated phases, each running as its own WiggumLoop:

```
SpecGeneration → TestWrite → TestReview → Implement → TestRun → FixIssues → ReviewCommit → DocUpdate
                                                        ↑__________________|
                                                         (cycles until tests pass)
```

### Phases

| Phase | Purpose |
|-------|---------|
| **SpecGeneration** | Parse requirements, create implementation plan |
| **TestWrite** | Write tests first (TDD) |
| **TestReview** | Review tests for quality |
| **Implement** | Build the feature |
| **TestRun** | Execute test suite |
| **FixIssues** | Fix failures (cycles with TestRun, max 5 cycles) |
| **ReviewCommit** | Review + auto-commit |
| **DocUpdate** | Update global implementation log |

### Configuration

```json
{
  "max_iterations_per_loop": 25,
  "test_fix_max_cycles": 5,
  "auto_commit": true,
  "docs_base_dir": ".codex-docs",
  "safety_guard_enabled": false
}
```

---

## 4. Feature Dev Task (Structured Workflow)

A 7-phase structured workflow using specialized subagents per phase:

```
Discovery → Exploration → Clarification → Architecture → Implementation → Review → Summary
```

Each phase has a dedicated agent role, prompt template, and success criteria.

---

## 5. Scored Code Review

Spawns **4 parallel reviewer sub-agents**, each focusing on a different aspect:

| Reviewer | Focus |
|----------|-------|
| **GuidelinesAuditorA** | Project conventions (AGENTS.md, CLAUDE.md) |
| **GuidelinesAuditorB** | Cross-validation of guidelines |
| **BugDetector** | Bug detection in changed files |
| **HistoryAnalyzer** | Git history context analysis |

Each finding includes a **confidence score (0–100)**. Findings below the threshold (default: 80) are filtered out.

```json
{
  "confidence_threshold": 80,
  "review_model": null,
  "post_comments": false
}
```

---

## 6. Safety Guard

Pre-screens all shell commands before execution. Enabled via `--safe` flag.

### Risk Categories

| Category | Examples |
|----------|----------|
| 🗑️ **File Destruction** | `rm -rf /`, overwriting critical files |
| 🔓 **Permission Escalation** | `sudo`, `chmod 777` |
| 🌐 **Network Exfiltration** | `curl` with env vars, data uploads |
| ⚙️ **System Mutation** | Modifying `/etc`, system packages |
| 🔑 **Credential Exposure** | Printing API keys, `.env` contents |

Uses a hardcoded blocklist + optional LLM-based analysis. Blocked commands return explanations.

---

## 7. Agent Team System

Multi-agent team that works in parallel on your codebase. Each agent gets its own git branch, claims tasks from a shared board, and merges results back.

### Quick Start

```bash
loop_codex team init -n 3
$EDITOR .codex-team/TASKS.md
loop_codex team launch --foreground
```

### Commands

| Command | Description |
|---------|-------------|
| `team init -n 3` | Create `.codex-team/` with config and task board |
| `team launch` | Start daemon and spawn agents |
| `team status` | Show agent table (`--json`, `--watch 2`) |
| `team dashboard` | Interactive TUI with 3 panes |
| `team add-task "..." -p high` | Add task to board |
| `team scale 6` | Change agent count |
| `team costs` | Budget breakdown (`--json`, `--per-task`) |
| `team log agent-1 -f` | Follow agent logs |
| `team tests` | Run test suite |
| `team stop` | Graceful shutdown |
| `team kill` | Force kill all (or `--agent agent-2`) |

### Docker Mode

```bash
loop_codex team launch --docker --image codex:latest -n 4
```

Agents run in isolated Docker containers with volume mounts for the workspace.

### Dashboard Keybindings

| Key | Action |
|-----|--------|
| `q` / `Ctrl+C` | Quit |
| `↑` / `↓` | Select agent |
| `Shift+K` | Kill selected agent |

### Configuration: `team_spec.yaml`

```yaml
agents:
  count: 3
  model_provider: openai
  model: o3-mini
  specializations:
    - name: parser
      count: 1
    - name: frontend
      count: 1

git:
  branch: main
  remote: origin
  merge_strategy: rebase

validation:
  test_command: "cargo test"

budget:
  max_total_usd: 500.0
  max_per_agent_usd: 100.0
  alert_threshold_pct: 80
```

### Task Board: `TASKS.md`

```markdown
## Refactor auth module
Priority: high
Specialization: parser

Extract JWT validation into a standalone crate.

## Add user preferences API
Priority: medium
Depends-on: task-001

Build REST endpoints for user settings.
```

### How It Works

1. **Init** creates `.codex-team/` with config and task board
2. **Launch** starts a daemon that spawns agent processes (or Docker containers)
3. Each agent **claims a task** via file-based locking
4. Agents work on **isolated git branches** (`agent/<id>/<task-id>`)
5. Completed work is **merged back** with 3-tier conflict resolution
6. **Patrol mode** runs tests, scans for TODOs, fixes quality issues
7. The daemon monitors **heartbeats**, cleans up **stale locks**, tracks **costs**
8. **Budget enforcement** halts agents when limits are reached

---

## Architecture Overview

```
loop_codex
├── Swarm System (core/src/swarm/)
│   ├── LaborMarket      — agent registry
│   ├── AgentSpec         — YAML spec loading + inheritance
│   ├── TaskDispatch      — task delegation tool
│   └── CreateSubagent    — dynamic agent creation
│
├── Task System (core/src/tasks/)
│   ├── WiggumLoop        — autonomous iteration loop
│   ├── Pipeline           — 8-phase TDD orchestrator
│   ├── FeatureDev         — 7-phase structured workflow
│   ├── ScoredReview       — 4-agent parallel code review
│   ├── SafetyGuard        — command pre-screening
│   ├── GhostSnapshot      — point-in-time codebase snapshots
│   ├── LoopContext        — per-phase context management
│   └── GlobalLog          — cross-feature implementation log
│
└── Team System (core/src/team/)
    ├── TeamSpec           — YAML team configuration
    ├── TaskBoard          — TASKS.md management
    ├── AgentProcess       — OS/Docker process spawning
    ├── AutonomousLoop     — agent decision loop
    ├── GitCoordinator     — branch/lock/merge management
    ├── CostTracker        — JSONL budget tracking
    ├── ConflictResolver   — 3-tier merge conflict resolution
    ├── Patrol             — test/scan/fix monitoring
    └── TeamDaemon         — background daemon + heartbeats
```
