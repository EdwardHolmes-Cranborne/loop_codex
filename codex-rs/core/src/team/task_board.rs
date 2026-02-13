//! Task board: parse TASKS.md, manage task state, serialize to YAML.
//!
//! The task board is the shared state that all agents read to find work.
//! It is stored as `state/tasks.yaml` in the `.codex-team/` directory
//! and is updated by agents as they claim, complete, or block tasks.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Priority of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

impl Default for Priority {
    fn default() -> Self {
        Self::Medium
    }
}

/// Status of a task on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Open,
    Claimed,
    InProgress,
    Done,
    Blocked,
}

impl Default for TaskStatus {
    fn default() -> Self {
        Self::Open
    }
}

/// A single task entry on the board.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEntry {
    /// Unique ID (e.g. "task-001").
    pub id: String,
    /// Human-readable title.
    pub title: String,
    /// Detailed description.
    #[serde(default)]
    pub description: String,
    /// Priority level.
    #[serde(default)]
    pub priority: Priority,
    /// Current status.
    #[serde(default)]
    pub status: TaskStatus,
    /// Agent that has claimed this task (if any).
    #[serde(default)]
    pub claimed_by: Option<String>,
    /// IDs of tasks this depends on.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Tags for specialization matching.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Child subtask IDs.
    #[serde(default)]
    pub subtasks: Vec<String>,
    /// When the task was completed.
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    /// Summary of how the task was completed.
    #[serde(default)]
    pub completed_summary: Option<String>,
}

/// The full task board.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskBoard {
    pub tasks: Vec<TaskEntry>,
}

// ---------------------------------------------------------------------------
// CRUD operations
// ---------------------------------------------------------------------------

impl TaskBoard {
    /// Create an empty board.
    pub fn new() -> Self {
        Self { tasks: Vec::new() }
    }

    /// Find the next available task, optionally filtered by specialization tags.
    ///
    /// Returns the highest-priority open task whose dependencies are all done
    /// and whose tags overlap with the specialization (if provided).
    pub fn next_available(&self, specialization: Option<&str>) -> Option<&TaskEntry> {
        let done_ids: std::collections::HashSet<&str> = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .map(|t| t.id.as_str())
            .collect();

        let mut candidates: Vec<&TaskEntry> = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Open)
            .filter(|t| t.depends_on.iter().all(|dep| done_ids.contains(dep.as_str())))
            .filter(|t| {
                match specialization {
                    Some(spec) => {
                        // Prefer tasks matching specialization, but allow any if none match
                        t.tags.is_empty()
                            || t.tags.iter().any(|tag| {
                                tag.eq_ignore_ascii_case(spec)
                            })
                    }
                    None => true,
                }
            })
            .collect();

        // Sort by priority (Critical > High > Medium > Low)
        candidates.sort_by_key(|t| t.priority);
        candidates.first().copied()
    }

    /// Claim a task for a given agent.
    pub fn claim(&mut self, task_id: &str, agent_id: &str) -> Result<(), TaskBoardError> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| TaskBoardError::NotFound(task_id.to_string()))?;

        if task.status != TaskStatus::Open {
            return Err(TaskBoardError::AlreadyClaimed {
                task_id: task_id.to_string(),
                by: task.claimed_by.clone().unwrap_or_default(),
            });
        }

        task.status = TaskStatus::Claimed;
        task.claimed_by = Some(agent_id.to_string());
        Ok(())
    }

    /// Mark a task as in-progress.
    pub fn start(&mut self, task_id: &str) -> Result<(), TaskBoardError> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| TaskBoardError::NotFound(task_id.to_string()))?;

        task.status = TaskStatus::InProgress;
        Ok(())
    }

    /// Mark a task as completed with a summary.
    pub fn complete(&mut self, task_id: &str, summary: &str) -> Result<(), TaskBoardError> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| TaskBoardError::NotFound(task_id.to_string()))?;

        task.status = TaskStatus::Done;
        task.completed_at = Some(Utc::now());
        task.completed_summary = Some(summary.to_string());
        Ok(())
    }

    /// Mark a task as blocked with a reason.
    pub fn block(&mut self, task_id: &str, reason: &str) -> Result<(), TaskBoardError> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| TaskBoardError::NotFound(task_id.to_string()))?;

        task.status = TaskStatus::Blocked;
        task.completed_summary = Some(format!("BLOCKED: {reason}"));
        Ok(())
    }

    /// Release a claimed task back to open (e.g. agent died).
    pub fn release(&mut self, task_id: &str) -> Result<(), TaskBoardError> {
        let task = self
            .tasks
            .iter_mut()
            .find(|t| t.id == task_id)
            .ok_or_else(|| TaskBoardError::NotFound(task_id.to_string()))?;

        task.status = TaskStatus::Open;
        task.claimed_by = None;
        Ok(())
    }

    /// Add a new sub-task under a parent.
    pub fn add_subtask(
        &mut self,
        parent_id: &str,
        subtask: TaskEntry,
    ) -> Result<(), TaskBoardError> {
        let subtask_id = subtask.id.clone();

        // Find parent
        let parent = self
            .tasks
            .iter_mut()
            .find(|t| t.id == parent_id)
            .ok_or_else(|| TaskBoardError::NotFound(parent_id.to_string()))?;

        parent.subtasks.push(subtask_id);

        // Add the subtask to the board
        self.tasks.push(subtask);
        Ok(())
    }

    /// Count tasks by status.
    pub fn count_by_status(&self) -> TaskCounts {
        let mut counts = TaskCounts::default();
        for task in &self.tasks {
            match task.status {
                TaskStatus::Open => counts.open += 1,
                TaskStatus::Claimed => counts.claimed += 1,
                TaskStatus::InProgress => counts.in_progress += 1,
                TaskStatus::Done => counts.done += 1,
                TaskStatus::Blocked => counts.blocked += 1,
            }
        }
        counts
    }

    /// Whether all tasks are either done or blocked.
    pub fn is_finished(&self) -> bool {
        self.tasks
            .iter()
            .all(|t| t.status == TaskStatus::Done || t.status == TaskStatus::Blocked)
    }
}

#[derive(Debug, Default)]
pub struct TaskCounts {
    pub open: usize,
    pub claimed: usize,
    pub in_progress: usize,
    pub done: usize,
    pub blocked: usize,
}

// ---------------------------------------------------------------------------
// Markdown parsing
// ---------------------------------------------------------------------------

/// Parse a TASKS.md file into a [`TaskBoard`].
///
/// Expected format: `## Title` headings for each task, with optional
/// `Priority:`, `Tags:`, `Depends:`, and `Description:` fields.
pub fn parse_tasks_markdown(content: &str) -> TaskBoard {
    let mut tasks = Vec::new();
    let mut current_title: Option<String> = None;
    let mut current_priority = Priority::default();
    let mut current_tags: Vec<String> = Vec::new();
    let mut current_depends: Vec<String> = Vec::new();
    let mut current_desc_lines: Vec<String> = Vec::new();
    let mut task_counter: u32 = 0;

    let flush = |title: &str,
                 priority: Priority,
                 tags: Vec<String>,
                 depends: Vec<String>,
                 desc_lines: Vec<String>,
                 counter: u32|
     -> TaskEntry {
        TaskEntry {
            id: format!("task-{counter:03}"),
            title: title.to_string(),
            description: desc_lines.join("\n").trim().to_string(),
            priority,
            status: TaskStatus::Open,
            claimed_by: None,
            depends_on: depends,
            tags,
            subtasks: Vec::new(),
            completed_at: None,
            completed_summary: None,
        }
    };

    for line in content.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            // Flush previous task
            if let Some(ref title) = current_title {
                task_counter += 1;
                tasks.push(flush(
                    title,
                    current_priority,
                    std::mem::take(&mut current_tags),
                    std::mem::take(&mut current_depends),
                    std::mem::take(&mut current_desc_lines),
                    task_counter,
                ));
            }
            current_title = Some(heading.trim().to_string());
            current_priority = Priority::default();
            current_tags = Vec::new();
            current_depends = Vec::new();
            current_desc_lines = Vec::new();
        } else if current_title.is_some() {
            let trimmed = line.trim();
            if let Some(val) = trimmed.strip_prefix("Priority:") {
                current_priority = match val.trim().to_lowercase().as_str() {
                    "critical" => Priority::Critical,
                    "high" => Priority::High,
                    "medium" => Priority::Medium,
                    "low" => Priority::Low,
                    _ => Priority::Medium,
                };
            } else if let Some(val) = trimmed.strip_prefix("Tags:") {
                current_tags = val.split(',').map(|s| s.trim().to_string()).collect();
            } else if let Some(val) = trimmed.strip_prefix("Depends:") {
                let dep = val.trim();
                if !dep.is_empty() && dep != "none" {
                    current_depends = dep.split(',').map(|s| s.trim().to_string()).collect();
                }
            } else if let Some(val) = trimmed.strip_prefix("Description:") {
                current_desc_lines.push(val.trim().to_string());
            } else if !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && !trimmed.starts_with("Priority:")
                && !trimmed.starts_with("Tags:")
                && !trimmed.starts_with("Depends:")
            {
                current_desc_lines.push(trimmed.to_string());
            }
        }
    }

    // Flush last task
    if let Some(ref title) = current_title {
        task_counter += 1;
        tasks.push(flush(
            title,
            current_priority,
            current_tags,
            current_depends,
            current_desc_lines,
            task_counter,
        ));
    }

    TaskBoard { tasks }
}

// ---------------------------------------------------------------------------
// Persistence (YAML)
// ---------------------------------------------------------------------------

/// Load a task board from a YAML file.
pub fn load_task_board(path: &Path) -> Result<TaskBoard, TaskBoardError> {
    let contents =
        std::fs::read_to_string(path).map_err(|e| TaskBoardError::Io(path.to_path_buf(), e))?;
    let board: TaskBoard = serde_yaml::from_str(&contents)
        .map_err(|e| TaskBoardError::Parse(path.to_path_buf(), e))?;
    Ok(board)
}

/// Save a task board to a YAML file.
pub fn save_task_board(board: &TaskBoard, path: &Path) -> Result<(), TaskBoardError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| TaskBoardError::Io(path.to_path_buf(), e))?;
    }
    let yaml = serde_yaml::to_string(board)
        .map_err(|e| TaskBoardError::Parse(path.to_path_buf(), e))?;
    std::fs::write(path, yaml).map_err(|e| TaskBoardError::Io(path.to_path_buf(), e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum TaskBoardError {
    NotFound(String),
    AlreadyClaimed { task_id: String, by: String },
    Io(PathBuf, std::io::Error),
    Parse(PathBuf, serde_yaml::Error),
}

impl std::fmt::Display for TaskBoardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "task '{id}' not found"),
            Self::AlreadyClaimed { task_id, by } => {
                write!(f, "task '{task_id}' already claimed by {by}")
            }
            Self::Io(path, e) => write!(f, "task board I/O error at '{}': {e}", path.display()),
            Self::Parse(path, e) => {
                write!(f, "task board parse error at '{}': {e}", path.display())
            }
        }
    }
}

impl std::error::Error for TaskBoardError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_board() -> TaskBoard {
        TaskBoard {
            tasks: vec![
                TaskEntry {
                    id: "task-001".to_string(),
                    title: "Implement Parser".to_string(),
                    description: "Build the C parser".to_string(),
                    priority: Priority::High,
                    status: TaskStatus::Open,
                    claimed_by: None,
                    depends_on: vec![],
                    tags: vec!["parser".to_string()],
                    subtasks: vec![],
                    completed_at: None,
                    completed_summary: None,
                },
                TaskEntry {
                    id: "task-002".to_string(),
                    title: "Implement Type Checker".to_string(),
                    description: "Build the type checker".to_string(),
                    priority: Priority::High,
                    status: TaskStatus::Open,
                    claimed_by: None,
                    depends_on: vec!["task-001".to_string()],
                    tags: vec!["frontend".to_string()],
                    subtasks: vec![],
                    completed_at: None,
                    completed_summary: None,
                },
                TaskEntry {
                    id: "task-003".to_string(),
                    title: "Write Tests".to_string(),
                    description: "Test infrastructure".to_string(),
                    priority: Priority::Medium,
                    status: TaskStatus::Open,
                    claimed_by: None,
                    depends_on: vec![],
                    tags: vec!["tester".to_string()],
                    subtasks: vec![],
                    completed_at: None,
                    completed_summary: None,
                },
            ],
        }
    }

    #[test]
    fn next_available_respects_priority() {
        let board = sample_board();
        let next = board.next_available(None).unwrap();
        assert_eq!(next.priority, Priority::High);
    }

    #[test]
    fn next_available_respects_dependencies() {
        let board = sample_board();
        // task-002 depends on task-001 which is not done, so it shouldn't be returned
        // when we filter for "frontend" specialization
        let next = board.next_available(Some("frontend"));
        // task-002 has deps not met, so nothing with "frontend" tag is available
        // but tasks with empty tags could match — task-001 has "parser" tag, task-003 has "tester"
        assert!(next.is_none() || next.unwrap().id != "task-002");
    }

    #[test]
    fn claim_and_complete() {
        let mut board = sample_board();

        board.claim("task-001", "agent-1").unwrap();
        assert_eq!(board.tasks[0].status, TaskStatus::Claimed);
        assert_eq!(board.tasks[0].claimed_by.as_deref(), Some("agent-1"));

        board.complete("task-001", "Done well").unwrap();
        assert_eq!(board.tasks[0].status, TaskStatus::Done);
        assert!(board.tasks[0].completed_at.is_some());
    }

    #[test]
    fn double_claim_rejected() {
        let mut board = sample_board();
        board.claim("task-001", "agent-1").unwrap();
        let result = board.claim("task-001", "agent-2");
        assert!(matches!(result, Err(TaskBoardError::AlreadyClaimed { .. })));
    }

    #[test]
    fn release_reopens_task() {
        let mut board = sample_board();
        board.claim("task-001", "agent-1").unwrap();
        board.release("task-001").unwrap();
        assert_eq!(board.tasks[0].status, TaskStatus::Open);
        assert!(board.tasks[0].claimed_by.is_none());
    }

    #[test]
    fn parse_markdown() {
        let md = r#"# Agent Team Tasks

## Implement Parser
Priority: high
Tags: parser, frontend
Depends: none
Description: Build a full C11 parser.

## Implement Optimizer
Priority: medium
Tags: optimizer
Depends: Implement Parser
Description: Implement optimization passes.

## Write Tests
Priority: low
Tags: tester
"#;
        let board = parse_tasks_markdown(md);
        assert_eq!(board.tasks.len(), 3);
        assert_eq!(board.tasks[0].title, "Implement Parser");
        assert_eq!(board.tasks[0].priority, Priority::High);
        assert_eq!(board.tasks[0].tags, vec!["parser", "frontend"]);
        assert_eq!(board.tasks[1].depends_on, vec!["Implement Parser"]);
        assert_eq!(board.tasks[2].priority, Priority::Low);
    }

    #[test]
    fn count_by_status() {
        let mut board = sample_board();
        board.claim("task-001", "agent-1").unwrap();
        board.complete("task-001", "done").unwrap();

        let counts = board.count_by_status();
        assert_eq!(counts.done, 1);
        assert_eq!(counts.open, 2);
    }

    #[test]
    fn is_finished() {
        let mut board = TaskBoard {
            tasks: vec![TaskEntry {
                id: "t1".to_string(),
                title: "Only task".to_string(),
                description: String::new(),
                priority: Priority::Medium,
                status: TaskStatus::Open,
                claimed_by: None,
                depends_on: vec![],
                tags: vec![],
                subtasks: vec![],
                completed_at: None,
                completed_summary: None,
            }],
        };
        assert!(!board.is_finished());
        board.complete("t1", "done").unwrap();
        assert!(board.is_finished());
    }

    #[test]
    fn yaml_round_trip() {
        let board = sample_board();
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("tasks.yaml");

        save_task_board(&board, &path).unwrap();
        let loaded = load_task_board(&path).unwrap();
        assert_eq!(loaded.tasks.len(), 3);
        assert_eq!(loaded.tasks[0].title, "Implement Parser");
    }
}
