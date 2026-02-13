//! ScoredReviewTask: SessionTask for confidence-scored multi-agent review.

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, trace_span};

use crate::codex::{TurnContext, run_turn};
use crate::state::TaskKind;
use codex_protocol::user_input::UserInput;

use super::{SessionTask, SessionTaskContext};
use super::scored_review::{
    ScoredReviewConfig, ReviewerRole, ReviewFinding,
    filter_and_sort_findings, parse_reviewer_output,
};

/// A session task that runs a multi-agent scored code review.
///
/// Spawns one `run_turn()` per reviewer role (4 total), parses each
/// output for JSON findings, then filters and sorts by confidence.
pub(crate) struct ScoredReviewTask {
    pub config: ScoredReviewConfig,
    pub files_to_review: Vec<String>,
}

#[async_trait]
impl SessionTask for ScoredReviewTask {
    fn kind(&self) -> TaskKind {
        TaskKind::ScoredReview
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        _input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> Option<String> {
        let sess = session.clone_session();
        let mut all_findings: Vec<ReviewFinding> = Vec::new();

        let files_str = self.files_to_review.join("\n");

        for role in ReviewerRole::all() {
            if cancellation_token.is_cancelled() {
                break;
            }

            let prompt = format!(
                "{}\n\n---\nFILES TO REVIEW:\n{files_str}",
                role.system_prompt(),
            );

            let span = trace_span!(
                "scored_review_role",
                role = role.label(),
            );

            sess.set_server_reasoning_included(false).await;
            let result = run_turn(
                Arc::clone(&sess),
                Arc::clone(&ctx),
                vec![UserInput::Text { text: prompt, text_elements: vec![] }],
                None,
                cancellation_token.child_token(),
            )
            .instrument(span)
            .await;

            if let Some(ref output) = result {
                let findings = parse_reviewer_output(output, *role);
                all_findings.extend(findings);
            }
        }

        // Filter and sort by confidence
        let output = filter_and_sort_findings(all_findings, self.config.confidence_threshold);

        // Build human-readable summary
        let mut summary = format!(
            "# Scored Review Complete\n\n\
             - **Total findings**: {}\n\
             - **Filtered out** (below {}% confidence): {}\n\
             - **Reported**: {}\n\n",
            output.total_raw_findings,
            output.threshold,
            output.filtered_count,
            output.findings.len(),
        );

        if output.findings.is_empty() {
            summary.push_str("No findings above the confidence threshold. 🎉\n");
        } else {
            summary.push_str("| Sev | Conf | Reviewer | File | Line | Description |\n");
            summary.push_str("|-----|------|----------|------|------|-------------|\n");
            for f in &output.findings {
                let line_str = f.line.map_or("—".to_string(), |l| l.to_string());
                summary.push_str(&format!(
                    "| {} | {}% | {} | `{}` | {} | {} |\n",
                    f.severity.emoji(),
                    f.confidence,
                    f.reviewer.label(),
                    f.file,
                    line_str,
                    f.description,
                ));
            }
        }

        Some(summary)
    }
}
