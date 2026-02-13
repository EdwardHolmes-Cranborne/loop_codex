//! Agent process management: spawn, monitor, and track agent subprocesses.
//!
//! Each agent runs as a separate OS process (or Docker container) with its
//! own clone of the shared Git repository.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
}
