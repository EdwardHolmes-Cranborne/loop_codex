//! WiggumLoopTask: SessionTask implementation for iterative development loops.

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, trace_span};

use crate::codex::{TurnContext, run_turn};
use crate::state::TaskKind;
use codex_protocol::user_input::UserInput;

use super::{SessionTask, SessionTaskContext};
use super::wiggum_loop::{
    WiggumLoopConfig, WiggumLoopStatus,
    check_completion_promise, build_iteration_prompt,
};

/// A session task that drives iterative WiggumLoop execution.
///
/// Repeatedly calls `run_turn()` until the agent's output contains the
/// completion promise string, or the maximum iteration count is reached.
pub(crate) struct WiggumLoopTask {
    pub config: WiggumLoopConfig,
}

#[async_trait]
impl SessionTask for WiggumLoopTask {
    fn kind(&self) -> TaskKind {
        TaskKind::WiggumLoop
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> Option<String> {
        let sess = session.clone_session();
        let mut last_output: Option<String>;
        let mut iteration: usize;
        let mut completed_by_promise = false;

        // First iteration uses the original input; subsequent iterations use the re-entry prompt.
        let first_result = {
            iteration = 1;
            let span = trace_span!("wiggum_loop_iter", iteration = 1);
            sess.set_server_reasoning_included(false).await;
            run_turn(
                Arc::clone(&sess),
                Arc::clone(&ctx),
                input,
                None,
                cancellation_token.child_token(),
            )
            .instrument(span)
            .await
        };

        if let Some(ref output) = first_result {
            if check_completion_promise(output, &self.config.completion_promise) {
                completed_by_promise = true;
                last_output = first_result;
            } else {
                last_output = first_result;
            }
        } else {
            last_output = first_result;
        }

        // Subsequent iterations
        if !completed_by_promise {
            for iter in 2..=self.config.max_iterations {
                if cancellation_token.is_cancelled() {
                    break;
                }
                iteration = iter;

                let prompt = build_iteration_prompt(&self.config, iter);
                let iter_input = vec![UserInput::Text { text: prompt, text_elements: vec![] }];

                let span = trace_span!("wiggum_loop_iter", iteration = iter);
                let result = run_turn(
                    Arc::clone(&sess),
                    Arc::clone(&ctx),
                    iter_input,
                    None,
                    cancellation_token.child_token(),
                )
                .instrument(span)
                .await;

                if let Some(ref output) = result {
                    if check_completion_promise(output, &self.config.completion_promise) {
                        completed_by_promise = true;
                        last_output = result;
                        break;
                    }
                }
                last_output = result;
            }
        }

        // Build status summary
        let status = WiggumLoopStatus {
            iteration,
            max_iterations: self.config.max_iterations,
            completed_by_promise,
            hit_max_iterations: iteration >= self.config.max_iterations && !completed_by_promise,
            final_output: last_output.clone(),
        };

        Some(serde_json::to_string_pretty(&status).unwrap_or_else(|_| {
            format!(
                "WiggumLoop completed: {} iterations, promise={completed_by_promise}",
                iteration
            )
        }))
    }
}
