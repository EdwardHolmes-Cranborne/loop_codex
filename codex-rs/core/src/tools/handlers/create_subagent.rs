//! Handler for the `create_subagent` tool.
//!
//! Allows the model to dynamically create a new specialist agent and
//! register it in the LaborMarket for subsequent TaskDispatch calls.

use async_trait::async_trait;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::swarm::create_subagent::{CreateSubagentParams, create_subagent};
use crate::tools::context::{ToolInvocation, ToolOutput, ToolPayload};
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::{ToolHandler, ToolKind};
use codex_protocol::models::FunctionCallOutputBody;

/// Arguments the model sends for `create_subagent`.
#[derive(Debug, Deserialize)]
struct CreateSubagentArgs {
    name: String,
    description: String,
    system_prompt: String,
}

pub struct CreateSubagentHandler;

#[async_trait]
impl ToolHandler for CreateSubagentHandler {
    fn kind(&self) -> ToolKind {
        ToolKind::Function
    }

    async fn is_mutating(&self, _invocation: &ToolInvocation) -> bool {
        // Creates state (registers an agent), so treat as mutating.
        true
    }

    async fn handle(&self, invocation: ToolInvocation) -> Result<ToolOutput, FunctionCallError> {
        let arguments = match invocation.payload {
            ToolPayload::Function { arguments } => arguments,
            _ => {
                return Err(FunctionCallError::RespondToModel(
                    "create_subagent handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: CreateSubagentArgs = parse_arguments(&arguments)?;

        let params = CreateSubagentParams {
            name: args.name,
            description: args.description,
            system_prompt: args.system_prompt,
        };

        // Read the LaborMarket from the session services.
        let market = &invocation.session.services.labor_market;

        let result = create_subagent(market, params);

        let result_json = serde_json::to_string_pretty(&result).unwrap_or_else(|_| {
            format!("create_subagent: success={}", result.success)
        });

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(result_json),
            success: Some(result.success),
        })
    }
}

