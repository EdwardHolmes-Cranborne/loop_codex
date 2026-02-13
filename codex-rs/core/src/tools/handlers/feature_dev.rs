//! Handler for the `feature_dev` tool.
//!
//! Exposes the 7-phase structured feature development workflow as a callable
//! tool. The model can initiate a feature development workflow that progresses
//! through Discovery → Exploration → Clarification → Architecture →
//! Implementation → Review → Summary.

use async_trait::async_trait;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tasks::feature_dev::{
    FeatureDevConfig, FeatureDevPhase, FeatureDevStatus, PhaseOutput,
    build_phase_prompt,
};
use crate::tools::context::{ToolInvocation, ToolOutput, ToolPayload};
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::{ToolHandler, ToolKind};
use codex_protocol::models::FunctionCallOutputBody;

/// Arguments the model sends for `feature_dev`.
#[derive(Debug, Deserialize)]
struct FeatureDevArgs {
    /// The feature request / description.
    feature_request: String,
    /// Which phase to generate a prompt for (default: "discovery").
    #[serde(default = "default_phase")]
    phase: String,
    /// Prior phase outputs as JSON array of {phase, output} objects.
    #[serde(default)]
    prior_outputs: Option<String>,
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

fn default_phase() -> String {
    "discovery".to_string()
}

pub struct FeatureDevHandler;

#[async_trait]
impl ToolHandler for FeatureDevHandler {
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
                    "feature_dev handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: FeatureDevArgs = parse_arguments(&arguments)?;

        let phase = match args.phase.as_str() {
            "discovery" => FeatureDevPhase::Discovery,
            "exploration" => FeatureDevPhase::Exploration,
            "clarification" => FeatureDevPhase::Clarification,
            "architecture" => FeatureDevPhase::Architecture,
            "implementation" => FeatureDevPhase::Implementation,
            "review" => FeatureDevPhase::Review,
            "summary" => FeatureDevPhase::Summary,
            _ => {
                return Ok(ToolOutput::Function {
                    body: FunctionCallOutputBody::Text(format!(
                        "Unknown phase '{}'. Valid phases: {}",
                        args.phase,
                        FeatureDevPhase::all()
                            .iter()
                            .map(|p| p.label())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )),
                    success: Some(false),
                });
            }
        };

        let config = FeatureDevConfig {
            feature_request: args.feature_request,
            explorer_count: args.explorer_count.unwrap_or(3),
            reviewer_count: args.reviewer_count.unwrap_or(2),
            skip_clarification: args.skip_clarification.unwrap_or(false),
        };

        // Parse prior outputs if provided.
        let prior_outputs: Vec<PhaseOutput> = if let Some(ref prior_json) = args.prior_outputs {
            serde_json::from_str(prior_json).unwrap_or_default()
        } else {
            vec![]
        };

        // Build the phase prompt.
        let prompt = build_phase_prompt(&config, phase, &prior_outputs);

        // Build status info.
        let status = FeatureDevStatus {
            current_phase: phase,
            phase_outputs: prior_outputs,
        };

        // Build response.
        let mut result = String::new();
        result.push_str(&format!(
            "# Feature Development — {} Phase\n\n",
            phase.label()
        ));

        // Show agent role if applicable.
        if let Some(role) = phase.agent_role() {
            result.push_str(&format!("**Agent role:** `{}`\n\n", role));
        }

        result.push_str(&format!(
            "**Feature:** {}\n\
             **Phase:** {} ({}/{})\n\
             **Prior phases completed:** {}\n\n",
            config.feature_request,
            phase.label(),
            FeatureDevPhase::all().iter().position(|p| *p == phase).unwrap_or(0) + 1,
            FeatureDevPhase::all().len(),
            status.phase_outputs.len(),
        ));

        result.push_str("## Generated Prompt\n\n");
        result.push_str(&prompt);
        result.push_str("\n\n---\n\n");

        // Show all phases for reference.
        result.push_str("## All Phases\n\n");
        for (i, p) in FeatureDevPhase::all().iter().enumerate() {
            let marker = if *p == phase { "→" } else { " " };
            let agent = p.agent_role().unwrap_or("(general)");
            result.push_str(&format!(
                "{} {}. **{}** (agent: `{}`)\n",
                marker,
                i + 1,
                p.label(),
                agent
            ));
        }

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(result),
            success: Some(true),
        })
    }
}
