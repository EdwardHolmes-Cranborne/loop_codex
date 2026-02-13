//! Patrol mode: what agents do when no tasks are available.
//!
//! When all tasks are claimed or completed, idle agents enter patrol mode:
//! 1. Run the test suite and fix any regressions
//! 2. Review recent commits for quality issues
//! 3. Scan for TODO/FIXME/HACK comments and create sub-tasks
//! 4. Optimize existing code (reduce duplication, improve docs)

use std::process::Command;

use super::agent_process::AgentProcess;
use super::autonomous_loop::{LoopConfig, LoopOutcome};
use super::git_coordinator::GitCoordinator;

/// Run one patrol cycle.
///
/// Returns a [`LoopOutcome::Patrolled`] after performing one of the
/// patrol activities.
pub fn run_patrol(
    agent: &mut AgentProcess,
    git: &GitCoordinator,
    config: &LoopConfig,
) -> LoopOutcome {
    // 1. Run tests if configured
    if let Some(ref test_cmd) = config.test_command {
        let test_result = run_test_suite(test_cmd, &git.work_dir, config.test_timeout_seconds);
        match test_result {
            TestResult::AllPassed => {
                tracing::debug!(agent = %agent.id, "patrol: all tests pass");
            }
            TestResult::SomeFailures { output } => {
                tracing::info!(
                    agent = %agent.id,
                    "patrol: found test failures, attempting fix"
                );
                attempt_fix_regression(agent, git, &output, config);
                return LoopOutcome::Patrolled;
            }
            TestResult::RunFailed => {
                tracing::warn!(agent = %agent.id, "patrol: test command failed to run");
            }
        }
    }

    // 2. Scan for TODO/FIXME/HACK
    let todos = scan_for_todos(&git.work_dir);
    if !todos.is_empty() {
        tracing::info!(
            agent = %agent.id,
            "patrol: found {} TODO/FIXME markers",
            todos.len()
        );
        // Log them but don't auto-create subtasks yet (Phase 4)
    }

    // 3. Check for code quality patterns
    let quality_issues = scan_for_quality_issues(&git.work_dir);
    if !quality_issues.is_empty() {
        tracing::info!(
            agent = %agent.id,
            "patrol: found {} quality issues",
            quality_issues.len()
        );
    }

    LoopOutcome::Patrolled
}

// ---------------------------------------------------------------------------
// Test suite runner
// ---------------------------------------------------------------------------

enum TestResult {
    AllPassed,
    SomeFailures { output: String },
    RunFailed,
}

fn run_test_suite(test_cmd: &str, work_dir: &std::path::Path, _timeout_secs: u32) -> TestResult {
    let parts: Vec<&str> = test_cmd.split_whitespace().collect();
    if parts.is_empty() {
        return TestResult::AllPassed;
    }

    let result = Command::new(parts[0])
        .args(&parts[1..])
        .current_dir(work_dir)
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                TestResult::AllPassed
            } else {
                let combined = format!(
                    "STDOUT:\n{}\nSTDERR:\n{}",
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                // Truncate to max_lines equivalent
                let truncated: String = combined
                    .lines()
                    .take(200)
                    .collect::<Vec<_>>()
                    .join("\n");
                TestResult::SomeFailures {
                    output: truncated,
                }
            }
        }
        Err(_) => TestResult::RunFailed,
    }
}

/// Attempt to fix a test regression by running a codex session.
fn attempt_fix_regression(
    agent: &AgentProcess,
    git: &GitCoordinator,
    test_output: &str,
    config: &LoopConfig,
) {
    let prompt = format!(
        "# Fix Test Regression\n\n\
         The following tests are failing. Analyze the output and fix the regression.\n\
         Make minimal changes to fix the failures.\n\n\
         ## Test Output\n\n```\n{test_output}\n```\n"
    );

    let prompt_path = git.work_dir.join(".codex-team-patrol-prompt.md");
    if std::fs::write(&prompt_path, &prompt).is_err() {
        return;
    }

    let mut cmd = Command::new(&config.codex_binary);
    cmd.args(["exec", "--prompt-from-file"]);
    cmd.arg(&prompt_path);
    cmd.current_dir(&git.work_dir);

    if let Some(ref provider) = config.model_provider {
        cmd.args(["--provider", provider]);
    }
    if let Some(ref model) = config.model {
        cmd.args(["--model", model]);
    }

    let result = cmd.output();
    let _ = std::fs::remove_file(&prompt_path);

    if let Ok(output) = result {
        if output.status.success() {
            // Commit and push the fix
            let _ = Command::new("git")
                .args(["add", "-A"])
                .current_dir(&git.work_dir)
                .status();
            let _ = Command::new("git")
                .args([
                    "commit",
                    "-m",
                    &format!("fix(patrol): {} fixes test regression", agent.id),
                ])
                .current_dir(&git.work_dir)
                .status();
            let _ = Command::new("git")
                .args(["push", &git.remote, &git.branch])
                .current_dir(&git.work_dir)
                .status();
        }
    }
}

// ---------------------------------------------------------------------------
// Code scanning
// ---------------------------------------------------------------------------

/// Scan the working directory for TODO/FIXME/HACK comments.
fn scan_for_todos(work_dir: &std::path::Path) -> Vec<TodoItem> {
    let output = Command::new("grep")
        .args([
            "-rn",
            "--include=*.rs",
            "--include=*.ts",
            "--include=*.py",
            "--include=*.go",
            "-E",
            r"(TODO|FIXME|HACK|XXX):",
            ".",
        ])
        .current_dir(work_dir)
        .output();

    match output {
        Ok(out) => {
            let text = String::from_utf8_lossy(&out.stdout);
            text.lines()
                .take(100) // cap at 100 items
                .filter_map(|line| {
                    let parts: Vec<&str> = line.splitn(3, ':').collect();
                    if parts.len() >= 3 {
                        Some(TodoItem {
                            file: parts[0].to_string(),
                            line_number: parts[1].parse().unwrap_or(0),
                            text: parts[2].trim().to_string(),
                        })
                    } else {
                        None
                    }
                })
                .collect()
        }
        Err(_) => Vec::new(),
    }
}

/// A TODO/FIXME found in the codebase.
#[derive(Debug)]
struct TodoItem {
    file: String,
    line_number: u32,
    text: String,
}

/// Scan for basic code quality issues (very lightweight).
fn scan_for_quality_issues(work_dir: &std::path::Path) -> Vec<String> {
    let mut issues = Vec::new();

    // Check for very large files (>1000 lines)
    let output = Command::new("find")
        .args([".", "-name", "*.rs", "-type", "f"])
        .current_dir(work_dir)
        .output();

    if let Ok(out) = output {
        let files = String::from_utf8_lossy(&out.stdout);
        for file in files.lines().take(200) {
            if let Ok(content) = std::fs::read_to_string(work_dir.join(file)) {
                let line_count = content.lines().count();
                if line_count > 1000 {
                    issues.push(format!(
                        "{file}: {line_count} lines (consider splitting)"
                    ));
                }
            }
        }
    }

    issues
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_todos_handles_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let todos = scan_for_todos(dir.path());
        assert!(todos.is_empty());
    }

    #[test]
    fn scan_todos_finds_markers() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("test.rs");
        std::fs::write(&file, "// TODO: fix this\nfn main() {}\n// FIXME: broken\n").unwrap();
        let todos = scan_for_todos(dir.path());
        assert!(todos.len() >= 2);
    }

    #[test]
    fn quality_scan_handles_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let issues = scan_for_quality_issues(dir.path());
        assert!(issues.is_empty());
    }
}
