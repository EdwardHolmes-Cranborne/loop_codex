//! Agent process management: spawn, monitor, and track agent subprocesses.
//!
//! Each agent runs as a separate OS process (or Docker container) with its
//! own clone of the shared Git repository. The parent process communicates
//! with agents via the filesystem (heartbeat files, lock files).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

/// Status of an agent process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentStatus {
    Starting,
    ClaimingTask,
    Working {
        task_id: String,
        branch: String,
    },
    Merging {
        task_id: String,
    },
    ResolvingConflicts {
        task_id: String,
        tier: u8,
    },
    RunningTests,
    Patrolling,
    Paused,
    Failed {
        reason: String,
    },
    Done,
    BudgetExhausted,
}

/// An agent process entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProcess {
    /// Unique identifier (e.g. "agent-1").
    pub id: String,
    /// Optional specialization label (e.g. "parser").
    pub specialization: Option<String>,
    /// Isolated working directory (full clone).
    pub work_dir: PathBuf,
    /// OS process ID (set after spawn).
    pub pid: Option<u32>,
    /// Current status.
    pub status: AgentStatus,
    /// Running cost total (USD).
    pub total_cost_usd: f64,
    /// Number of codex sessions completed.
    pub sessions_completed: u64,
    /// Number of consecutive failures.
    pub consecutive_failures: u32,
    /// When this agent was started.
    pub started_at: DateTime<Utc>,
}

impl AgentProcess {
    /// Create a new agent (not yet spawned).
    pub fn new(id: String, specialization: Option<String>, work_dir: PathBuf) -> Self {
        Self {
            id,
            specialization,
            work_dir,
            pid: None,
            status: AgentStatus::Starting,
            total_cost_usd: 0.0,
            sessions_completed: 0,
            consecutive_failures: 0,
            started_at: Utc::now(),
        }
    }

    /// Record a session cost.
    pub fn record_cost(&mut self, cost_usd: f64) {
        self.total_cost_usd += cost_usd;
        self.sessions_completed += 1;
    }

    /// Record a failure.
    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
    }

    /// Reset the failure counter (after a successful session).
    pub fn reset_failures(&mut self) {
        self.consecutive_failures = 0;
    }

    /// Whether the agent has exceeded the max consecutive failures.
    pub fn too_many_failures(&self, max: u32) -> bool {
        self.consecutive_failures >= max
    }
}

// ---------------------------------------------------------------------------
// Process spawning
// ---------------------------------------------------------------------------

/// Configuration for spawning agent processes.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    /// Path to the codex binary.
    pub codex_binary: PathBuf,
    /// Repository URL to clone.
    pub repo_url: String,
    /// Team branch name.
    pub branch: String,
    /// Base directory for agent working directories.
    pub work_base_dir: PathBuf,
    /// Path to the team spec file.
    pub team_spec_path: PathBuf,
    /// Whether to use Docker containers.
    pub use_docker: bool,
    /// Docker image (if using Docker).
    pub docker_image: String,
    /// Model provider override.
    pub model_provider: Option<String>,
    /// Model override.
    pub model: Option<String>,
    /// Log directory.
    pub log_dir: PathBuf,
}

/// Handle for a spawned agent process.
pub struct AgentHandle {
    /// The agent metadata.
    pub agent: AgentProcess,
    /// The OS child process (None if using Docker or if not yet spawned).
    child: Option<Child>,
}

impl AgentHandle {
    /// Check if the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => child.try_wait().map(|s| s.is_none()).unwrap_or(false),
            None => false,
        }
    }

    /// Wait for the child process to finish, returning exit code.
    pub fn wait(&mut self) -> Option<i32> {
        self.child
            .as_mut()
            .and_then(|child| child.wait().ok())
            .and_then(|status| status.code())
    }

    /// Kill the child process.
    pub fn kill(&mut self) -> std::io::Result<()> {
        if let Some(ref mut child) = self.child {
            child.kill()
        } else {
            Ok(())
        }
    }

    /// Get the OS PID.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|c| c.id())
    }
}

/// Spawn a single agent process.
///
/// The agent is launched as a `codex exec` subprocess with the autonomous
/// loop script. Each agent gets its own clone of the repository.
pub fn spawn_agent(
    agent_id: &str,
    specialization: Option<&str>,
    config: &SpawnConfig,
) -> Result<AgentHandle, AgentSpawnError> {
    let work_dir = config.work_base_dir.join(agent_id);
    let log_file = config.log_dir.join(format!("{agent_id}.log"));

    // Ensure directories exist
    std::fs::create_dir_all(&work_dir)
        .map_err(|e| AgentSpawnError::Io(work_dir.clone(), e))?;
    std::fs::create_dir_all(&config.log_dir)
        .map_err(|e| AgentSpawnError::Io(config.log_dir.clone(), e))?;

    // Clone the repo if work_dir is empty
    if std::fs::read_dir(&work_dir)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true)
    {
        let clone_status = Command::new("git")
            .args([
                "clone",
                "--branch",
                &config.branch,
                &config.repo_url,
                ".",
            ])
            .current_dir(&work_dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| AgentSpawnError::Io(work_dir.clone(), e))?;

        if !clone_status.success() {
            return Err(AgentSpawnError::CloneFailed(agent_id.to_string()));
        }
    }

    // Open log file for stdout/stderr
    let log = std::fs::File::create(&log_file)
        .map_err(|e| AgentSpawnError::Io(log_file.clone(), e))?;
    let log_err = log
        .try_clone()
        .map_err(|e| AgentSpawnError::Io(log_file.clone(), e))?;

    // Build the command
    let mut cmd = if config.use_docker {
        let mut c = Command::new("docker");
        c.args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/workspace", work_dir.display()),
            "-w",
            "/workspace",
            "-e",
            &format!("CODEX_TEAM_AGENT_ID={agent_id}"),
            "-e",
            &format!("CODEX_TEAM_SPEC={}", config.team_spec_path.display()),
        ]);
        if let Some(spec) = specialization {
            c.args(["-e", &format!("CODEX_TEAM_SPECIALIZATION={spec}")]);
        }
        c.arg(&config.docker_image);
        c.args(["codex", "exec"]);
        c
    } else {
        let mut c = Command::new(&config.codex_binary);
        c.args(["exec", "--no-interactive"]);
        c.current_dir(&work_dir);
        c.env("CODEX_TEAM_AGENT_ID", agent_id);
        c.env(
            "CODEX_TEAM_SPEC",
            config.team_spec_path.to_string_lossy().as_ref(),
        );
        if let Some(spec) = specialization {
            c.env("CODEX_TEAM_SPECIALIZATION", spec);
        }
        c
    };

    // Add model overrides
    if let Some(ref provider) = config.model_provider {
        cmd.args(["--provider", provider]);
    }
    if let Some(ref model) = config.model {
        cmd.args(["--model", model]);
    }

    // Spawn
    let child = cmd
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err))
        .spawn()
        .map_err(|e| AgentSpawnError::SpawnFailed(agent_id.to_string(), e))?;

    let pid = child.id();
    let mut agent =
        AgentProcess::new(agent_id.to_string(), specialization.map(String::from), work_dir);
    agent.pid = Some(pid);
    agent.status = AgentStatus::ClaimingTask;

    Ok(AgentHandle {
        agent,
        child: Some(child),
    })
}

/// Spawn an entire team of agents.
pub fn spawn_team(
    count: u32,
    specializations: &[(String, u32)],
    config: &SpawnConfig,
) -> Result<Vec<AgentHandle>, AgentSpawnError> {
    let mut handles = Vec::new();
    let mut agent_num: u32 = 1;

    // Spawn specialized agents first
    for (spec_name, spec_count) in specializations {
        for _ in 0..*spec_count {
            let agent_id = format!("agent-{agent_num}");
            let handle = spawn_agent(&agent_id, Some(spec_name), config)?;
            handles.push(handle);
            agent_num += 1;
        }
    }

    // Spawn remaining unspecialized agents
    while agent_num <= count {
        let agent_id = format!("agent-{agent_num}");
        let handle = spawn_agent(&agent_id, None, config)?;
        handles.push(handle);
        agent_num += 1;
    }

    Ok(handles)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum AgentSpawnError {
    Io(PathBuf, std::io::Error),
    CloneFailed(String),
    SpawnFailed(String, std::io::Error),
}

impl std::fmt::Display for AgentSpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, e) => write!(f, "I/O error at '{}': {e}", path.display()),
            Self::CloneFailed(id) => write!(f, "git clone failed for agent '{id}'"),
            Self::SpawnFailed(id, e) => write!(f, "failed to spawn agent '{id}': {e}"),
        }
    }
}

impl std::error::Error for AgentSpawnError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_agent_defaults() {
        let agent = AgentProcess::new(
            "agent-1".to_string(),
            Some("parser".to_string()),
            PathBuf::from("/tmp/agent-1"),
        );
        assert_eq!(agent.status, AgentStatus::Starting);
        assert_eq!(agent.total_cost_usd, 0.0);
        assert_eq!(agent.sessions_completed, 0);
    }

    #[test]
    fn cost_tracking() {
        let mut agent = AgentProcess::new(
            "agent-1".to_string(),
            None,
            PathBuf::from("/tmp/agent-1"),
        );
        agent.record_cost(5.0);
        agent.record_cost(3.0);
        assert!((agent.total_cost_usd - 8.0).abs() < 0.01);
        assert_eq!(agent.sessions_completed, 2);
    }

    #[test]
    fn failure_tracking() {
        let mut agent = AgentProcess::new(
            "agent-1".to_string(),
            None,
            PathBuf::from("/tmp/agent-1"),
        );
        agent.record_failure();
        agent.record_failure();
        assert!(agent.too_many_failures(2));
        assert!(!agent.too_many_failures(3));
        agent.reset_failures();
        assert!(!agent.too_many_failures(1));
    }

    #[test]
    fn agent_status_serializes() {
        let status = AgentStatus::Working {
            task_id: "task-001".to_string(),
            branch: "agent/agent-1/task-001".to_string(),
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("working"));
        assert!(json.contains("task-001"));

        let deserialized: AgentStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, status);
    }
}
