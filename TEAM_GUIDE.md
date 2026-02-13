# Agent Team System — User Guide

`loop_codex` extends codex with a **multi-agent team** that works in parallel on your codebase. Each agent gets its own git branch, claims tasks from a shared board, and merges results back.

---

## Quick Start

```bash
# 1. Initialize a team in your repo
loop_codex team init -n 3

# 2. Edit the task board
$EDITOR .codex-team/TASKS.md

# 3. Launch
loop_codex team launch --foreground
```

---

## Commands Reference

### `loop_codex team init`

Creates `.codex-team/` directory with default config.

| Flag | Description | Default |
|------|-------------|---------|
| `-n` | Number of agents | 3 |
| `--path` | Repo path | `.` |
| `--yes` | Skip prompts | false |

**Creates:**
```
.codex-team/
├── team_spec.yaml    # Team configuration
├── TASKS.md          # Task board
├── state/            # Runtime state
│   ├── locks/        # Task lock files
│   └── agents/       # Agent heartbeats
├── costs/            # Cost tracking (JSONL)
└── logs/             # Per-agent logs
```

---

### `loop_codex team launch`

Starts the daemon and spawns agents.

| Flag | Description |
|------|-------------|
| `--spec` | Path to `team_spec.yaml` |
| `-n` | Override agent count |
| `--docker` | Run agents in Docker containers |
| `--image` | Docker image name |
| `--provider` | Model provider override |
| `--model` | Model override |
| `--skip-tests` | Don't validate tests first |
| `--foreground` | Run in foreground (Ctrl+C to stop) |
| `--budget` | Max total spend in USD |

**Docker mode:**
```bash
loop_codex team launch --docker --image codex:latest -n 4
```

---

### `loop_codex team status`

Show current agent status.

```bash
loop_codex team status          # Table view
loop_codex team status --json   # JSON output
loop_codex team status --watch 2  # Auto-refresh every 2s
```

---

### `loop_codex team dashboard`

Launch a full interactive TUI dashboard with three panes:

- **Agent Table** — shows each agent's status, current task, sessions, cost, PID
- **Task Board** — task list with status and priority labels
- **Budget Gauge** — visual spend vs. budget limit with stats

```bash
loop_codex team dashboard              # Default 2s refresh
loop_codex team dashboard --refresh 5  # Custom refresh
```

**Keybindings:**
| Key | Action |
|-----|--------|
| `q` | Quit |
| `↑` / `↓` | Select agent |
| `Shift+K` | Kill selected agent |
| `Ctrl+C` | Quit |

---

### `loop_codex team add-task`

Add a task to the board without editing TASKS.md manually.

```bash
loop_codex team add-task "Refactor auth module" \
  -d "Extract JWT logic into separate crate" \
  -p high \
  --specialization parser
```

| Flag | Description | Default |
|------|-------------|---------|
| `-d` | Description | — |
| `-p` | Priority (`high`, `medium`, `low`) | `medium` |
| `--depends-on` | Comma-separated task IDs | — |
| `--specialization` | Required specialization | — |

---

### `loop_codex team scale`

Change the agent count (takes effect on next launch).

```bash
loop_codex team scale 6
```

---

### `loop_codex team costs`

Show cost breakdown.

```bash
loop_codex team costs            # Summary view
loop_codex team costs --json     # JSON output
loop_codex team costs --per-task # Per-task breakdown
```

---

### `loop_codex team log`

View agent logs.

```bash
loop_codex team log agent-1          # Last 50 lines
loop_codex team log agent-1 -n 200   # Last 200 lines
loop_codex team log agent-1 -f       # Follow (tail -f)
```

---

### `loop_codex team tests`

Run the team's test suite.

```bash
loop_codex team tests                    # Uses spec default
loop_codex team tests --command "npm test"  # Override
```

---

### `loop_codex team stop` / `kill`

```bash
loop_codex team stop              # Graceful (60s timeout)
loop_codex team stop --timeout 30 # Custom timeout
loop_codex team kill              # Kill all immediately
loop_codex team kill --agent agent-2  # Kill specific agent
```

---

## Configuration: `team_spec.yaml`

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

---

## Task Board: `TASKS.md`

Tasks are defined in markdown. Each `##` heading is a task:

```markdown
# Team Tasks

## Refactor auth module
Priority: high
Specialization: parser

Extract JWT validation into a standalone crate.

## Add user preferences API
Priority: medium
Depends-on: task-001

Build REST endpoints for user settings.
```

---

## How It Works

1. **Init** creates the `.codex-team/` directory with config and task board
2. **Launch** starts a daemon that spawns agent processes (or Docker containers)
3. Each agent **claims a task** via file-based locking (prevents conflicts)
4. Agents work on **isolated git branches** (`agent/<id>/<task-id>`)
5. When done, agents **merge back** to the team branch with 3-tier conflict resolution
6. A **patrol mode** runs tests, scans for TODOs, and fixes quality issues
7. The daemon monitors **heartbeats**, cleans up **stale locks**, and tracks **costs**
8. **Budget enforcement** stops agents when spending limits are reached
