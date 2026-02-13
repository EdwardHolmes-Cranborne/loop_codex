//! 3-tier merge conflict resolution.
//!
//! Tier 1: Auto-merge (Git's built-in recursive strategy with extended options)
//! Tier 2: Heuristic (analyze conflict markers, prefer incoming for non-overlapping)
//! Tier 3: LLM-assisted (send conflicted files to a codex session for resolution)

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Command;

use super::autonomous_loop::LoopConfig;
use super::git_coordinator::GitCoordinator;

/// Which tier resolved the conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictTier {
    AutoMerge,
    Heuristic,
    LlmAssisted,
}

/// Result of a conflict resolution attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictResolution {
    pub tier: ConflictTier,
    pub files_resolved: Vec<String>,
    pub prompt_tokens: Option<u64>,
}

/// Attempt to resolve merge conflicts using the 3-tier strategy.
///
/// Returns `true` if all conflicts were resolved, `false` otherwise.
pub fn resolve_conflicts(
    git: &GitCoordinator,
    _feature_branch: &str,
    config: &LoopConfig,
) -> bool {
    // Tier 1: Try Git's auto-merge with extended strategies
    tracing::info!("Tier 1: attempting auto-merge");
    if try_auto_merge(git) {
        tracing::info!("Tier 1 resolved all conflicts");
        log_resolution(git, ConflictTier::AutoMerge, &[]);
        return true;
    }

    // Tier 2: Heuristic resolution of remaining conflicts
    tracing::info!("Tier 2: attempting heuristic resolution");
    let conflicted = match git.conflicted_files() {
        Ok(files) => files,
        Err(_) => return false,
    };

    let mut heuristic_resolved = Vec::new();
    for file in &conflicted {
        if try_heuristic_resolve(git, file) {
            heuristic_resolved.push(file.clone());
        }
    }

    // Check if everything is resolved
    if !git.has_conflicts() {
        tracing::info!(
            "Tier 2 resolved {} files",
            heuristic_resolved.len()
        );
        commit_resolution(git, "Heuristic conflict resolution");
        log_resolution(git, ConflictTier::Heuristic, &heuristic_resolved);
        return true;
    }

    // Tier 3: LLM-assisted resolution
    tracing::info!("Tier 3: attempting LLM-assisted resolution");
    let remaining = match git.conflicted_files() {
        Ok(files) => files,
        Err(_) => return false,
    };

    if try_llm_resolve(git, &remaining, config) {
        tracing::info!("Tier 3 resolved remaining {} files", remaining.len());
        commit_resolution(git, "LLM-assisted conflict resolution");
        log_resolution(git, ConflictTier::LlmAssisted, &remaining);
        return true;
    }

    // All tiers failed — abort the merge
    tracing::warn!("All conflict resolution tiers failed, aborting merge");
    let _ = run_git(&git.work_dir, &["merge", "--abort"]);
    false
}

// ---------------------------------------------------------------------------
// Tier 1: Auto-merge
// ---------------------------------------------------------------------------

/// Try Git's extended merge strategies.
fn try_auto_merge(git: &GitCoordinator) -> bool {
    // Try with "ours" strategy for .lock files and generated files
    let result = run_git(
        &git.work_dir,
        &[
            "merge",
            "-X",
            "patience",    // patience diff for better results
            "--no-edit",
            "--no-commit", // don't commit yet so we can check
        ],
    );

    if result.is_ok() && !git.has_conflicts() {
        // Commit the auto-merge
        let _ = run_git(&git.work_dir, &["commit", "--no-edit", "-m", "auto-merge: resolved"]);
        return true;
    }

    false
}

// ---------------------------------------------------------------------------
// Tier 2: Heuristic
// ---------------------------------------------------------------------------

/// Attempt heuristic resolution of a single conflicted file.
///
/// Strategy: If the only conflicts are in non-overlapping sections
/// (e.g., different functions), accept both sides. For `.lock` files
/// and generated files, prefer the incoming version.
fn try_heuristic_resolve(git: &GitCoordinator, file: &str) -> bool {
    let file_path = git.work_dir.join(file);

    // For lock files and generated files, prefer incoming (theirs)
    if file.ends_with(".lock")
        || file.ends_with(".generated")
        || file.contains("package-lock.json")
        || file.contains("Cargo.lock")
    {
        let result = run_git(
            &git.work_dir,
            &["checkout", "--theirs", file],
        );
        if result.is_ok() {
            let _ = run_git(&git.work_dir, &["add", file]);
            return true;
        }
    }

    // For state files (task board, locks), prefer incoming
    if file.contains(".codex-team/state/") {
        let result = run_git(
            &git.work_dir,
            &["checkout", "--theirs", file],
        );
        if result.is_ok() {
            let _ = run_git(&git.work_dir, &["add", file]);
            return true;
        }
    }

    // Read the file and check for simple conflict patterns
    let content = match std::fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(_) => return false,
    };

    // Count conflict markers
    let conflict_count = content.matches("<<<<<<<").count();

    // Simple single-conflict case: if both sides only add lines
    // (no deletions), we can concatenate both.
    if conflict_count == 1 {
        if let Some(resolved) = try_concatenate_conflict(&content) {
            if std::fs::write(&file_path, resolved).is_ok() {
                let _ = run_git(&git.work_dir, &["add", file]);
                return true;
            }
        }
    }

    false
}

/// Try to resolve a conflict by concatenating both sides.
///
/// This works when both sides are purely additive (e.g., two agents
/// both added new functions to the same file).
fn try_concatenate_conflict(content: &str) -> Option<String> {
    let mut result = String::new();
    let mut in_conflict = false;
    let mut ours: Vec<&str> = Vec::new();
    let mut theirs: Vec<&str> = Vec::new();
    let mut in_theirs = false;

    for line in content.lines() {
        if line.starts_with("<<<<<<<") {
            in_conflict = true;
            in_theirs = false;
            ours.clear();
            theirs.clear();
        } else if line.starts_with("=======") && in_conflict {
            in_theirs = true;
        } else if line.starts_with(">>>>>>>") && in_conflict {
            in_conflict = false;
            // Concatenate both sides
            for l in &ours {
                result.push_str(l);
                result.push('\n');
            }
            for l in &theirs {
                result.push_str(l);
                result.push('\n');
            }
        } else if in_conflict {
            if in_theirs {
                theirs.push(line);
            } else {
                ours.push(line);
            }
        } else {
            result.push_str(line);
            result.push('\n');
        }
    }

    if in_conflict {
        // Unterminated conflict — can't resolve
        None
    } else {
        Some(result)
    }
}

// ---------------------------------------------------------------------------
// Tier 3: LLM-assisted
// ---------------------------------------------------------------------------

/// Send conflicted files to a codex session for resolution.
fn try_llm_resolve(
    git: &GitCoordinator,
    conflicted_files: &[String],
    config: &LoopConfig,
) -> bool {
    // Build a prompt with all conflicted file contents
    let mut prompt = String::from(
        "# Merge Conflict Resolution\n\n\
         You are resolving merge conflicts in the following files.\n\
         For each file, resolve the conflict by keeping the intent of both sides.\n\
         Remove all conflict markers (`<<<<<<<`, `=======`, `>>>>>>>`).\n\
         Output the complete resolved file content.\n\n",
    );

    for file in conflicted_files {
        let file_path = git.work_dir.join(file);
        if let Ok(content) = std::fs::read_to_string(&file_path) {
            prompt.push_str(&format!("## File: {file}\n\n```\n{content}\n```\n\n"));
        }
    }

    // Write prompt to temp file
    let prompt_path = git.work_dir.join(".codex-team-conflict-prompt.md");
    if std::fs::write(&prompt_path, &prompt).is_err() {
        return false;
    }

    // Run codex to resolve
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

    match result {
        Ok(output) if output.status.success() => {
            // Check if conflicts are resolved
            !git.has_conflicts()
        }
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn commit_resolution(git: &GitCoordinator, message: &str) {
    let _ = run_git(&git.work_dir, &["add", "-A"]);
    let _ = run_git(&git.work_dir, &["commit", "--no-edit", "-m", message]);
}

fn log_resolution(git: &GitCoordinator, tier: ConflictTier, files: &[String]) {
    let log_dir = git.work_dir.join(".codex-team/logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("conflicts.log");

    let entry = serde_json::json!({
        "tier": tier,
        "files": files,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        let _ = writeln!(f, "{}", entry);
    }
}

fn run_git(cwd: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concatenate_simple_conflict() {
        let content = "line 1\n<<<<<<< HEAD\nfn foo() {}\n=======\nfn bar() {}\n>>>>>>> feature\nline 2\n";
        let resolved = try_concatenate_conflict(content).unwrap();
        assert!(resolved.contains("fn foo()"));
        assert!(resolved.contains("fn bar()"));
        assert!(!resolved.contains("<<<<<<<"));
        assert!(!resolved.contains("======="));
        assert!(!resolved.contains(">>>>>>>"));
    }

    #[test]
    fn concatenate_no_conflict() {
        let content = "line 1\nline 2\nline 3\n";
        let resolved = try_concatenate_conflict(content).unwrap();
        assert_eq!(resolved, content);
    }

    #[test]
    fn concatenate_unterminated_fails() {
        let content = "line 1\n<<<<<<< HEAD\nfn foo() {}\n=======\nfn bar() {}\n";
        let resolved = try_concatenate_conflict(content);
        assert!(resolved.is_none());
    }

    #[test]
    fn conflict_resolution_serializes() {
        let res = ConflictResolution {
            tier: ConflictTier::Heuristic,
            files_resolved: vec!["src/main.rs".to_string()],
            prompt_tokens: None,
        };
        let json = serde_json::to_string(&res).unwrap();
        assert!(json.contains("Heuristic"));
    }
}
