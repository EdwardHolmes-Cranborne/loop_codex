//! Handler for the `scored_review` tool.
//!
//! Spawns a `ScoredReviewTask` that runs a confidence-scored multi-agent
//! code review with 4 parallel reviewer roles.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tasks::ScoredReviewTask;
use crate::tasks::scored_review::{ReviewerRole, ScoredReviewConfig};
use crate::tools::context::{ToolInvocation, ToolOutput, ToolPayload};
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::{ToolHandler, ToolKind};
use codex_protocol::models::FunctionCallOutputBody;

/// Arguments the model sends for `scored_review`.
#[derive(Debug, Deserialize)]
struct ScoredReviewArgs {
    /// Files to review (list of paths).
    files_to_review: Vec<String>,
    /// Confidence threshold (0-100). Findings below this are discarded.
    #[serde(default)]
    confidence_threshold: Option<u32>,
    /// Optional model override for review agents.
    #[serde(default)]
    review_model: Option<String>,
    /// Whether to post comments (e.g., to GitHub PR).
    #[serde(default)]
    post_comments: Option<bool>,
}

pub struct ScoredReviewHandler;

#[async_trait]
impl ToolHandler for ScoredReviewHandler {
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
                    "scored_review handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: ScoredReviewArgs = parse_arguments(&arguments)?;

        if args.files_to_review.is_empty() {
            return Ok(ToolOutput::Function {
                body: FunctionCallOutputBody::Text(
                    "No files specified. Provide a `files_to_review` array.".to_string(),
                ),
                success: Some(false),
            });
        }

        let config = ScoredReviewConfig {
            confidence_threshold: args.confidence_threshold.unwrap_or(80),
            review_model: args.review_model,
            post_comments: args.post_comments.unwrap_or(false),
        };

        let file_count = args.files_to_review.len();

        // Spawn the scored review task.
        let task = ScoredReviewTask {
            config,
            files_to_review: args.files_to_review,
        };
        session.spawn_task(turn, Vec::new(), task).await;

        // Build confirmation.
        let roles = ReviewerRole::all()
            .iter()
            .map(|r| format!("- `{}` — {}", match r {
                ReviewerRole::GuidelinesAuditorA => "guidelines_auditor_a",
                ReviewerRole::GuidelinesAuditorB => "guidelines_auditor_b",
                ReviewerRole::BugDetector => "bug_detector",
                ReviewerRole::HistoryAnalyzer => "history_analyzer",
            }, r.label()))
            .collect::<Vec<_>>()
            .join("\n");

        let output = format!(
            "# Scored Review Launched\n\n\
             **Files to review:** {file_count}\n\
             **Confidence threshold:** {}%\n\
             **Post comments:** {}\n\n\
             ## Reviewer Roles\n{roles}\n\n\
             The review is now running in the background with 4 parallel reviewers.",
            task_threshold_display(args.confidence_threshold),
            args.post_comments.map_or("no".to_string(), |b| if b { "yes" } else { "no" }.to_string()),
        );

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(output),
            success: Some(true),
        })
    }
}

fn task_threshold_display(threshold: Option<u32>) -> u32 {
    threshold.unwrap_or(80)
}

