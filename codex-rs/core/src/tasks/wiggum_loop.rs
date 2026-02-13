//! WiggumLoopTask: iterative development loops (Ralph Wiggum pattern).
//!
//! Spawns a sub-codex turn, waits for completion, then re-submits the same
//! prompt if the output doesn't contain a completion promise. This creates
//! autonomous iteration loops that continue until the agent signals it's done.

use serde::Deserialize;
use serde::Serialize;

/// Configuration for a Wiggum Loop.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WiggumLoopConfig {
    /// The prompt to iterate on.
    pub prompt: String,
    /// Maximum number of iterations before forced exit.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    /// String that signals the loop is complete.
    /// When found in the agent's output, the loop exits.
    #[serde(default = "default_completion_promise")]
    pub completion_promise: String,
}

fn default_max_iterations() -> usize {
    25
}

fn default_completion_promise() -> String {
    "WIGGUM_LOOP_COMPLETE".to_string()
}

impl Default for WiggumLoopConfig {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            max_iterations: default_max_iterations(),
            completion_promise: default_completion_promise(),
        }
    }
}

/// Status of a Wiggum Loop execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WiggumLoopStatus {
    /// Current iteration number (1-indexed).
    pub iteration: usize,
    /// Maximum iterations allowed.
    pub max_iterations: usize,
    /// Whether the loop completed via completion promise.
    pub completed_by_promise: bool,
    /// Whether the loop hit the max iteration limit.
    pub hit_max_iterations: bool,
    /// Output from the final iteration.
    pub final_output: Option<String>,
}

/// Check if the agent's output contains the completion promise.
pub fn check_completion_promise(output: &str, promise: &str) -> bool {
    if promise.is_empty() {
        return false;
    }
    output.contains(promise)
}

/// Build the re-entry prompt for the next iteration of the loop.
///
/// This includes context about the iteration state so the agent can track
/// its progress and knows when to emit the completion promise.
pub fn build_iteration_prompt(config: &WiggumLoopConfig, iteration: usize) -> String {
    format!(
        "Continue working on the task below. This is iteration {iteration}/{max} of the \
        Ralph Wiggum loop. When you have fully completed the task, include the exact string \
        \"{promise}\" in your response to signal completion.\n\n\
        If you are not done yet, continue working. Do NOT include the completion signal \
        until the task is truly finished.\n\n\
        ORIGINAL TASK:\n{prompt}",
        max = config.max_iterations,
        promise = config.completion_promise,
        prompt = config.prompt,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_promise_detection() {
        let promise = "WIGGUM_LOOP_COMPLETE";
        assert!(check_completion_promise(
            "All done! WIGGUM_LOOP_COMPLETE",
            promise
        ));
        assert!(!check_completion_promise("Still working...", promise));
        assert!(!check_completion_promise("", promise));
    }

    #[test]
    fn empty_promise_never_matches() {
        assert!(!check_completion_promise("anything", ""));
    }

    #[test]
    fn iteration_prompt_includes_context() {
        let config = WiggumLoopConfig {
            prompt: "Refactor the auth module".to_string(),
            max_iterations: 10,
            completion_promise: "DONE".to_string(),
        };

        let prompt = build_iteration_prompt(&config, 3);
        assert!(prompt.contains("iteration 3/10"));
        assert!(prompt.contains("\"DONE\""));
        assert!(prompt.contains("Refactor the auth module"));
    }

    #[test]
    fn default_config_values() {
        let config = WiggumLoopConfig::default();
        assert_eq!(config.max_iterations, 25);
        assert_eq!(config.completion_promise, "WIGGUM_LOOP_COMPLETE");
    }

    #[test]
    fn wiggum_loop_status_tracking() {
        let status = WiggumLoopStatus {
            iteration: 5,
            max_iterations: 25,
            completed_by_promise: true,
            hit_max_iterations: false,
            final_output: Some("All done!".to_string()),
        };
        assert!(status.completed_by_promise);
        assert!(!status.hit_max_iterations);
        assert_eq!(status.iteration, 5);
    }
}
