//! Handler for the `wiggum_loop` tool.
//!
//! Spawns a `WiggumLoopTask` that iteratively calls `run_turn()` until
//! the agent signals completion via a promise string, or max iterations
//! are reached.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tasks::WiggumLoopTask;
use crate::tasks::wiggum_loop::WiggumLoopConfig;
use crate::tools::context::{ToolInvocation, ToolOutput, ToolPayload};
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::{ToolHandler, ToolKind};
use codex_protocol::models::FunctionCallOutputBody;
use codex_protocol::user_input::UserInput;

/// Arguments the model sends for `wiggum_loop`.
#[derive(Debug, Deserialize)]
struct WiggumLoopArgs {
    /// The prompt to iterate on.
    prompt: String,
    /// Maximum number of iterations (default: 25).
    #[serde(default)]
    max_iterations: Option<usize>,
    /// String that signals loop completion (default: "WIGGUM_LOOP_COMPLETE").
    #[serde(default)]
    completion_promise: Option<String>,
}

pub struct WiggumLoopHandler;

#[async_trait]
impl ToolHandler for WiggumLoopHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        true
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let session = Arc::clone(&invocation.session);
        let turn = Arc::clone(&invocation.turn);

        let arguments = match invocation.payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "wiggum_loop handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: WiggumLoopArgs = parse_arguments(&arguments)?;

        let config = WiggumLoopConfig {
            prompt: args.prompt.clone(),
            max_iterations: args.max_iterations.unwrap_or(25),
            completion_promise: args
                .completion_promise
                .unwrap_or_else(|| "WIGGUM_LOOP_COMPLETE".to_string()),
        };

        let task = WiggumLoopTask { config: config.clone() };
        let input = vec![UserInput::Text {
            text: args.prompt,
            text_elements: vec![],
        }];
        session.spawn_task(turn, input, task).await;

        let output = format!(
            "# Wiggum Loop Launched\n\n\
             **Max iterations:** {}\n\
             **Completion signal:** `{}`\n\n\
             The loop is now running. It will iterate until the agent's output \
             contains the completion signal or max iterations are reached.",
            config.max_iterations,
            config.completion_promise,
        );

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(output),
            success: Some(true),
        })
    }
}
