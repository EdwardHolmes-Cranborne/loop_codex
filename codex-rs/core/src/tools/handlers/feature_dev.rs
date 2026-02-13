//! Handler for the `feature_dev` tool.
//!
//! Spawns a `FeatureDevTask` that orchestrates the 7-phase structured
//! feature development workflow: Discovery → Exploration → Clarification →
//! Architecture → Implementation → Review → Summary.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tasks::FeatureDevTask;
use crate::tasks::feature_dev::{FeatureDevConfig, FeatureDevPhase};
use crate::tools::context::{ToolInvocation, ToolOutput, ToolPayload};
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::{ToolHandler, ToolKind};
use codex_protocol::models::FunctionCallOutputBody;

/// Arguments the model sends for `feature_dev`.
#[derive(Debug, Deserialize)]
struct FeatureDevArgs {
    /// The feature request / description.
    feature_request: String,
    /// Number of parallel explorer agents for Phase 2 (default: 3).
    #[serde(default)]
    explorer_count: Option<usize>,
    /// Number of parallel reviewer agents for Phase 6 (default: 2).
    #[serde(default)]
    reviewer_count: Option<usize>,
    /// Whether to skip the clarification phase (default: false).
    #[serde(default)]
    skip_clarification: Option<bool>,
}

pub struct FeatureDevHandler;

#[async_trait]
impl ToolHandler for FeatureDevHandler {
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
                    "feature_dev handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: FeatureDevArgs = parse_arguments(&arguments)?;

        let config = FeatureDevConfig {
            feature_request: args.feature_request.clone(),
            explorer_count: args.explorer_count.unwrap_or(3),
            reviewer_count: args.reviewer_count.unwrap_or(2),
            skip_clarification: args.skip_clarification.unwrap_or(false),
        };

        // Spawn the feature development task.
        let task = FeatureDevTask { config: config.clone() };
        session.spawn_task(turn, Vec::new(), task).await;

        // Build confirmation response.
        let phase_labels = FeatureDevPhase::all()
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let agent = p.agent_role().unwrap_or("general");
                format!("{}. **{}** (agent: `{}`)", i + 1, p.label(), agent)
            })
            .collect::<Vec<_>>()
            .join("\n");

        let output = format!(
            "# Feature Development Launched\n\n\
             **Feature:** {}\n\
             **Explorers:** {}\n\
             **Reviewers:** {}\n\
             **Skip clarification:** {}\n\n\
             ## Phases\n{}\n\n\
             The feature development workflow is now running in the background.",
            config.feature_request,
            config.explorer_count,
            config.reviewer_count,
            config.skip_clarification,
            phase_labels,
        );

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(output),
            success: Some(true),
        })
    }
}

