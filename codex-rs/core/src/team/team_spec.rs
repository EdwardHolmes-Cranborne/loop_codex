//! Team specification: parse and validate `team_spec.yaml`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Top-level TeamSpec
// ---------------------------------------------------------------------------

/// Full team specification, typically loaded from `.codex-team/team_spec.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamSpec {
    /// Human-readable team name (e.g. "compiler-build").
    pub team_name: String,

    /// Git configuration for the shared repo.
    #[serde(default)]
    pub git: GitConfig,

    /// Agent fleet configuration.
    #[serde(default)]
    pub agents: AgentTeamConfig,

    /// Where to find the task list.
    #[serde(default)]
    pub tasks: TaskSourceConfig,

    /// Budget caps.
    #[serde(default)]
    pub budget: BudgetConfig,

    /// Post-merge validation settings.
    #[serde(default)]
    pub validation: ValidationConfig,

    /// Context-engineering knobs (inspired by Carlini's tricks).
    #[serde(default)]
    pub context: ContextConfig,
}

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitConfig {
    /// Branch the team works on (created from HEAD if missing).
    #[serde(default = "default_branch")]
    pub branch: String,

    /// Remote name (default: "origin").
    #[serde(default = "default_remote")]
    pub remote: String,

    /// How feature branches are integrated.
    #[serde(default)]
    pub merge_strategy: MergeStrategy,
}

impl Default for GitConfig {
    fn default() -> Self {
        Self {
            branch: default_branch(),
            remote: default_remote(),
            merge_strategy: MergeStrategy::default(),
        }
    }
}

fn default_branch() -> String {
    "agent-team/main".to_string()
}
fn default_remote() -> String {
    "origin".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum MergeStrategy {
    #[default]
    Rebase,
    Merge,
    Squash,
}

// ---------------------------------------------------------------------------
// Agents
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTeamConfig {
    /// Number of parallel agents to spawn.
    #[serde(default = "default_agent_count")]
    pub count: u32,

    /// Time-box each codex session (minutes).
    #[serde(default = "default_session_timeout")]
    pub session_timeout_minutes: u32,

    /// Agent self-terminates after N consecutive failures.
    #[serde(default = "default_max_failures")]
    pub max_consecutive_failures: u32,

    /// How often agents pull upstream changes (seconds).
    #[serde(default = "default_pull_interval")]
    pub pull_interval_seconds: u64,

    /// Stale lock cleanup threshold (minutes).
    #[serde(default = "default_lock_timeout")]
    pub lock_timeout_minutes: u32,

    /// Override model provider from codex config.
    pub model_provider: Option<String>,

    /// Override model from codex config.
    pub model: Option<String>,

    /// Optional agent specializations.
    #[serde(default)]
    pub specializations: Vec<Specialization>,
}

impl Default for AgentTeamConfig {
    fn default() -> Self {
        Self {
            count: default_agent_count(),
            session_timeout_minutes: default_session_timeout(),
            max_consecutive_failures: default_max_failures(),
            pull_interval_seconds: default_pull_interval(),
            lock_timeout_minutes: default_lock_timeout(),
            model_provider: None,
            model: None,
            specializations: Vec::new(),
        }
    }
}

fn default_agent_count() -> u32 {
    8
}
fn default_session_timeout() -> u32 {
    30
}
fn default_max_failures() -> u32 {
    5
}
fn default_pull_interval() -> u64 {
    60
}
fn default_lock_timeout() -> u32 {
    30
}

/// An optional agent specialization (e.g. "parser", "optimizer").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Specialization {
    /// Label for this specialization.
    pub name: String,
    /// How many agents should have this specialization.
    #[serde(default = "default_spec_count")]
    pub count: u32,
    /// Human-readable focus description.
    #[serde(default)]
    pub focus: String,
    /// Extra text appended to the system prompt for this specialization.
    pub system_prompt_append: Option<String>,
}

fn default_spec_count() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Tasks
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSourceConfig {
    /// Path to the task file (Markdown with `## Task` headings).
    #[serde(default = "default_tasks_source")]
    pub source: String,

    /// Whether agents can dynamically create sub-tasks.
    #[serde(default = "default_auto_discover")]
    pub auto_discover: bool,

    /// Limit on sub-tasks an agent can create per parent task.
    #[serde(default = "default_max_subtasks")]
    pub max_subtasks_per_task: u32,
}

impl Default for TaskSourceConfig {
    fn default() -> Self {
        Self {
            source: default_tasks_source(),
            auto_discover: default_auto_discover(),
            max_subtasks_per_task: default_max_subtasks(),
        }
    }
}

fn default_tasks_source() -> String {
    "TASKS.md".to_string()
}
fn default_auto_discover() -> bool {
    true
}
fn default_max_subtasks() -> u32 {
    10
}

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Hard cap on total spending (USD) across all agents.
    #[serde(default = "default_max_total")]
    pub max_total_usd: f64,

    /// Per-agent spending cap (USD).
    #[serde(default = "default_max_per_agent")]
    pub max_per_agent_usd: f64,

    /// Alert when spending reaches this percentage of the cap.
    #[serde(default = "default_alert_pct")]
    pub alert_threshold_pct: u32,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_total_usd: default_max_total(),
            max_per_agent_usd: default_max_per_agent(),
            alert_threshold_pct: default_alert_pct(),
        }
    }
}

fn default_max_total() -> f64 {
    500.0
}
fn default_max_per_agent() -> f64 {
    100.0
}
fn default_alert_pct() -> u32 {
    80
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationConfig {
    /// Command to run after each merge (e.g. "cargo test").
    pub test_command: Option<String>,

    /// Timeout for the test command (seconds).
    #[serde(default = "default_test_timeout")]
    pub test_timeout_seconds: u32,

    /// Oracle command for reference comparison (e.g. "gcc").
    pub oracle_command: Option<String>,

    /// Minimum pass rate to allow a merge (0.0–1.0).
    #[serde(default = "default_pass_rate")]
    pub required_test_pass_rate: f64,
}

fn default_test_timeout() -> u32 {
    300
}
fn default_pass_rate() -> f64 {
    0.95
}

// ---------------------------------------------------------------------------
// Context engineering
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Include recent test output in agent prompts so they can fix failures.
    #[serde(default = "default_true")]
    pub include_test_output: bool,

    /// Max lines of test output to include.
    #[serde(default = "default_test_output_lines")]
    pub max_test_output_lines: u32,

    /// Number of recent commits by other agents to include for awareness.
    #[serde(default = "default_recent_commits")]
    pub include_recent_commits: u32,

    /// Include the current task board state in the agent prompt.
    #[serde(default = "default_true")]
    pub include_task_board: bool,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            include_test_output: true,
            max_test_output_lines: default_test_output_lines(),
            include_recent_commits: default_recent_commits(),
            include_task_board: true,
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_test_output_lines() -> u32 {
    200
}
fn default_recent_commits() -> u32 {
    5
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Errors from team spec operations.
#[derive(Debug)]
pub enum TeamSpecError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    Validation(String),
}

impl std::fmt::Display for TeamSpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read team spec '{}': {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(f, "failed to parse team spec '{}': {source}", path.display())
            }
            Self::Validation(msg) => write!(f, "invalid team spec: {msg}"),
        }
    }
}

impl std::error::Error for TeamSpecError {}

impl TeamSpec {
    /// Create a default TeamSpec with a given agent count.
    pub fn default_with_agents(count: u32) -> Self {
        Self {
            team_name: "my-team".to_string(),
            git: GitConfig::default(),
            agents: AgentTeamConfig {
                count,
                ..AgentTeamConfig::default()
            },
            tasks: TaskSourceConfig::default(),
            budget: BudgetConfig::default(),
            validation: ValidationConfig::default(),
            context: ContextConfig::default(),
        }
    }
}

/// Load a [`TeamSpec`] from a YAML file.
pub fn load_team_spec(path: &Path) -> Result<TeamSpec, TeamSpecError> {
    let contents = std::fs::read_to_string(path).map_err(|e| TeamSpecError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    let spec: TeamSpec =
        serde_yaml::from_str(&contents).map_err(|e| TeamSpecError::Parse {
            path: path.to_path_buf(),
            source: e,
        })?;

    validate_spec(&spec)?;
    Ok(spec)
}

/// Write a [`TeamSpec`] to a YAML file.
pub fn write_team_spec(spec: &TeamSpec, path: &Path) -> Result<(), TeamSpecError> {
    let yaml = serde_yaml::to_string(spec).map_err(|e| TeamSpecError::Parse {
        path: path.to_path_buf(),
        source: e,
    })?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| TeamSpecError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
    }

    std::fs::write(path, yaml).map_err(|e| TeamSpecError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    Ok(())
}

fn validate_spec(spec: &TeamSpec) -> Result<(), TeamSpecError> {
    if spec.team_name.is_empty() {
        return Err(TeamSpecError::Validation(
            "team_name must not be empty".to_string(),
        ));
    }
    if spec.agents.count == 0 {
        return Err(TeamSpecError::Validation(
            "agents.count must be at least 1".to_string(),
        ));
    }
    if spec.budget.max_total_usd <= 0.0 {
        return Err(TeamSpecError::Validation(
            "budget.max_total_usd must be positive".to_string(),
        ));
    }
    if spec.budget.max_per_agent_usd <= 0.0 {
        return Err(TeamSpecError::Validation(
            "budget.max_per_agent_usd must be positive".to_string(),
        ));
    }

    // Check specialization agent counts don't exceed total
    let spec_total: u32 = spec.agents.specializations.iter().map(|s| s.count).sum();
    if spec_total > spec.agents.count {
        return Err(TeamSpecError::Validation(format!(
            "specialization agent counts ({spec_total}) exceed total agent count ({})",
            spec.agents.count
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn parse_minimal_spec() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("team_spec.yaml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "team_name: test-team").unwrap();

        let spec = load_team_spec(&path).unwrap();
        assert_eq!(spec.team_name, "test-team");
        assert_eq!(spec.agents.count, 8); // default
        assert_eq!(spec.git.branch, "agent-team/main");
        assert_eq!(spec.budget.max_total_usd, 500.0);
    }

    #[test]
    fn parse_full_spec() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("team_spec.yaml");
        let yaml = r#"
team_name: compiler-build
git:
  branch: my-branch
  remote: upstream
  merge_strategy: squash
agents:
  count: 4
  session_timeout_minutes: 15
  specializations:
    - name: parser
      count: 2
      focus: "parsing and lexing"
budget:
  max_total_usd: 200
  max_per_agent_usd: 50
validation:
  test_command: "cargo test"
"#;
        std::fs::write(&path, yaml).unwrap();

        let spec = load_team_spec(&path).unwrap();
        assert_eq!(spec.team_name, "compiler-build");
        assert_eq!(spec.git.branch, "my-branch");
        assert_eq!(spec.git.merge_strategy, MergeStrategy::Squash);
        assert_eq!(spec.agents.count, 4);
        assert_eq!(spec.agents.session_timeout_minutes, 15);
        assert_eq!(spec.agents.specializations.len(), 1);
        assert_eq!(spec.agents.specializations[0].name, "parser");
        assert_eq!(spec.budget.max_total_usd, 200.0);
        assert_eq!(
            spec.validation.test_command.as_deref(),
            Some("cargo test")
        );
    }

    #[test]
    fn empty_name_rejected() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("team_spec.yaml");
        std::fs::write(&path, "team_name: \"\"").unwrap();

        let result = load_team_spec(&path);
        assert!(matches!(result, Err(TeamSpecError::Validation(_))));
    }

    #[test]
    fn specialization_overflow_rejected() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("team_spec.yaml");
        let yaml = r#"
team_name: test
agents:
  count: 2
  specializations:
    - name: a
      count: 2
    - name: b
      count: 2
"#;
        std::fs::write(&path, yaml).unwrap();

        let result = load_team_spec(&path);
        assert!(matches!(result, Err(TeamSpecError::Validation(_))));
    }

    #[test]
    fn round_trip_write_read() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("out.yaml");

        let spec = TeamSpec {
            team_name: "round-trip".to_string(),
            git: GitConfig::default(),
            agents: AgentTeamConfig::default(),
            tasks: TaskSourceConfig::default(),
            budget: BudgetConfig::default(),
            validation: ValidationConfig::default(),
            context: ContextConfig::default(),
        };

        write_team_spec(&spec, &path).unwrap();
        let loaded = load_team_spec(&path).unwrap();
        assert_eq!(loaded.team_name, "round-trip");
    }
}
