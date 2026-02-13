//! Terminal window spawner (macOS).
//!
//! Opens a new Terminal.app window for each agent, running `loop_codex`
//! with the agent's task. The agent gets its own TUI showing live progress.

use std::io;
use std::path::Path;
use std::process::Command;

/// Open a new Terminal.app window running codex for the given agent.
///
/// Returns the PID of the shell process inside the new window.
pub fn open_agent_window(
    agent_id: &str,
    codex_binary: &Path,
    work_dir: &Path,
    task_prompt: &str,
    log_file: &Path,
    provider_flag: Option<&str>,
    provider: Option<&str>,
    model: Option<&str>,
) -> io::Result<u32> {
    // Build the codex command that will run inside the terminal window.
    // We use the interactive mode (not exec) so the agent gets a full TUI.
    let mut cmd_parts = vec![
        format!("cd {}", shell_escape(work_dir)),
        format!(
            "export CODEX_TEAM_AGENT_ID={}",
            shell_escape_str(agent_id)
        ),
    ];

    // Build the codex invocation
    let mut codex_cmd = format!(
        "{} --full-auto {}",
        shell_escape(&codex_binary),
        shell_escape_str(task_prompt),
    );

    if let Some(flag) = provider_flag {
        codex_cmd.push_str(&format!(" {flag}"));
    }
    if let Some(p) = provider {
        codex_cmd.push_str(&format!(" --provider {}", shell_escape_str(p)));
    }
    if let Some(m) = model {
        codex_cmd.push_str(&format!(" -m {}", shell_escape_str(m)));
    }

    // Tee output to log file while keeping it in the terminal
    cmd_parts.push(format!(
        "{} 2>&1 | tee {}",
        codex_cmd,
        shell_escape(log_file),
    ));

    let script = cmd_parts.join(" && ");

    // Use osascript to tell Terminal.app to open a new window with our command.
    let applescript = format!(
        r#"tell application "Terminal"
    activate
    set newTab to do script "{script_escaped}"
    set custom title of front window to "Agent: {agent_id}"
end tell"#,
        script_escaped = script.replace('\\', "\\\\").replace('"', "\\\""),
        agent_id = agent_id,
    );

    let status = Command::new("osascript")
        .args(["-e", &applescript])
        .output()?;

    if !status.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "osascript failed: {}",
                String::from_utf8_lossy(&status.stderr)
            ),
        ));
    }

    // osascript doesn't directly give us the PID of the new shell.
    // We use a second script to get the PID of the most recent terminal process.
    let pid_script = format!(
        r#"tell application "System Events"
    set termProcs to every process whose name is "Terminal"
    if (count of termProcs) > 0 then
        set p to front process
        return unix id of p
    end if
end tell"#
    );

    let pid_output = Command::new("osascript")
        .args(["-e", &pid_script])
        .output()?;

    let pid_str = String::from_utf8_lossy(&pid_output.stdout);
    let pid = pid_str
        .trim()
        .parse::<u32>()
        .unwrap_or(0);

    Ok(pid)
}

/// Shell-escape a path.
fn shell_escape(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', "'\\''"))
}

/// Shell-escape a string.
fn shell_escape_str(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_escape_handles_spaces() {
        let p = Path::new("/some path/with spaces");
        let escaped = shell_escape(p);
        assert_eq!(escaped, "'/some path/with spaces'");
    }

    #[test]
    fn shell_escape_handles_quotes() {
        let escaped = shell_escape_str("it's a test");
        assert_eq!(escaped, "'it'\\''s a test'");
    }
}
