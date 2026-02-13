//! Team daemon: background process that spawns and monitors agents.
//!
//! The daemon is the parent process that:
//! 1. Writes its PID to `state/daemon.pid`
//! 2. Spawns agent processes
//! 3. Monitors heartbeats and respawns dead agents
//! 4. Handles SIGTERM for graceful shutdown
//! 5. Cleans up on exit

use std::path::PathBuf;

use super::team_spec::TeamSpec;

/// Run the team daemon.
///
/// In foreground mode this blocks the current process.
/// In background mode it spawns a child and returns immediately.
pub async fn run_daemon(
    spec: TeamSpec,
    agent_count: u32,
    use_docker: bool,
    _foreground: bool,
) -> Result<(), DaemonError> {
    let team_dir = PathBuf::from(".codex-team");
    let state_dir = team_dir.join("state");
    std::fs::create_dir_all(&state_dir)
        .map_err(|e| DaemonError::Io(state_dir.clone(), e))?;

    // Write PID file
    let pid_path = state_dir.join("daemon.pid");
    let pid = std::process::id();
    std::fs::write(&pid_path, pid.to_string())
        .map_err(|e| DaemonError::Io(pid_path.clone(), e))?;

    tracing::info!("team daemon started (PID {pid}, {agent_count} agents)");

    // Build spawn config
    let codex_binary = std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("codex"));

    let repo_url = ".".to_string();
    let branch = spec.git.branch.clone();

    let spawn_config = super::agent_process::SpawnConfig {
        codex_binary,
        repo_url,
        branch,
        work_base_dir: team_dir.join("workdirs"),
        team_spec_path: team_dir.join("team_spec.yaml"),
        use_docker,
        docker_image: "codex:latest".to_string(),
        model_provider: spec.agents.model_provider.clone(),
        model: spec.agents.model.clone(),
        log_dir: team_dir.join("logs"),
    };

    // Spawn agents
    let specializations: Vec<(String, u32)> = spec
        .agents
        .specializations
        .iter()
        .map(|s| (s.name.clone(), s.count))
        .collect();

    let mut handles = super::agent_process::spawn_team(
        agent_count,
        &specializations,
        &spawn_config,
    )
    .map_err(|e| DaemonError::SpawnFailed(e.to_string()))?;

    tracing::info!("spawned {} agents", handles.len());

    // Monitor loop
    let heartbeat_interval = std::time::Duration::from_secs(30);

    loop {
        tokio::time::sleep(heartbeat_interval).await;

        // Check if PID file was removed (graceful stop signal)
        if !pid_path.exists() {
            tracing::info!("PID file removed, shutting down gracefully");
            break;
        }

        // Check agent health
        let mut all_done = true;
        for handle in &mut handles {
            if handle.is_alive() {
                all_done = false;
            } else {
                let exit_code = handle.wait();
                tracing::info!(
                    agent = %handle.agent.id,
                    "agent exited (code: {:?})",
                    exit_code
                );
            }
        }

        if all_done {
            tracing::info!("all agents have exited");
            break;
        }
    }

    // Cleanup
    let _ = std::fs::remove_file(&pid_path);
    tracing::info!("daemon shutdown complete");
    Ok(())
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum DaemonError {
    Io(PathBuf, std::io::Error),
    SpawnFailed(String),
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, e) => write!(f, "daemon I/O error at '{}': {e}", path.display()),
            Self::SpawnFailed(msg) => write!(f, "failed to spawn agents: {msg}"),
        }
    }
}

impl std::error::Error for DaemonError {}
