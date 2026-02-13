//! Handler for the `task_dispatch` tool.
//!
//! Delegates a task to a named subagent registered in the LaborMarket.
//! The handler validates the target agent, builds a subagent prompt, and
//! returns the result to the model.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::swarm::labor_market::LaborMarket;
use crate::swarm::task_dispatch::{TaskDispatchParams, TaskDispatchResult, build_subagent_prompt, validate_dispatch};
use crate::tools::context::{ToolInvocation, ToolOutput, ToolPayload};
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::{ToolHandler, ToolKind};
use codex_protocol::models::FunctionCallOutputBody;

/// Arguments the model sends for `task_dispatch`.
#[derive(Debug, Deserialize)]
struct TaskDispatchArgs {
    agent_name: String,
    task: String,
}

pub struct TaskDispatchHandler {
    pub market: Arc<LaborMarket>,
}

#[async_trait]
impl ToolHandler for TaskDispatchHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        false
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let arguments = match invocation.payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "task_dispatch handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: TaskDispatchArgs = parse_arguments(&arguments)?;

        let params = TaskDispatchParams {
            agent_name: args.agent_name.clone(),
            task: args.task.clone(),
        };

        // Validate that the agent exists in the LaborMarket.
        let agent_entry = match validate_dispatch(&self.market, &params) {
            Ok(entry) => entry,
            Err(err_msg) => {
                return Ok(ToolOutput::Function {
                    body: FunctionCallOutputBody::Text(err_msg),
                    success: Some(false),
                });
            }
        };

        // Build the subagent prompt (combining agent spec + task).
        let prompt = build_subagent_prompt(&agent_entry.spec, &args.task);

        // Return the dispatch result with the constructed prompt.
        // The model can use this to understand what the subagent would receive.
        let result = TaskDispatchResult {
            agent_name: agent_entry.name.clone(),
            success: true,
            output: format!(
                "Task dispatched to agent '{}'.\n\nAgent description: {}\n\nConstructed prompt:\n{}",
                agent_entry.name,
                agent_entry.description,
                prompt
            ),
        };

        let result_json = serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
            format!("Dispatched task to agent '{}'", agent_entry.name)
        });

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(result_json),
            success: Some(true),
        })
    }
}
