//! Handler for the `scored_review` tool.
//!
//! Allows the model to run a confidence-scored multi-agent code review.
//! 4 parallel reviewer roles (GuidelinesAuditorA, GuidelinesAuditorB,
//! BugDetector, HistoryAnalyzer) parse findings and filter by threshold.

use async_trait::async_trait;
use serde::Deserialize;

use crate::function_tool::FunctionCallError;
use crate::tasks::scored_review::{
    ReviewerRole, ScoredReviewConfig, filter_and_sort_findings,
    parse_reviewer_output,
};
use crate::tools::context::{ToolInvocation, ToolOutput, ToolPayload};
use crate::tools::handlers::parse_arguments;
use crate::tools::registry::{ToolHandler, ToolKind};
use codex_protocol::models::FunctionCallOutputBody;

/// Arguments the model sends for `scored_review`.
#[derive(Debug, Deserialize)]
struct ScoredReviewArgs {
    /// The review output text (from one or more reviewer agents).
    review_text: String,
    /// Which reviewer role produced this output.
    #[serde(default = "default_reviewer_role")]
    reviewer_role: String,
    /// Confidence threshold (0-100). Findings below this are discarded.
    #[serde(default)]
    confidence_threshold: Option<u32>,
}

fn default_reviewer_role() -> String {
    "bug_detector".to_string()
}

pub struct ScoredReviewHandler;

#[async_trait]
impl ToolHandler for ScoredReviewHandler {
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
                    "scored_review handler received unsupported payload".to_string(),
                ));
            }
        };

        let args: ScoredReviewArgs = parse_arguments(&arguments)?;

        let role = match args.reviewer_role.as_str() {
            "guidelines_auditor_a" => ReviewerRole::GuidelinesAuditorA,
            "guidelines_auditor_b" => ReviewerRole::GuidelinesAuditorB,
            "bug_detector" => ReviewerRole::BugDetector,
            "history_analyzer" => ReviewerRole::HistoryAnalyzer,
            _ => ReviewerRole::BugDetector,
        };

        let config = ScoredReviewConfig {
            confidence_threshold: args.confidence_threshold.unwrap_or(80),
            review_model: None,
            post_comments: false,
        };

        // Parse the reviewer output into structured findings.
        let findings = parse_reviewer_output(&args.review_text, role);

        // Filter and sort by confidence threshold.
        let output = filter_and_sort_findings(findings, config.confidence_threshold);

        // Build a detailed response.
        let mut result = String::new();
        result.push_str(&format!(
            "# Scored Review Results\n\n\
             **Reviewer:** {}\n\
             **Threshold:** {}%\n\
             **Total raw findings:** {}\n\
             **Filtered out:** {}\n\
             **Findings above threshold:** {}\n\n",
            role.label(),
            output.threshold,
            output.total_raw_findings,
            output.filtered_count,
            output.findings.len(),
        ));

        if output.findings.is_empty() {
            result.push_str("No findings above the confidence threshold.\n");
        } else {
            result.push_str("## Findings\n\n");
            for (i, f) in output.findings.iter().enumerate() {
                result.push_str(&format!(
                    "### {} Finding {} (Confidence: {}%)\n\
                     - **File:** {}\n\
                     - **Line:** {}\n\
                     - **Description:** {}\n",
                    f.severity.emoji(),
                    i + 1,
                    f.confidence,
                    f.file,
                    f.line.map_or("N/A".to_string(), |l| l.to_string()),
                    f.description,
                ));
                if let Some(ref suggestion) = f.suggestion {
                    result.push_str(&format!("- **Suggestion:** {}\n", suggestion));
                }
                result.push('\n');
            }
        }

        // Show available reviewer roles for reference.
        result.push_str("## Available Reviewer Roles\n\n");
        for r in ReviewerRole::all() {
            result.push_str(&format!("- `{}` — {}\n",
                match r {
                    ReviewerRole::GuidelinesAuditorA => "guidelines_auditor_a",
                    ReviewerRole::GuidelinesAuditorB => "guidelines_auditor_b",
                    ReviewerRole::BugDetector => "bug_detector",
                    ReviewerRole::HistoryAnalyzer => "history_analyzer",
                },
                r.label(),
            ));
        }

        Ok(ToolOutput::Function {
            body: FunctionCallOutputBody::Text(result),
            success: Some(true),
        })
    }
}
