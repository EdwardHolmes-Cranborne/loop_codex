//! Handler for the `feature_pipeline` tool.
//!
//! Parses an implementation plan into features, spawns a `PipelineTask`
//! that drives each feature through the 8-phase TDD pipeline, and
//! returns a confirmation message.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tasks::PipelineTask;
use crate::tasks::loop_types::LoopPhase;
use crate::tasks::pipeline::{PipelineConfig, parse_implementation_plan};
use crate::tools::context::{ToolInvocation, ToolOutput, ToolPayload};
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::{ToolHandler, ToolKind};
use codex_protocol::models::FunctionCallOutputBody;

/// Arguments the model sends for `feature_pipeline`.
#[derive(Debug, Deserialize)]
struct FeaturePipelineArgs {
    /// Markdown implementation plan with `## Feature Name` headings.
    implementation_plan: String,
    /// Optional: max iterations per loop phase (default 25).
    #[serde(default)]
    max_iterations_per_loop: Option<usize>,
    /// Optional: max test→fix cycles (default 5).
    #[serde(default)]
    max_test_fix_cycles: Option<usize>,
}

pub struct FeaturePipelineHandler;

#[async_trait]
impl ToolHandler for FeaturePipelineHandler {
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
                    "feature_pipeline handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: FeaturePipelineArgs = parse_arguments(&arguments)?;

        // Parse the implementation plan into features.
        let features = parse_implementation_plan(&args.implementation_plan);

        if features.is_empty() {
            return Ok(ToolOutput::Function {
                body: FunctionCallOutputBody::Text(
                    "No features found in the implementation plan. \
                     Use `## Feature Name` headings to define features."
                        .to_string(),
                ),
                success: Some(false),
            });
        }

        // Build pipeline configuration.
        let mut config = PipelineConfig::default();
        if let Some(max_iter) = args.max_iterations_per_loop {
            config.max_iterations_per_loop = max_iter;
        }
        if let Some(max_cycles) = args.max_test_fix_cycles {
            config.test_fix_max_cycles = max_cycles;
        }

        let feature_count = features.len();
        let phase_labels: String = LoopPhase::all()
            .iter()
            .map(|p| p.label())
            .collect::<Vec<_>>()
            .join(" → ");

        let feature_names: Vec<String> = features.iter().map(|f| f.name.clone()).collect();

        // Build confirmation and spawn the pipeline task.
        let task = PipelineTask {
            plan_text: args.implementation_plan,
        };

        session.spawn_task(turn, Vec::new(), task).await;

        let mut output = format!(
            "# Pipeline Launched\n\n\
             **Features:** {feature_count}\n\
             **Phases:** {phase_labels}\n\
             **Max iterations per phase:** {}\n\
             **Max test→fix cycles:** {}\n\n\
             ## Features queued\n",
            config.max_iterations_per_loop,
            config.test_fix_max_cycles,
        );

        for (i, name) in feature_names.iter().enumerate() {
            output.push_str(&format!("{}. {}\n", i + 1, name));
        }

        output.push_str(
            "\nThe pipeline is now running in the background. \
             Each feature will be processed through all 8 phases sequentially.",
        );

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(output),
            success: Some(true),
        })
    }
}

