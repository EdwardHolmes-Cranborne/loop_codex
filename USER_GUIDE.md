# Loop Codex — User Guide

`loop_codex` is a fork of OpenAI Codex with multi-agent orchestration, autonomous iteration loops, TDD pipelines, safety guards, and parallel agent teams.

---

## Installation

```bash
cd codex-rs && cargo build --release
cp target/release/codex ~/.cargo/bin/loop_codex
```

> This does **not** overwrite your upstream `codex` (installed via `brew install codex`).

---

## Using `loop_codex`

### Interactive Mode

```bash
loop_codex                              # Start interactive session
loop_codex "Refactor the auth module"   # Start with a prompt
loop_codex --safe "Fix login bugs"      # Enable safety guard
loop_codex --full-auto "Add tests"      # Auto-approve sandbox commands
```

### Non-Interactive (`exec`)

```bash
loop_codex exec "Add unit tests for parser.rs"
loop_codex exec --full-auto "Refactor config loading"
loop_codex exec --json "Analyze performance"   # JSONL output
echo "Fix lint errors" | loop_codex exec -     # Read from stdin
```

### Code Review

```bash
loop_codex review          # Review current repo changes
loop_codex exec review     # Non-interactive review
```

---

## Provider Flags

```bash
loop_codex --oss                       # LM Studio / Ollama (auto-detect)
loop_codex --local                     # LM Studio at 127.0.0.1:1234
loop_codex --synthetic                 # Synthetic API
loop_codex --openrouter                # OpenRouter API
loop_codex -m o3-mini                  # Specific model
loop_codex --oss --local-provider ollama   # Force Ollama
```

---

## Safety Guard (`--safe`)

Pre-screens every shell command before execution. Blocks dangerous operations and explains why.

```bash
loop_codex --safe "Clean up the project"
```

**What it blocks:**

| Risk | Examples |
|------|----------|
| 🗑️ File Destruction | `rm -rf /`, overwriting critical files |
| 🔓 Permission Escalation | `sudo`, `chmod 777` |
| 🌐 Network Exfiltration | `curl` with env vars |
| ⚙️ System Mutation | Modifying `/etc`, system packages |
| 🔑 Credential Exposure | Printing API keys, `.env` |

---

## Sandbox Modes

```bash
loop_codex -s read-only "Analyze codebase"        # No writes
loop_codex -s workspace-write "Add feature"        # Write to workspace only
loop_codex -s danger-full-access "System update"   # Full access (dangerous)
loop_codex --full-auto "Add tests"                 # Auto-approve + workspace-write
```

---

## Under the Hood: What Makes `loop_codex` Different

When you run `loop_codex`, these systems work together behind the scenes:

### Swarm System

A lead agent can delegate tasks to specialized subagents:

- **TaskDispatch** — delegate focused tasks with isolated context
- **CreateSubagent** — spin up new agents at runtime

Built-in agent types:
- **default** — lead agent with full tools + task delegation
- **coder** — focused implementation agent (no delegation, no agent creation)
- **reviewer** — read-only review agent (no file writes, no shell)

### Wiggum Loop (Autonomous Iteration)

When a task isn't done in one turn, the agent re-submits with context. Iterates up to 25 times until it outputs `WIGGUM_LOOP_COMPLETE`. This is what enables long-running autonomous work.

### Feature Pipeline (TDD)

Multi-phase development driven by tests:

```
Spec → Write Tests → Review Tests → Implement → Run Tests → Fix Issues → Review & Commit → Update Docs
                                                   ↑_______________|
                                                   (cycles until tests pass, max 5)
```

### Scored Code Review

4 parallel reviewers each score findings by confidence (0-100):
- **Guidelines Auditor A+B** — project conventions
- **Bug Detector** — logic errors in changed files
- **History Analyzer** — git context analysis

Findings below threshold (default: 80) are filtered out.

### Feature Dev Workflow

7-phase structured development:
```
Discovery → Exploration → Clarification → Architecture → Implementation → Review → Summary
```

---

## Agent Team System (`loop_codex team`)

Multiple agents working in parallel on shared tasks with git coordination.

### Quick Start

```bash
loop_codex team init -n 3             # Create .codex-team/ config
$EDITOR .codex-team/TASKS.md          # Add tasks
loop_codex team launch --foreground   # Start agents
loop_codex team dashboard             # Watch live TUI
```

### All Team Commands

```bash
# Setup
loop_codex team init -n 3                            # Create config
loop_codex team init -n 5 --yes                      # Skip prompts

# Run
loop_codex team launch                               # Background daemon
loop_codex team launch --foreground                   # Foreground
loop_codex team launch --docker --image codex:latest  # Docker mode

# Monitor
loop_codex team status                # Table view
loop_codex team status --json         # JSON output
loop_codex team status --watch 2      # Auto-refresh
loop_codex team dashboard             # Full TUI (q to quit, ↑↓ select, K kill)

# Task Management
loop_codex team add-task "Refactor auth" -p high -d "Extract JWT logic"
loop_codex team add-task "Add tests" -p medium --depends-on task-001
loop_codex team scale 6               # Change agent count

# Budget
loop_codex team costs                 # Summary
loop_codex team costs --json          # JSON
loop_codex team costs --per-task      # Per-task breakdown

# Logs
loop_codex team log agent-1           # Last 50 lines
loop_codex team log agent-1 -f        # Follow (tail -f)
loop_codex team log agent-1 -n 200    # Last 200 lines

# Lifecycle
loop_codex team stop                  # Graceful shutdown
loop_codex team stop --timeout 30     # Custom timeout
loop_codex team kill                  # Force kill all
loop_codex team kill --agent agent-2  # Kill one agent

# Tests
loop_codex team tests                          # Use spec default
loop_codex team tests --command "cargo test"   # Override
```

### Team Config: `.codex-team/team_spec.yaml`

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
  merge_strategy: rebase       # or "merge"

validation:
  test_command: "cargo test"

budget:
  max_total_usd: 500.0
  max_per_agent_usd: 100.0
  alert_threshold_pct: 80
```

### Task Board: `.codex-team/TASKS.md`

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

### How Agent Teams Work

1. `init` creates `.codex-team/` with config and task board
2. `launch` starts a daemon that spawns agent processes (or Docker containers)
3. Each agent claims a task via file-based locking (no conflicts)
4. Agents work on isolated git branches (`agent/<id>/<task-id>`)
5. Completed work is merged back with 3-tier conflict resolution
6. Patrol mode runs tests, scans for TODOs, fixes quality issues
7. Daemon monitors heartbeats, cleans up stale locks, tracks costs
8. Budget enforcement halts agents when limits are reached
