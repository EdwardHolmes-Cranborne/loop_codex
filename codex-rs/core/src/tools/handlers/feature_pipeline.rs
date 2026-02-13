//! Handler for the `feature_pipeline` tool.
//!
//! Allows the model to initiate a TDD-driven feature development pipeline.
//! Parses an implementation plan into features and reports the execution plan
//! with all 8 phases (SpecGeneration → TestWrite → TestReview → Implement →
//! TestRun → FixIssues → ReviewCommit → DocUpdate).

use async_trait::async_trait;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tasks::loop_types::LoopPhase;
use crate::tasks::pipeline::{FeaturePipeline, PipelineConfig, parse_implementation_plan};
use crate::tasks::wiggum_loop::{WiggumLoopConfig, WiggumLoopStatus, build_iteration_prompt, check_completion_promise};
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
    /// Optional: project root directory for pipeline docs.
    #[serde(default)]
    project_root: Option<String>,
}

pub struct FeaturePipelineHandler;

#[async_trait]
impl ToolHandler for FeaturePipelineHandler {
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

        let project_root = args
            .project_root
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from("."));

        let pipeline = FeaturePipeline::new(features.clone(), config, &project_root);

        // Build execution plan for all features.
        let mut output = String::new();
        output.push_str(&format!(
            "# Feature Pipeline Initialized\n\n\
             **Features found:** {}\n\
             **Pipeline phases:** {}\n\n",
            pipeline.feature_count(),
            LoopPhase::all()
                .iter()
                .map(|p| p.label())
                .collect::<Vec<_>>()
                .join(" → "),
        ));

        for feature in &features {
            let plan = pipeline.plan_feature_execution(feature);
            output.push_str(&format!("---\n\n{}\n\n", plan.describe()));

            // Show the WiggumLoop config for each phase.
            output.push_str("**Phase configurations:**\n\n");
            for phase in LoopPhase::all() {
                let loop_config = pipeline.build_loop_config(*phase);
                output.push_str(&format!(
                    "- **{}**: max {} iterations, completion signal: `{}`\n",
                    phase.label(),
                    loop_config.max_iterations,
                    loop_config.completion_promise,
                ));
            }
            output.push('\n');
        }

        // Demonstrate wiggum loop iteration prompt building.
        let sample_config = WiggumLoopConfig {
            prompt: features[0].description.clone(),
            max_iterations: 25,
            completion_promise: "PHASE_COMPLETE".to_string(),
        };
        let sample_prompt = build_iteration_prompt(&sample_config, 1);
        let _sample_check = check_completion_promise(&sample_prompt, "PHASE_COMPLETE");

        // Build a status summary to show the model how tracking works.
        let status = WiggumLoopStatus {
            iteration: 0,
            max_iterations: sample_config.max_iterations,
            completed_by_promise: false,
            hit_max_iterations: false,
            final_output: None,
        };

        output.push_str(&format!(
            "## Execution Instructions\n\n\
             To execute this pipeline, process each feature through all phases sequentially.\n\
             For each phase, use the phase's system prompt and iterate using the Wiggum Loop pattern:\n\n\
             1. Send the phase prompt to the agent\n\
             2. Check the response for the completion signal\n\
             3. If not complete, re-submit with `build_iteration_prompt` context\n\
             4. Repeat until completion signal found or max iterations reached\n\
             5. Move to next phase with prior outputs as context\n\n\
             **Current status:** iteration {}/{}, completed: {}, max_reached: {}\n",
            status.iteration,
            status.max_iterations,
            status.completed_by_promise,
            status.hit_max_iterations,
        ));

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(output),
            success: Some(true),
        })
    }
}
