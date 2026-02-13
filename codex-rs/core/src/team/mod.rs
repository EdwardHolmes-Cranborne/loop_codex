//! Agent Team: parallel autonomous agents coordinating via a shared Git repo.
//!
//! Inspired by Carlini's 16-agent compiler build. Each agent independently
//! claims tasks from a shared task board using lock files, works in isolated
//! branches, and pushes results back. No orchestration agent — coordination
//! happens entirely through Git.

pub mod agent_process;
pub mod autonomous_loop;
pub mod conflict_resolver;
pub mod cost_tracker;
pub mod git_coordinator;
pub mod patrol;
pub mod task_board;
pub mod team_daemon;
pub mod team_spec;

pub use agent_process::{AgentProcess, AgentStatus};
pub use cost_tracker::{CostEntry, CostTracker};
pub use git_coordinator::GitCoordinator;
pub use task_board::{Priority, TaskBoard, TaskEntry, TaskStatus};
pub use team_spec::{MergeStrategy, TeamSpec};
