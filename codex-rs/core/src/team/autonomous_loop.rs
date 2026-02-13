//! Autonomous loop: the core decision loop each agent runs.
//!
//! Implements the full decision tree:
//! sync → find task → claim → work → validate → merge → release → repeat.
//!
//! Each agent runs this loop independently. All coordination happens
//! through the Git repository (lock files, task board, heartbeats).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use super::agent_process::{AgentProcess, AgentStatus};
use super::cost_tracker::{CostEntry, CostTracker};
use super::git_coordinator::GitCoordinator;
use super::task_board::{TaskBoard, TaskStatus, load_task_board, save_task_board};
use crate::tools::sandboxing::{Sandboxable, SandboxablePreference};

/// Sandbox policy for team agent tool execution.
///
/// When running under Docker isolation, we `Require` sandboxing.
/// When running local validation commands (e.g. tests), we `Forbid`
/// sandboxing since the commands need direct filesystem access.
pub(crate) struct TeamSandboxPolicy {
    pub use_docker: bool,
}

impl Sandboxable for TeamSandboxPolicy {
    fn sandbox_preference(&self) -> SandboxablePreference {
        if self.use_docker {
            SandboxablePreference::Require
        } else {
            SandboxablePreference::Forbid
        }
    }

    fn escalate_on_failure(&self) -> bool {
        // Team agents should not escalate; let the daemon handle retries.
        false
    }
}

/// Configuration for the autonomous loop.
pub struct LoopConfig {
    /// Path to the codex binary for running sessions.
    pub codex_binary: PathBuf,
    /// Max time per codex session (minutes).
    pub session_timeout_minutes: u32,
    /// Max consecutive failures before the agent stops.
    pub max_consecutive_failures: u32,
    /// How often to sync upstream (seconds).
    pub pull_interval_seconds: u64,
    /// Path to the task board YAML file.
    pub task_board_path: PathBuf,
    /// Test command (e.g. "cargo test").
    pub test_command: Option<String>,
    /// Test timeout (seconds).
    pub test_timeout_seconds: u32,
    /// Model provider override.
    pub model_provider: Option<String>,
    /// Model override.
    pub model: Option<String>,
    /// Whether to include recent commits in the agent prompt.
    pub include_recent_commits: u32,
    /// Whether to include task board state in the agent prompt.
    pub include_task_board: bool,
    /// Include test output in agent prompt.
    pub include_test_output: bool,
    /// Max test output lines.
    pub max_test_output_lines: u32,
    /// Whether agents are running under Docker isolation.
    pub use_docker: bool,
}

/// Result of running a single codex session.
#[derive(Debug)]
pub struct SessionResult {
    /// Whether the session succeeded.
    pub success: bool,
    /// Exit code of the codex process.
    pub exit_code: Option<i32>,
    /// Estimated cost of this session.
    pub cost_usd: f64,
    /// Session identifier.
    pub session_id: String,
    /// Test output (if tests were run).
    pub test_output: Option<String>,
}

/// Outcome of a single loop iteration.
#[derive(Debug)]
pub enum LoopOutcome {
    /// Successfully completed a task.
    TaskCompleted { task_id: String },
    /// Failed to complete a task (will retry or move on).
    TaskFailed { task_id: String, reason: String },
    /// No tasks available — entered patrol mode.
    Patrolled,
    /// Budget exhausted — agent should stop.
    BudgetExhausted,
    /// Too many consecutive failures — agent should stop.
    TooManyFailures,
    /// All tasks are finished.
    AllDone,
}

/// Run the core autonomous loop for a single agent.
///
/// This is the entry point for each agent process. It loops indefinitely
/// until one of the exit conditions is met:
/// - All tasks done
/// - Budget exhausted
/// - Too many consecutive failures
/// - External kill signal
pub fn run_autonomous_loop(
    agent: &mut AgentProcess,
    git: &GitCoordinator,
    cost_tracker: &CostTracker,
    config: &LoopConfig,
) -> Vec<LoopOutcome> {
    let mut outcomes = Vec::new();

    loop {
        // 1. Check budget
        if let Ok(true) = cost_tracker.is_over_budget() {
            agent.status = AgentStatus::BudgetExhausted;
            outcomes.push(LoopOutcome::BudgetExhausted);
            break;
        }
        if let Ok(true) = cost_tracker.is_agent_over_budget(&agent.id) {
            agent.status = AgentStatus::BudgetExhausted;
            outcomes.push(LoopOutcome::BudgetExhausted);
            break;
        }

        // 2. Check failure count
        if agent.too_many_failures(config.max_consecutive_failures) {
            agent.status = AgentStatus::Failed {
                reason: format!(
                    "exceeded max consecutive failures ({})",
                    config.max_consecutive_failures
                ),
            };
            outcomes.push(LoopOutcome::TooManyFailures);
            break;
        }

        // 3. Sync upstream
        agent.status = AgentStatus::ClaimingTask;
        if let Err(e) = git.sync_upstream() {
            tracing::warn!(agent = %agent.id, "sync failed: {e}");
            // Not fatal — continue with local state
        }

        // 4. Clean up stale locks
        if let Ok(released) = git.cleanup_stale_locks() {
            for task_id in &released {
                tracing::info!(agent = %agent.id, "released stale lock for {task_id}");
            }
        }

        // 5. Load task board
        let mut board = match load_task_board(&config.task_board_path) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(agent = %agent.id, "failed to load task board: {e}");
                agent.record_failure();
                std::thread::sleep(Duration::from_secs(config.pull_interval_seconds));
                continue;
            }
        };

        // 6. Check if all done
        if board.is_finished() {
            agent.status = AgentStatus::Done;
            outcomes.push(LoopOutcome::AllDone);
            break;
        }

        // 7. Find next available task
        let spec = agent.specialization.as_deref();
        let next_task = board.next_available(spec).map(|t| t.id.clone());

        match next_task {
            None => {
                // No tasks available — patrol
                agent.status = AgentStatus::Patrolling;
                let patrol_result =
                    super::patrol::run_patrol(agent, git, config);
                outcomes.push(patrol_result);
                std::thread::sleep(Duration::from_secs(config.pull_interval_seconds));
            }
            Some(task_id) => {
                // 8. Attempt to claim the task
                if let Err(_) = board.claim(&task_id, &agent.id) {
                    // Someone else claimed it — try again
                    continue;
                }
                if let Err(_) = save_task_board(&board, &config.task_board_path) {
                    tracing::warn!(agent = %agent.id, "failed to save task board after claim");
                }

                // 9. Claim via Git (atomic push)
                match git.claim_task(&task_id, &agent.id) {
                    Err(_) => {
                        // Another agent claimed it first — release locally and retry
                        let _ = board.release(&task_id);
                        let _ = save_task_board(&board, &config.task_board_path);
                        continue;
                    }
                    Ok(()) => {}
                }

                // 10. Create feature branch
                let branch_name = match git.create_feature_branch(&agent.id, &task_id) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::error!(agent = %agent.id, "failed to create branch: {e}");
                        let _ = git.release_task(&task_id, &agent.id);
                        agent.record_failure();
                        continue;
                    }
                };

                agent.status = AgentStatus::Working {
                    task_id: task_id.clone(),
                    branch: branch_name.clone(),
                };

                // 11. Build the prompt with context
                let task_entry = board.tasks.iter().find(|t| t.id == task_id);
                let prompt = build_agent_prompt(
                    task_entry.map(|t| t.title.as_str()).unwrap_or(&task_id),
                    task_entry
                        .map(|t| t.description.as_str())
                        .unwrap_or(""),
                    git,
                    &board,
                    config,
                );

                // 12. Run a codex session
                let session = run_codex_session(agent, &prompt, config);

                // 13. Record cost
                let cost_entry = CostEntry {
                    agent_id: agent.id.clone(),
                    session_id: session.session_id.clone(),
                    timestamp: chrono::Utc::now(),
                    input_tokens: 0, // TODO: extract from session metrics
                    output_tokens: 0,
                    cost_usd: session.cost_usd,
                    task_id: task_id.clone(),
                };
                let _ = cost_tracker.record(&cost_entry);
                agent.record_cost(session.cost_usd);

                if !session.success {
                    agent.record_failure();
                    let _ = board.block(&task_id, "session failed");
                    let _ = save_task_board(&board, &config.task_board_path);
                    let _ = git.release_task(&task_id, &agent.id);
                    outcomes.push(LoopOutcome::TaskFailed {
                        task_id: task_id.clone(),
                        reason: "codex session failed".to_string(),
                    });
                    continue;
                }

                // 14. Run tests (if configured)
                agent.status = AgentStatus::RunningTests;
                let test_passed = if let Some(ref test_cmd) = config.test_command {
                    run_test_command(test_cmd, &git.work_dir, config.test_timeout_seconds, config.use_docker)
                } else {
                    true
                };

                if !test_passed {
                    agent.record_failure();
                    let _ = board.block(&task_id, "tests failed after session");
                    let _ = save_task_board(&board, &config.task_board_path);
                    let _ = git.release_task(&task_id, &agent.id);
                    outcomes.push(LoopOutcome::TaskFailed {
                        task_id: task_id.clone(),
                        reason: "tests failed".to_string(),
                    });
                    continue;
                }

                // 15. Merge back to team branch
                agent.status = AgentStatus::Merging {
                    task_id: task_id.clone(),
                };

                match git.merge_feature_branch(&branch_name) {
                    Ok(()) => {
                        // Success! Mark task done
                        agent.reset_failures();
                        let _ = board.complete(&task_id, &format!(
                            "Completed by {} in session {}",
                            agent.id, session.session_id
                        ));
                        let _ = save_task_board(&board, &config.task_board_path);
                        let _ = git.release_task(&task_id, &agent.id);
                        outcomes.push(LoopOutcome::TaskCompleted {
                            task_id: task_id.clone(),
                        });
                    }
                    Err(e) => {
                        // Merge conflict — attempt resolution
                        tracing::warn!(
                            agent = %agent.id,
                            task = %task_id,
                            "merge conflict: {e}"
                        );
                        agent.status = AgentStatus::ResolvingConflicts {
                            task_id: task_id.clone(),
                            tier: 1,
                        };

                        let resolved = super::conflict_resolver::resolve_conflicts(
                            git,
                            &branch_name,
                            config,
                        );

                        if resolved {
                            agent.reset_failures();
                            let _ = board.complete(&task_id, &format!(
                                "Completed by {} (with conflict resolution)",
                                agent.id
                            ));
                            let _ = save_task_board(&board, &config.task_board_path);
                            let _ = git.release_task(&task_id, &agent.id);
                            outcomes.push(LoopOutcome::TaskCompleted {
                                task_id: task_id.clone(),
                            });
                        } else {
                            agent.record_failure();
                            let _ = board.block(&task_id, "merge conflict unresolvable");
                            let _ = save_task_board(&board, &config.task_board_path);
                            let _ = git.release_task(&task_id, &agent.id);
                            outcomes.push(LoopOutcome::TaskFailed {
                                task_id: task_id.clone(),
                                reason: "merge conflict".to_string(),
                            });
                        }
                    }
                }

                // Update heartbeat
                let heartbeat = super::git_coordinator::AgentHeartbeat {
                    agent_id: agent.id.clone(),
                    pid: agent.pid.unwrap_or(std::process::id()),
                    specialization: agent.specialization.clone(),
                    status: format!("{:?}", agent.status),
                    current_task: None,
                    current_branch: None,
                    heartbeat: chrono::Utc::now(),
                    sessions_completed: agent.sessions_completed,
                    total_cost_usd: agent.total_cost_usd,
                    last_commit: None,
                    started_at: agent.started_at,
                    consecutive_failures: agent.consecutive_failures,
                };
                let _ = git.write_heartbeat(&heartbeat);
            }
        }
    }

    outcomes
}

// ---------------------------------------------------------------------------
// Prompt construction
// ---------------------------------------------------------------------------

/// Build the agent's prompt with context engineering (Carlini's tricks).
fn build_agent_prompt(
    task_title: &str,
    task_description: &str,
    git: &GitCoordinator,
    board: &TaskBoard,
    config: &LoopConfig,
) -> String {
    let mut prompt = String::with_capacity(4096);

    prompt.push_str(&format!("# Task: {task_title}\n\n"));

    if !task_description.is_empty() {
        prompt.push_str(&format!("{task_description}\n\n"));
    }

    // Include recent commits by other agents
    if config.include_recent_commits > 0 {
        if let Ok(commits) = git.recent_commits(config.include_recent_commits) {
            if !commits.is_empty() {
                prompt.push_str("## Recent Commits (by other agents)\n\n");
                for commit in &commits {
                    prompt.push_str(&format!("- {commit}\n"));
                }
                prompt.push('\n');
            }
        }
    }

    // Include task board state
    if config.include_task_board {
        prompt.push_str("## Current Task Board\n\n");
        for task in &board.tasks {
            let status_icon = match task.status {
                TaskStatus::Open => "⬚",
                TaskStatus::Claimed => "🔒",
                TaskStatus::InProgress => "🔧",
                TaskStatus::Done => "✅",
                TaskStatus::Blocked => "❌",
            };
            prompt.push_str(&format!(
                "{status_icon} {} ({})\n",
                task.title,
                match &task.claimed_by {
                    Some(agent) => format!("claimed by {agent}"),
                    None => format!("{:?}", task.status),
                }
            ));
        }
        prompt.push('\n');
    }

    // Instructions
    prompt.push_str("## Instructions\n\n");
    prompt.push_str("Complete the task described above. Key rules:\n");
    prompt.push_str("- Make focused, minimal changes for this task only\n");
    prompt.push_str("- Do not modify files unrelated to the task\n");
    prompt.push_str("- Commit your changes with a descriptive message\n");
    prompt.push_str("- Be aware of other agents' recent work to avoid conflicts\n");

    if config.test_command.is_some() {
        prompt.push_str("- Your changes must pass the test suite\n");
    }

    prompt
}

// ---------------------------------------------------------------------------
// Session execution
// ---------------------------------------------------------------------------

/// Run a codex session for the given prompt.
fn run_codex_session(
    agent: &AgentProcess,
    prompt: &str,
    config: &LoopConfig,
) -> SessionResult {
    let session_id = format!(
        "{}-{}-{}",
        agent.id,
        agent.sessions_completed + 1,
        chrono::Utc::now().timestamp()
    );

    let timeout = Duration::from_secs((config.session_timeout_minutes as u64) * 60);
    let start = Instant::now();

    // Write prompt to a temp file
    let prompt_path = agent.work_dir.join(".codex-team-prompt.md");
    if let Err(e) = std::fs::write(&prompt_path, prompt) {
        tracing::error!(agent = %agent.id, "failed to write prompt: {e}");
        return SessionResult {
            success: false,
            exit_code: None,
            cost_usd: 0.0,
            session_id,
            test_output: None,
        };
    }

    let mut cmd = Command::new(&config.codex_binary);
    cmd.args(["exec", "--prompt-from-file"]);
    cmd.arg(&prompt_path);
    cmd.current_dir(&agent.work_dir);

    if let Some(ref provider) = config.model_provider {
        cmd.args(["--provider", provider]);
    }
    if let Some(ref model) = config.model {
        cmd.args(["--model", model]);
    }

    let result = cmd.output();

    // Cleanup prompt file
    let _ = std::fs::remove_file(&prompt_path);

    match result {
        Ok(output) => {
            let elapsed = start.elapsed();
            let timed_out = elapsed >= timeout;
            let success = output.status.success() && !timed_out;

            // Estimate cost (rough: $0.01 per 10 seconds as placeholder)
            let cost_usd = (elapsed.as_secs_f64() / 10.0) * 0.01;

            SessionResult {
                success,
                exit_code: output.status.code(),
                cost_usd,
                session_id,
                test_output: None,
            }
        }
        Err(e) => {
            tracing::error!(agent = %agent.id, "codex exec failed: {e}");
            SessionResult {
                success: false,
                exit_code: None,
                cost_usd: 0.0,
                session_id,
                test_output: None,
            }
        }
    }
}

/// Run the test command and return whether it passed.
fn run_test_command(test_cmd: &str, work_dir: &Path, _timeout_secs: u32, use_docker: bool) -> bool {
    let parts: Vec<&str> = test_cmd.split_whitespace().collect();
    if parts.is_empty() {
        return true;
    }

    // Determine sandbox preference for this command
    let policy = TeamSandboxPolicy { use_docker };
    let pref = policy.sandbox_preference();
    tracing::debug!(
        cmd = test_cmd,
        sandbox = ?pref,
        escalate = policy.escalate_on_failure(),
        "running test command with sandbox preference",
    );

    let result = Command::new(parts[0])
        .args(&parts[1..])
        .current_dir(work_dir)
        .output();

    match result {
        Ok(output) => output.status.success(),
        Err(e) => {
            tracing::warn!("test command failed to run: {e}");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_prompt_includes_task() {
        let git = GitCoordinator::new(
            PathBuf::from("/tmp/test"),
            "origin".to_string(),
            "main".to_string(),
            super::super::team_spec::MergeStrategy::Rebase,
            30,
        );
        let board = TaskBoard::new();
        let config = LoopConfig {
            codex_binary: PathBuf::from("codex"),
            session_timeout_minutes: 30,
            max_consecutive_failures: 5,
            pull_interval_seconds: 60,
            task_board_path: PathBuf::from("/tmp/tasks.yaml"),
            test_command: None,
            model_provider: None,
            model: None,
            include_recent_commits: 0,
            include_task_board: false,
            include_test_output: false,
            max_test_output_lines: 200,
            test_timeout_seconds: 300,
            use_docker: false,
        };

        let prompt = build_agent_prompt("Parse C files", "Implement parser", &git, &board, &config);
        assert!(prompt.contains("Parse C files"));
        assert!(prompt.contains("Implement parser"));
        assert!(prompt.contains("Instructions"));
    }

    #[test]
    fn build_prompt_with_task_board() {
        let git = GitCoordinator::new(
            PathBuf::from("/tmp/test"),
            "origin".to_string(),
            "main".to_string(),
            super::super::team_spec::MergeStrategy::Rebase,
            30,
        );
        let board = super::super::task_board::parse_tasks_markdown(
            "## Task One\nPriority: high\n\n## Task Two\nPriority: low\n",
        );
        let config = LoopConfig {
            codex_binary: PathBuf::from("codex"),
            session_timeout_minutes: 30,
            max_consecutive_failures: 5,
            pull_interval_seconds: 60,
            task_board_path: PathBuf::from("/tmp/tasks.yaml"),
            test_command: Some("cargo test".to_string()),
            model_provider: None,
            model: None,
            include_recent_commits: 0,
            include_task_board: true,
            include_test_output: false,
            max_test_output_lines: 200,
            test_timeout_seconds: 300,
            use_docker: false,
        };

        let prompt = build_agent_prompt("My Task", "", &git, &board, &config);
        assert!(prompt.contains("Task Board"));
        assert!(prompt.contains("Task One"));
        assert!(prompt.contains("Task Two"));
        assert!(prompt.contains("must pass the test suite"));
    }
}
