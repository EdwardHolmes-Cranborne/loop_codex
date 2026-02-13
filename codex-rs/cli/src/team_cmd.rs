//! CLI subcommand tree for `codex team`.
//!
//! Provides init, launch, status, stop, add-task, scale, costs, log,
//! tests, kill, and dashboard subcommands.

use clap::{Args, Parser};
use std::path::PathBuf;

/// Manage a parallel agent team.
#[derive(Debug, Parser)]
pub struct TeamCli {
    #[clap(subcommand)]
    pub sub: TeamSubcommand,
}

#[derive(Debug, clap::Subcommand)]
pub enum TeamSubcommand {
    /// Initialize a new agent team in the current repo.
    Init(TeamInitArgs),

    /// Launch the agent team (starts daemon + agents).
    Launch(TeamLaunchArgs),

    /// Show status of all running agents and tasks.
    Status(TeamStatusArgs),

    /// Stop the team gracefully (finish current tasks, then exit).
    Stop(TeamStopArgs),

    /// Immediately kill all agent processes.
    Kill(TeamKillArgs),

    /// Add a task to the team task board.
    #[clap(name = "add-task")]
    AddTask(TeamAddTaskArgs),

    /// Scale the number of agents up or down.
    Scale(TeamScaleArgs),

    /// Show cost breakdown across all agents.
    Costs(TeamCostsArgs),

    /// Show log output for a specific agent.
    Log(TeamLogArgs),

    /// Run the team's test suite.
    Tests(TeamTestsArgs),

    /// Launch interactive TUI dashboard.
    Dashboard(TeamDashboardArgs),
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

/// Initialize a `.codex-team/` directory with default config.
#[derive(Debug, Args)]
pub struct TeamInitArgs {
    /// Number of agents to configure (default: 3).
    #[clap(short = 'n', long, default_value = "3")]
    pub agents: u32,

    /// Path to the repository (default: current directory).
    #[clap(short, long)]
    pub path: Option<PathBuf>,

    /// Skip interactive prompts, use all defaults.
    #[clap(long)]
    pub yes: bool,
}

// ---------------------------------------------------------------------------
// Launch
// ---------------------------------------------------------------------------

/// Start the team daemon and spawn agent processes.
#[derive(Debug, Args)]
pub struct TeamLaunchArgs {
    /// Path to team_spec.yaml (default: .codex-team/team_spec.yaml).
    #[clap(short, long)]
    pub spec: Option<PathBuf>,

    /// Number of agents to spawn (overrides spec).
    #[clap(short = 'n', long)]
    pub agents: Option<u32>,

    /// Disable Docker isolation (agents run as local processes instead).
    #[clap(long)]
    pub no_docker: bool,

    /// Docker image to use (default from spec).
    #[clap(long)]
    pub image: Option<String>,

    /// Model provider override.
    #[clap(long)]
    pub provider: Option<String>,

    /// Model override.
    #[clap(long)]
    pub model: Option<String>,

    /// Use Synthetic API provider.
    #[clap(long)]
    pub synthetic: bool,

    /// Use OpenRouter API provider.
    #[clap(long)]
    pub openrouter: bool,

    /// Use LM Studio at 127.0.0.1:1234.
    #[clap(long)]
    pub local: bool,

    /// Use local open source model provider (LM Studio or Ollama).
    #[clap(long)]
    pub oss: bool,

    /// Skip initial test validation.
    #[clap(long)]
    pub skip_tests: bool,

    /// Run in foreground (don't daemonize).
    #[clap(long)]
    pub foreground: bool,

    /// Maximum total budget in USD.
    #[clap(long)]
    pub budget: Option<f64>,

    /// Disable GUI mode (don't open Terminal.app windows for agents).
    #[clap(long)]
    pub no_gui: bool,
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

/// Show current team status.
#[derive(Debug, Args)]
pub struct TeamStatusArgs {
    /// Output as JSON.
    #[clap(long)]
    pub json: bool,

    /// Watch mode: refresh every N seconds.
    #[clap(long)]
    pub watch: Option<u64>,
}

// ---------------------------------------------------------------------------
// Stop / Kill
// ---------------------------------------------------------------------------

/// Gracefully stop the team.
#[derive(Debug, Args)]
pub struct TeamStopArgs {
    /// Timeout in seconds before force-killing (default: 60).
    #[clap(long, default_value = "60")]
    pub timeout: u64,
}

/// Immediately kill all agents.
#[derive(Debug, Args)]
pub struct TeamKillArgs {
    /// Kill a specific agent by ID.
    #[clap(long)]
    pub agent: Option<String>,
}

// ---------------------------------------------------------------------------
// Add Task
// ---------------------------------------------------------------------------

/// Add a new task to the task board.
#[derive(Debug, Args)]
pub struct TeamAddTaskArgs {
    /// Task title.
    pub title: String,

    /// Task description.
    #[clap(short, long)]
    pub description: Option<String>,

    /// Priority (high, medium, low).
    #[clap(short, long, default_value = "medium")]
    pub priority: String,

    /// Task dependencies (comma-separated task IDs).
    #[clap(long)]
    pub depends_on: Option<String>,

    /// Specialization required for this task.
    #[clap(long)]
    pub specialization: Option<String>,
}

// ---------------------------------------------------------------------------
// Scale
// ---------------------------------------------------------------------------

/// Scale the number of agents.
#[derive(Debug, Args)]
pub struct TeamScaleArgs {
    /// Target number of agents.
    pub count: u32,
}

// ---------------------------------------------------------------------------
// Costs
// ---------------------------------------------------------------------------

/// Show cost breakdown.
#[derive(Debug, Args)]
pub struct TeamCostsArgs {
    /// Output as JSON.
    #[clap(long)]
    pub json: bool,

    /// Show per-task breakdown.
    #[clap(long)]
    pub per_task: bool,
}

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

/// Show agent log output.
#[derive(Debug, Args)]
pub struct TeamLogArgs {
    /// Agent ID (e.g. "agent-1").
    pub agent_id: String,

    /// Number of lines to show (tail).
    #[clap(short = 'n', long, default_value = "50")]
    pub lines: u32,

    /// Follow log output (like tail -f).
    #[clap(short, long)]
    pub follow: bool,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Run the team's test suite.
#[derive(Debug, Args)]
pub struct TeamTestsArgs {
    /// Test command override.
    #[clap(long)]
    pub command: Option<String>,
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

/// Launch interactive TUI dashboard.
#[derive(Debug, Args)]
pub struct TeamDashboardArgs {
    /// Refresh interval in seconds.
    #[clap(long, default_value = "2")]
    pub refresh: u64,
}

// ---------------------------------------------------------------------------
// Command handlers
// ---------------------------------------------------------------------------

/// Entry point for `codex team <subcommand>`.
pub async fn run_team_command(cli: TeamCli) -> anyhow::Result<()> {
    match cli.sub {
        TeamSubcommand::Init(args) => cmd_init(args).await,
        TeamSubcommand::Launch(args) => cmd_launch(args).await,
        TeamSubcommand::Status(args) => cmd_status(args).await,
        TeamSubcommand::Stop(args) => cmd_stop(args).await,
        TeamSubcommand::Kill(args) => cmd_kill(args).await,
        TeamSubcommand::AddTask(args) => cmd_add_task(args).await,
        TeamSubcommand::Scale(args) => cmd_scale(args).await,
        TeamSubcommand::Costs(args) => cmd_costs(args).await,
        TeamSubcommand::Log(args) => cmd_log(args).await,
        TeamSubcommand::Tests(args) => cmd_tests(args).await,
        TeamSubcommand::Dashboard(args) => cmd_dashboard(args).await,
    }
}

// ---------------------------------------------------------------------------
// Init
// ---------------------------------------------------------------------------

async fn cmd_init(args: TeamInitArgs) -> anyhow::Result<()> {
    let root = args.path.unwrap_or_else(|| PathBuf::from("."));
    let team_dir = root.join(".codex-team");
    std::fs::create_dir_all(&team_dir)?;

    // Create default team_spec.yaml
    let spec = codex_core::team::TeamSpec::default_with_agents(args.agents);
    let spec_path = team_dir.join("team_spec.yaml");
    codex_core::team::team_spec::write_team_spec(&spec, &spec_path)?;

    // Create TASKS.md template
    let tasks_path = team_dir.join("TASKS.md");
    if !tasks_path.exists() {
        std::fs::write(
            &tasks_path,
            "# Team Tasks\n\n## Example Task\nPriority: medium\n\nDescribe what needs to be done here.\n",
        )?;
    }

    // Create state directories
    std::fs::create_dir_all(team_dir.join("state/locks"))?;
    std::fs::create_dir_all(team_dir.join("state/agents"))?;
    std::fs::create_dir_all(team_dir.join("costs"))?;
    std::fs::create_dir_all(team_dir.join("logs"))?;

    println!("✅ Initialized agent team in {}", team_dir.display());
    println!("   Agents: {}", args.agents);
    println!("   Spec:   {}", spec_path.display());
    println!("   Tasks:  {}", tasks_path.display());
    println!("\nEdit TASKS.md to add your tasks, then run: codex team launch");

    Ok(())
}

// ---------------------------------------------------------------------------
// Launch
// ---------------------------------------------------------------------------

async fn cmd_launch(args: TeamLaunchArgs) -> anyhow::Result<()> {
    let spec_path = args
        .spec
        .unwrap_or_else(|| PathBuf::from(".codex-team/team_spec.yaml"));

    let spec = codex_core::team::team_spec::load_team_spec(&spec_path)?;

    let agent_count = args.agents.unwrap_or(spec.agents.count);

    // Docker is default; --no-docker disables it
    let use_docker = !args.no_docker;
    // GUI is default on macOS; --no-gui disables it
    let gui_mode = !args.no_gui;

    // Map provider convenience flags
    let provider_flag = if args.synthetic {
        Some("--synthetic".to_string())
    } else if args.openrouter {
        Some("--openrouter".to_string())
    } else if args.local {
        Some("--local".to_string())
    } else if args.oss {
        Some("--oss".to_string())
    } else {
        None
    };

    let mode = if use_docker { "Docker" } else { "local" };
    let gui_label = if gui_mode { " (GUI)" } else { "" };
    println!("🚀 Launching team with {agent_count} agents ({mode}{gui_label})...");

    if args.foreground {
        println!("   Running in foreground (Ctrl+C to stop)");
    } else {
        println!("   Daemon PID will be written to .codex-team/state/daemon.pid");
    }

    // Run the daemon inline (foreground mode for now)
    codex_core::team::team_daemon::run_daemon(
        spec,
        agent_count,
        use_docker,
        args.foreground,
        gui_mode,
        provider_flag,
    )
    .await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Status
// ---------------------------------------------------------------------------

async fn cmd_status(args: TeamStatusArgs) -> anyhow::Result<()> {
    let state_dir = PathBuf::from(".codex-team/state");
    let agents_dir = state_dir.join("agents");

    if !agents_dir.exists() {
        println!("No team is currently running.");
        return Ok(());
    }

    // Read all agent heartbeat files
    let mut heartbeats = Vec::new();
    for entry in std::fs::read_dir(&agents_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let content = std::fs::read_to_string(&path)?;
            let hb: codex_core::team::git_coordinator::AgentHeartbeat =
                serde_json::from_str(&content)?;
            heartbeats.push(hb);
        }
    }

    if args.json {
        println!("{}", serde_json::to_string_pretty(&heartbeats)?);
    } else {
        println!("🤖 Agent Team Status ({} agents)\n", heartbeats.len());
        println!(
            "{:<12} {:<12} {:<18} {:<10} {:<10}",
            "AGENT", "STATUS", "TASK", "SESSIONS", "COST"
        );
        println!("{}", "-".repeat(62));
        for hb in &heartbeats {
            println!(
                "{:<12} {:<12} {:<18} {:<10} ${:<9.2}",
                hb.agent_id,
                hb.status,
                hb.current_task.as_deref().unwrap_or("-"),
                hb.sessions_completed,
                hb.total_cost_usd,
            );
        }

        // Show cost summary
        let total_cost: f64 = heartbeats.iter().map(|h| h.total_cost_usd).sum();
        let total_sessions: u64 = heartbeats.iter().map(|h| h.sessions_completed).sum();
        println!("{}", "-".repeat(62));
        println!(
            "{:<12} {:<12} {:<18} {:<10} ${:<9.2}",
            "TOTAL", "", "", total_sessions, total_cost
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Stop / Kill
// ---------------------------------------------------------------------------

async fn cmd_stop(args: TeamStopArgs) -> anyhow::Result<()> {
    let pid_path = PathBuf::from(".codex-team/state/daemon.pid");
    if !pid_path.exists() {
        println!("No team daemon is running.");
        return Ok(());
    }

    let pid_str = std::fs::read_to_string(&pid_path)?;
    let pid: i32 = pid_str.trim().parse()?;

    println!("Sending SIGTERM to daemon (PID {pid})...");
    unsafe {
        libc::kill(pid, libc::SIGTERM);
    }

    // Wait for graceful shutdown
    println!(
        "Waiting up to {}s for agents to finish current tasks...",
        args.timeout
    );
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < args.timeout {
        if !pid_path.exists() {
            println!("✅ Team stopped gracefully.");
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    println!("⚠️  Timeout reached, force-killing...");
    unsafe {
        libc::kill(pid, libc::SIGKILL);
    }
    let _ = std::fs::remove_file(&pid_path);
    println!("✅ Team killed.");
    Ok(())
}

async fn cmd_kill(args: TeamKillArgs) -> anyhow::Result<()> {
    if let Some(agent_id) = args.agent {
        // Kill specific agent
        let path = PathBuf::from(format!(
            ".codex-team/state/agents/{agent_id}.json"
        ));
        if !path.exists() {
            anyhow::bail!("Agent '{agent_id}' not found.");
        }
        let content = std::fs::read_to_string(&path)?;
        let hb: codex_core::team::git_coordinator::AgentHeartbeat =
            serde_json::from_str(&content)?;
        println!("Killing agent {} (PID {})...", agent_id, hb.pid);
        unsafe {
            libc::kill(hb.pid as i32, libc::SIGKILL);
        }
        let _ = std::fs::remove_file(&path);
        println!("✅ Agent {agent_id} killed.");
    } else {
        // Kill all
        let pid_path = PathBuf::from(".codex-team/state/daemon.pid");
        if pid_path.exists() {
            let pid_str = std::fs::read_to_string(&pid_path)?;
            let pid: i32 = pid_str.trim().parse()?;
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
            let _ = std::fs::remove_file(&pid_path);
        }
        // Kill all agent processes
        let agents_dir = PathBuf::from(".codex-team/state/agents");
        if agents_dir.exists() {
            for entry in std::fs::read_dir(&agents_dir)? {
                let entry = entry?;
                let content = std::fs::read_to_string(entry.path())?;
                if let Ok(hb) = serde_json::from_str::<codex_core::team::git_coordinator::AgentHeartbeat>(&content) {
                    unsafe {
                        libc::kill(hb.pid as i32, libc::SIGKILL);
                    }
                }
                let _ = std::fs::remove_file(entry.path());
            }
        }
        println!("✅ All agents killed.");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Add Task
// ---------------------------------------------------------------------------

async fn cmd_add_task(args: TeamAddTaskArgs) -> anyhow::Result<()> {
    let tasks_path = PathBuf::from(".codex-team/TASKS.md");
    let mut content = if tasks_path.exists() {
        std::fs::read_to_string(&tasks_path)?
    } else {
        "# Team Tasks\n".to_string()
    };

    content.push_str(&format!("\n## {}\n", args.title));
    content.push_str(&format!("Priority: {}\n", args.priority));
    if let Some(ref deps) = args.depends_on {
        content.push_str(&format!("Depends-on: {}\n", deps));
    }
    if let Some(ref spec) = args.specialization {
        content.push_str(&format!("Specialization: {}\n", spec));
    }
    content.push('\n');
    if let Some(ref desc) = args.description {
        content.push_str(&format!("{}\n", desc));
    }
    content.push('\n');

    std::fs::write(&tasks_path, content)?;
    println!("✅ Added task: {}", args.title);
    Ok(())
}

// ---------------------------------------------------------------------------
// Scale
// ---------------------------------------------------------------------------

async fn cmd_scale(args: TeamScaleArgs) -> anyhow::Result<()> {
    println!(
        "Scaling to {} agents... (modifying team_spec.yaml)",
        args.count
    );

    let spec_path = PathBuf::from(".codex-team/team_spec.yaml");
    let mut spec = codex_core::team::team_spec::load_team_spec(&spec_path)?;
    spec.agents.count = args.count;
    codex_core::team::team_spec::write_team_spec(&spec, &spec_path)?;

    println!("✅ Scaled to {} agents. Restart with: codex team launch", args.count);
    Ok(())
}

// ---------------------------------------------------------------------------
// Costs
// ---------------------------------------------------------------------------

async fn cmd_costs(args: TeamCostsArgs) -> anyhow::Result<()> {
    let log_path = PathBuf::from(".codex-team/costs/costs.jsonl");
    let spec_path = PathBuf::from(".codex-team/team_spec.yaml");
    let spec = codex_core::team::team_spec::load_team_spec(&spec_path)?;

    let tracker = codex_core::team::CostTracker::new(
        log_path,
        spec.budget.max_total_usd,
        spec.budget.max_per_agent_usd,
        spec.budget.alert_threshold_pct,
    );

    if args.json {
        let entries = tracker.read_all()?;
        println!("{}", serde_json::to_string_pretty(&entries)?);
    } else {
        let summary = tracker.summary()?;
        println!("💰 Cost Summary\n");
        print!("{summary}");

        if tracker.should_alert()? {
            println!("\n⚠️  Budget alert threshold reached!");
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Log
// ---------------------------------------------------------------------------

async fn cmd_log(args: TeamLogArgs) -> anyhow::Result<()> {
    let log_path = PathBuf::from(format!(
        ".codex-team/logs/{}.log",
        args.agent_id
    ));
    if !log_path.exists() {
        anyhow::bail!("No log file for agent '{}'", args.agent_id);
    }

    let content = std::fs::read_to_string(&log_path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = if lines.len() > args.lines as usize {
        lines.len() - args.lines as usize
    } else {
        0
    };

    for line in &lines[start..] {
        println!("{line}");
    }

    if args.follow {
        println!("(follow mode not yet implemented — use: tail -f {})", log_path.display());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

async fn cmd_tests(args: TeamTestsArgs) -> anyhow::Result<()> {
    let spec_path = PathBuf::from(".codex-team/team_spec.yaml");
    let spec = codex_core::team::team_spec::load_team_spec(&spec_path)?;

    let test_cmd = args
        .command
        .or(spec.validation.test_command.clone())
        .unwrap_or_else(|| "cargo test".to_string());

    println!("🧪 Running: {test_cmd}\n");

    let status = std::process::Command::new("sh")
        .args(["-c", &test_cmd])
        .status()?;

    if status.success() {
        println!("\n✅ Tests passed!");
    } else {
        println!("\n❌ Tests failed (exit code: {:?})", status.code());
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Dashboard
// ---------------------------------------------------------------------------

async fn cmd_dashboard(args: TeamDashboardArgs) -> anyhow::Result<()> {
    super::team_dashboard::run_dashboard(args.refresh).await
}
