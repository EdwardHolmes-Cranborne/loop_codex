//! FeatureDevTask: SessionTask for multi-phase feature development.

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, trace_span};

use crate::codex::{TurnContext, run_turn};
use crate::state::TaskKind;
use codex_protocol::user_input::UserInput;

use super::{SessionTask, SessionTaskContext};
use super::feature_dev::{
    FeatureDevConfig, FeatureDevPhase, FeatureDevStatus, PhaseOutput,
    build_phase_prompt,
};

/// A session task that drives the 7-phase feature development workflow.
///
/// Phases: Discovery → Exploration → Clarification → Architecture →
/// Implementation → Review → Summary.
///
/// Phases with `agent_role()` (Exploration, Review) can run multiple
/// parallel sub-agents; all other phases run a single agent turn.
pub(crate) struct FeatureDevTask {
    pub config: FeatureDevConfig,
}

#[async_trait]
impl SessionTask for FeatureDevTask {
    fn kind(&self) -> TaskKind {
        TaskKind::FeatureDev
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        _input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> Option<String> {
        let sess = session.clone_session();
        let mut prior_outputs: Vec<PhaseOutput> = Vec::new();

        for phase in FeatureDevPhase::all() {
            // Skip clarification if configured
            if *phase == FeatureDevPhase::Clarification && self.config.skip_clarification {
                continue;
            }
            if cancellation_token.is_cancelled() {
                break;
            }

            let prompt = build_phase_prompt(&self.config, *phase, &prior_outputs);

            // Determine how many agents to run for this phase
            let agent_count = match phase {
                FeatureDevPhase::Exploration => self.config.explorer_count,
                FeatureDevPhase::Review => self.config.reviewer_count,
                _ => 1,
            };

            let mut combined_output = String::new();
            let phase_label = phase.label();
            let _agent_role = phase.agent_role();

            for agent_idx in 0..agent_count {
                if cancellation_token.is_cancelled() {
                    break;
                }

                let span = trace_span!(
                    "feature_dev_phase",
                    phase = phase_label,
                    agent = agent_idx,
                );

                sess.set_server_reasoning_included(false).await;
                let result = run_turn(
                    Arc::clone(&sess),
                    Arc::clone(&ctx),
                    vec![UserInput::Text { text: prompt.clone(), text_elements: vec![] }],
                    None,
                    cancellation_token.child_token(),
                )
                .instrument(span)
                .await;

                if let Some(ref output) = result {
                    if agent_count > 1 {
                        combined_output.push_str(&format!(
                            "--- Agent {}/{agent_count} ({phase_label}) ---\n",
                            agent_idx + 1
                        ));
                    }
                    combined_output.push_str(output);
                    combined_output.push('\n');
                }
            }

            prior_outputs.push(PhaseOutput {
                phase: *phase,
                output: combined_output,
            });
        }

        // Build final status
        let current_phase = prior_outputs
            .last()
            .map(|po| po.phase)
            .unwrap_or(FeatureDevPhase::Discovery);

        let status = FeatureDevStatus {
            current_phase,
            phase_outputs: prior_outputs,
        };

        Some(serde_json::to_string_pretty(&status).unwrap_or_else(|_| {
            "FeatureDev workflow completed".to_string()
        }))
    }
}
