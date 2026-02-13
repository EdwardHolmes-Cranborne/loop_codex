//! Git-based coordination for the agent team.
//!
//! All inter-agent coordination happens through Git:
//! - Lock files for task claiming (atomic push-or-retry)
//! - Feature branches per task per agent
//! - Heartbeat files for health monitoring
//! - Sync via pull/rebase cycles

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

use super::team_spec::MergeStrategy;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Lock file written to `state/locks/{task_id}.lock`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskLock {
    pub task_id: String,
    pub agent_id: String,
    pub branch: String,
    pub claimed_at: DateTime<Utc>,
    pub timeout_at: DateTime<Utc>,
}

/// Agent heartbeat file written to `state/agents/{agent_id}.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHeartbeat {
    pub agent_id: String,
    pub pid: u32,
    pub specialization: Option<String>,
    pub status: String,
    pub current_task: Option<String>,
    pub current_branch: Option<String>,
    pub heartbeat: DateTime<Utc>,
    pub sessions_completed: u64,
    pub total_cost_usd: f64,
    pub last_commit: Option<String>,
    pub started_at: DateTime<Utc>,
    pub consecutive_failures: u32,
}

/// Coordinator that manages Git operations for a single agent.
pub struct GitCoordinator {
    /// Path to the agent's local clone.
    pub work_dir: PathBuf,
    /// Remote name (e.g. "origin").
    pub remote: String,
    /// Team branch name (e.g. "agent-team/main").
    pub branch: String,
    /// How feature branches are integrated.
    pub merge_strategy: MergeStrategy,
    /// Lock timeout in minutes.
    pub lock_timeout_minutes: u32,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum GitError {
    /// A git command exited with a non-zero status.
    CommandFailed {
        command: String,
        stderr: String,
        code: Option<i32>,
    },
    /// Lock file already exists (another agent claimed the task).
    LockConflict { task_id: String },
    /// I/O error reading/writing coordination files.
    Io(PathBuf, std::io::Error),
    /// JSON parse error.
    Json(PathBuf, serde_json::Error),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CommandFailed {
                command,
                stderr,
                code,
            } => {
                write!(
                    f,
                    "git command failed (exit {}): {command}\n{stderr}",
                    code.map(|c| c.to_string())
                        .unwrap_or_else(|| "?".to_string())
                )
            }
            Self::LockConflict { task_id } => {
                write!(f, "lock conflict: task '{task_id}' already claimed")
            }
            Self::Io(path, e) => write!(f, "I/O error at '{}': {e}", path.display()),
            Self::Json(path, e) => write!(f, "JSON error at '{}': {e}", path.display()),
        }
    }
}

impl std::error::Error for GitError {}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl GitCoordinator {
    /// Create a new coordinator for an agent's working directory.
    pub fn new(
        work_dir: PathBuf,
        remote: String,
        branch: String,
        merge_strategy: MergeStrategy,
        lock_timeout_minutes: u32,
    ) -> Self {
        Self {
            work_dir,
            remote,
            branch,
            merge_strategy,
            lock_timeout_minutes,
        }
    }

    // -- Repository setup --

    /// Clone the repository into the agent's work directory.
    pub fn clone_repo(&self, repo_url: &str) -> Result<(), GitError> {
        run_git(
            &self.work_dir.parent().unwrap_or(Path::new(".")),
            &[
                "clone",
                "--branch",
                &self.branch,
                repo_url,
                &self.work_dir.to_string_lossy(),
            ],
        )?;
        Ok(())
    }

    /// Create the team branch from the current HEAD if it doesn't exist.
    pub fn ensure_team_branch(&self) -> Result<(), GitError> {
        // Check if branch exists locally
        let result = run_git_output(
            &self.work_dir,
            &["rev-parse", "--verify", &self.branch],
        );
        if result.is_err() {
            // Create it
            run_git(&self.work_dir, &["checkout", "-b", &self.branch])?;
            run_git(
                &self.work_dir,
                &["push", "-u", &self.remote, &self.branch],
            )?;
        }
        Ok(())
    }

    // -- Sync --

    /// Pull latest changes from the remote team branch.
    pub fn sync_upstream(&self) -> Result<(), GitError> {
        run_git(&self.work_dir, &["fetch", &self.remote, &self.branch])?;

        match self.merge_strategy {
            MergeStrategy::Rebase => {
                run_git(
                    &self.work_dir,
                    &[
                        "rebase",
                        &format!("{}/{}", self.remote, self.branch),
                    ],
                )?;
            }
            MergeStrategy::Merge | MergeStrategy::Squash => {
                run_git(
                    &self.work_dir,
                    &[
                        "merge",
                        &format!("{}/{}", self.remote, self.branch),
                        "--no-edit",
                    ],
                )?;
            }
        }
        Ok(())
    }

    // -- Task claiming (lock file protocol) --

    /// Attempt to claim a task by creating a lock file and pushing it.
    ///
    /// Returns `Ok(())` if this agent successfully claimed the task.
    /// Returns `Err(GitError::LockConflict)` if another agent got there first.
    pub fn claim_task(&self, task_id: &str, agent_id: &str) -> Result<(), GitError> {
        let lock_dir = self.work_dir.join(".codex-team/state/locks");
        std::fs::create_dir_all(&lock_dir)
            .map_err(|e| GitError::Io(lock_dir.clone(), e))?;

        let lock_path = lock_dir.join(format!("{task_id}.lock"));

        // Check if already locked
        if lock_path.exists() {
            return Err(GitError::LockConflict {
                task_id: task_id.to_string(),
            });
        }

        let now = Utc::now();
        let lock = TaskLock {
            task_id: task_id.to_string(),
            agent_id: agent_id.to_string(),
            branch: format!("agent/{agent_id}/{task_id}"),
            claimed_at: now,
            timeout_at: now + chrono::Duration::minutes(self.lock_timeout_minutes as i64),
        };

        let json = serde_json::to_string_pretty(&lock)
            .map_err(|e| GitError::Json(lock_path.clone(), e))?;
        std::fs::write(&lock_path, json)
            .map_err(|e| GitError::Io(lock_path.clone(), e))?;

        // Stage, commit, and push
        run_git(&self.work_dir, &["add", &lock_path.to_string_lossy()])?;
        run_git(
            &self.work_dir,
            &[
                "commit",
                "-m",
                &format!("claim: {agent_id} claims {task_id}"),
            ],
        )?;

        // Atomic push — if this fails, another agent claimed it first
        let push_result = run_git(
            &self.work_dir,
            &["push", &self.remote, &self.branch],
        );

        if push_result.is_err() {
            // Undo the commit and remove the lock file
            let _ = run_git(&self.work_dir, &["reset", "--hard", "HEAD~1"]);
            let _ = std::fs::remove_file(&lock_path);
            return Err(GitError::LockConflict {
                task_id: task_id.to_string(),
            });
        }

        Ok(())
    }

    /// Release a task lock after completion.
    pub fn release_task(&self, task_id: &str, agent_id: &str) -> Result<(), GitError> {
        let lock_path = self
            .work_dir
            .join(format!(".codex-team/state/locks/{task_id}.lock"));

        if lock_path.exists() {
            std::fs::remove_file(&lock_path)
                .map_err(|e| GitError::Io(lock_path.clone(), e))?;
        }

        // Update progress log
        let progress_dir = self.work_dir.join(".codex-team/state");
        std::fs::create_dir_all(&progress_dir)
            .map_err(|e| GitError::Io(progress_dir.clone(), e))?;

        let progress_path = progress_dir.join("progress.jsonl");
        let entry = serde_json::json!({
            "task_id": task_id,
            "agent_id": agent_id,
            "completed_at": Utc::now().to_rfc3339(),
        });

        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&progress_path)
            .map_err(|e| GitError::Io(progress_path.clone(), e))?;
        writeln!(file, "{}", entry)
            .map_err(|e| GitError::Io(progress_path.clone(), e))?;

        // Stage and commit
        run_git(&self.work_dir, &["add", "-A"])?;
        run_git(
            &self.work_dir,
            &[
                "commit",
                "-m",
                &format!("done: {agent_id} completed {task_id}"),
            ],
        )?;
        run_git(&self.work_dir, &["push", &self.remote, &self.branch])?;

        Ok(())
    }

    // -- Feature branches --

    /// Create a feature branch for working on a task.
    pub fn create_feature_branch(
        &self,
        agent_id: &str,
        task_id: &str,
    ) -> Result<String, GitError> {
        let branch_name = format!("agent/{agent_id}/{task_id}");
        run_git(&self.work_dir, &["checkout", "-b", &branch_name])?;
        Ok(branch_name)
    }

    /// Merge a feature branch back to the team branch.
    pub fn merge_feature_branch(&self, feature_branch: &str) -> Result<(), GitError> {
        run_git(&self.work_dir, &["checkout", &self.branch])?;

        match self.merge_strategy {
            MergeStrategy::Rebase => {
                // Rebase feature onto team branch, then fast-forward merge
                run_git(&self.work_dir, &["checkout", feature_branch])?;
                run_git(
                    &self.work_dir,
                    &["rebase", &self.branch],
                )?;
                run_git(&self.work_dir, &["checkout", &self.branch])?;
                run_git(
                    &self.work_dir,
                    &["merge", "--ff-only", feature_branch],
                )?;
            }
            MergeStrategy::Merge => {
                run_git(
                    &self.work_dir,
                    &["merge", feature_branch, "--no-edit"],
                )?;
            }
            MergeStrategy::Squash => {
                run_git(
                    &self.work_dir,
                    &["merge", "--squash", feature_branch],
                )?;
                run_git(
                    &self.work_dir,
                    &[
                        "commit",
                        "-m",
                        &format!("squash merge: {feature_branch}"),
                    ],
                )?;
            }
        }

        // Push
        run_git(&self.work_dir, &["push", &self.remote, &self.branch])?;

        // Clean up feature branch
        run_git(&self.work_dir, &["branch", "-D", feature_branch])?;

        Ok(())
    }

    // -- Heartbeat --

    /// Write/update the agent heartbeat file.
    pub fn write_heartbeat(&self, heartbeat: &AgentHeartbeat) -> Result<(), GitError> {
        let agents_dir = self.work_dir.join(".codex-team/state/agents");
        std::fs::create_dir_all(&agents_dir)
            .map_err(|e| GitError::Io(agents_dir.clone(), e))?;

        let path = agents_dir.join(format!("{}.json", heartbeat.agent_id));
        let json = serde_json::to_string_pretty(heartbeat)
            .map_err(|e| GitError::Json(path.clone(), e))?;
        std::fs::write(&path, json).map_err(|e| GitError::Io(path.clone(), e))?;

        Ok(())
    }

    /// Read a specific agent's heartbeat.
    pub fn read_heartbeat(&self, agent_id: &str) -> Result<AgentHeartbeat, GitError> {
        let path = self
            .work_dir
            .join(format!(".codex-team/state/agents/{agent_id}.json"));
        let contents =
            std::fs::read_to_string(&path).map_err(|e| GitError::Io(path.clone(), e))?;
        let heartbeat: AgentHeartbeat =
            serde_json::from_str(&contents).map_err(|e| GitError::Json(path.clone(), e))?;
        Ok(heartbeat)
    }

    // -- Stale lock cleanup --

    /// Find and remove lock files whose timeout has expired.
    pub fn cleanup_stale_locks(&self) -> Result<Vec<String>, GitError> {
        let lock_dir = self.work_dir.join(".codex-team/state/locks");
        if !lock_dir.exists() {
            return Ok(vec![]);
        }

        let now = Utc::now();
        let mut released = Vec::new();

        let entries = std::fs::read_dir(&lock_dir)
            .map_err(|e| GitError::Io(lock_dir.clone(), e))?;

        for entry in entries {
            let entry = entry.map_err(|e| GitError::Io(lock_dir.clone(), e))?;
            let path = entry.path();

            if path.extension().and_then(|e| e.to_str()) != Some("lock") {
                continue;
            }

            let contents =
                std::fs::read_to_string(&path).map_err(|e| GitError::Io(path.clone(), e))?;
            let lock: TaskLock =
                serde_json::from_str(&contents).map_err(|e| GitError::Json(path.clone(), e))?;

            if now > lock.timeout_at {
                std::fs::remove_file(&path).map_err(|e| GitError::Io(path.clone(), e))?;
                released.push(lock.task_id);
            }
        }

        Ok(released)
    }

    // -- Recent commits --

    /// Get the last N commits on the team branch (for context injection).
    pub fn recent_commits(&self, count: u32) -> Result<Vec<String>, GitError> {
        let output = run_git_output(
            &self.work_dir,
            &[
                "log",
                &format!("-{count}"),
                "--oneline",
                "--format=%h  %an  %s  %cr",
            ],
        )?;
        Ok(output.lines().map(|l| l.to_string()).collect())
    }

    /// Check if there are merge conflicts in the working tree.
    pub fn has_conflicts(&self) -> bool {
        run_git_output(&self.work_dir, &["diff", "--name-only", "--diff-filter=U"])
            .map(|output| !output.trim().is_empty())
            .unwrap_or(false)
    }

    /// List conflicted files.
    pub fn conflicted_files(&self) -> Result<Vec<String>, GitError> {
        let output =
            run_git_output(&self.work_dir, &["diff", "--name-only", "--diff-filter=U"])?;
        Ok(output
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| l.to_string())
            .collect())
    }
}

// ---------------------------------------------------------------------------
// Git helpers
// ---------------------------------------------------------------------------

/// Run a git command, returning an error if it fails.
fn run_git(cwd: &Path, args: &[&str]) -> Result<(), GitError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| GitError::Io(cwd.to_path_buf(), e))?;

    if !output.status.success() {
        return Err(GitError::CommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            code: output.status.code(),
        });
    }
    Ok(())
}

/// Run a git command and return its stdout.
fn run_git_output(cwd: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| GitError::Io(cwd.to_path_buf(), e))?;

    if !output.status.success() {
        return Err(GitError::CommandFailed {
            command: format!("git {}", args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            code: output.status.code(),
        });
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_coordinator(dir: &Path) -> GitCoordinator {
        GitCoordinator::new(
            dir.to_path_buf(),
            "origin".to_string(),
            "agent-team/main".to_string(),
            MergeStrategy::Rebase,
            30,
        )
    }

    #[test]
    fn lock_file_round_trip() {
        let dir = TempDir::new().unwrap();
        let lock_dir = dir.path().join(".codex-team/state/locks");
        std::fs::create_dir_all(&lock_dir).unwrap();

        let lock = TaskLock {
            task_id: "task-001".to_string(),
            agent_id: "agent-1".to_string(),
            branch: "agent/agent-1/task-001".to_string(),
            claimed_at: Utc::now(),
            timeout_at: Utc::now() + chrono::Duration::minutes(30),
        };

        let path = lock_dir.join("task-001.lock");
        let json = serde_json::to_string_pretty(&lock).unwrap();
        std::fs::write(&path, &json).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let loaded: TaskLock = serde_json::from_str(&contents).unwrap();
        assert_eq!(loaded.task_id, "task-001");
        assert_eq!(loaded.agent_id, "agent-1");
    }

    #[test]
    fn heartbeat_round_trip() {
        let dir = TempDir::new().unwrap();
        // Init a git repo so coordinator methods work
        Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .status()
            .unwrap();

        let coordinator = make_coordinator(dir.path());
        let heartbeat = AgentHeartbeat {
            agent_id: "agent-1".to_string(),
            pid: 42301,
            specialization: Some("parser".to_string()),
            status: "working".to_string(),
            current_task: Some("task-001".to_string()),
            current_branch: Some("agent/agent-1/task-001".to_string()),
            heartbeat: Utc::now(),
            sessions_completed: 14,
            total_cost_usd: 12.50,
            last_commit: Some("9cf8fbe".to_string()),
            started_at: Utc::now(),
            consecutive_failures: 0,
        };

        coordinator.write_heartbeat(&heartbeat).unwrap();
        let loaded = coordinator.read_heartbeat("agent-1").unwrap();
        assert_eq!(loaded.agent_id, "agent-1");
        assert_eq!(loaded.pid, 42301);
        assert_eq!(loaded.specialization.as_deref(), Some("parser"));
    }

    #[test]
    fn stale_lock_cleanup() {
        let dir = TempDir::new().unwrap();
        let lock_dir = dir.path().join(".codex-team/state/locks");
        std::fs::create_dir_all(&lock_dir).unwrap();

        // Write a lock that has already expired
        let lock = TaskLock {
            task_id: "task-stale".to_string(),
            agent_id: "agent-dead".to_string(),
            branch: "agent/agent-dead/task-stale".to_string(),
            claimed_at: Utc::now() - chrono::Duration::hours(2),
            timeout_at: Utc::now() - chrono::Duration::hours(1), // already expired
        };
        let path = lock_dir.join("task-stale.lock");
        std::fs::write(&path, serde_json::to_string(&lock).unwrap()).unwrap();

        let coordinator = make_coordinator(dir.path());
        let released = coordinator.cleanup_stale_locks().unwrap();
        assert_eq!(released, vec!["task-stale"]);
        assert!(!path.exists());
    }
}
