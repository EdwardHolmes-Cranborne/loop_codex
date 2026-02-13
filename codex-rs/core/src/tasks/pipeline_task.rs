//! PipelineTask: SessionTask for TDD-driven feature development pipeline.
//!
//! Parses an implementation plan into features, then drives each through
//! the 8-phase pipeline: SpecGeneration → TestWrite → TestReview → Implement →
//! TestRun → FixIssues (cycle) → ReviewCommit → DocUpdate.

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{Instrument, trace_span, warn};

use crate::codex::{TurnContext, run_turn};
use crate::state::TaskKind;
use codex_protocol::user_input::UserInput;

use super::{SessionTask, SessionTaskContext};
use super::loop_context::LoopOutput;
use super::loop_types::LoopPhase;
use super::global_log::GlobalImplementationLog;
use super::pipeline::{
    FeaturePipeline, LoopResult, PipelineConfig, parse_implementation_plan,
};
use super::wiggum_loop::{check_completion_promise, build_iteration_prompt};

/// A session task that orchestrates the full TDD feature pipeline.
///
/// For each feature parsed from the implementation plan, it runs all 8
/// `LoopPhase` stages as WiggumLoop iterations with optional test→fix cycling.
pub(crate) struct PipelineTask {
    pub plan_text: String,
}

#[async_trait]
impl SessionTask for PipelineTask {
    fn kind(&self) -> TaskKind {
        TaskKind::Pipeline
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<SessionTaskContext>,
        ctx: Arc<TurnContext>,
        _input: Vec<UserInput>,
        cancellation_token: CancellationToken,
    ) -> Option<String> {
        let sess = session.clone_session();
        let cwd = ctx.cwd.clone();

        // Parse the plan into features
        let features = parse_implementation_plan(&self.plan_text);
        if features.is_empty() {
            return Some("No features found in implementation plan.".to_string());
        }

        let config = PipelineConfig::default();
        let mut pipeline = FeaturePipeline::new(features.clone(), config, &cwd);

        // Allow custom log path via environment variable.
        let custom_log = std::env::var("CODEX_PIPELINE_LOG").ok().map(std::path::PathBuf::from);
        if let Some(log_path) = custom_log {
            pipeline.global_log = GlobalImplementationLog::at(log_path);
        }

        // Ensure global log exists
        if let Err(e) = pipeline.global_log.ensure_exists() {
            warn!("Failed to create global log: {e}");
        }

        let total_features = pipeline.feature_count();
        let mut feature_summaries: Vec<String> = Vec::new();

        for (feat_idx, feature) in features.iter().enumerate() {
            if cancellation_token.is_cancelled() {
                break;
            }
            pipeline.set_running(feat_idx);

            let mut prior_loop_outputs: Vec<LoopOutput> = Vec::new();

            // Log the execution plan
            let exec_plan = pipeline.plan_feature_execution(feature);
            let plan_desc = exec_plan.describe();
            let _max_phases = exec_plan.max_phase_sequence();
            tracing::debug!("Feature plan: {plan_desc}");

            for phase in LoopPhase::all() {
                if cancellation_token.is_cancelled() {
                    break;
                }

                // Build loop context with doc lifecycle
                let loop_ctx = pipeline.create_loop_context(
                    feature,
                    *phase,
                    prior_loop_outputs.clone(),
                );

                // Create docs directory for this feature
                if let Err(e) = loop_ctx.create_feature_docs_dir() {
                    warn!("Failed to create docs dir: {e}");
                }

                // Build WiggumLoopConfig from pipeline
                let loop_config = pipeline.build_loop_config(*phase);

                // Fast Phase 1/2 safety pre-check (no LLM overhead)
                let quick_verdict = pipeline.check_command_safety(&loop_config.prompt);
                if !quick_verdict.safe {
                    let risk_info = quick_verdict.risk_category
                        .map(|c| format!("{} {}", c.emoji(), c.label()))
                        .unwrap_or_default();
                    let reason = format!(
                        "Safety guard (Phase 1/2) blocked phase {} for feature '{}': {} ({})",
                        phase.label(), feature.name, quick_verdict.explanation, risk_info,
                    );
                    warn!("{reason}");
                    pipeline.set_failed(feat_idx, *phase, reason);
                    break;
                }

                // Full Phase 3 LLM safety check (more expensive, includes prompt generation)
                let (safety_verdict, llm_safety_prompt) =
                    pipeline.check_command_safety_with_llm(&loop_config.prompt);

                // If Phase 3 produced an LLM prompt, log it for future async analysis.
                if let Some(ref prompt) = llm_safety_prompt {
                    tracing::debug!(
                        "Phase 3 LLM safety prompt generated ({} chars) for phase {}",
                        prompt.len(),
                        phase.label(),
                    );
                }

                if !safety_verdict.safe {
                    let risk_info = safety_verdict.risk_category
                        .map(|c| format!("{} {}", c.emoji(), c.label()))
                        .unwrap_or_default();
                    let reason = format!(
                        "Safety guard blocked phase {} for feature '{}': {} ({})",
                        phase.label(), feature.name, safety_verdict.explanation, risk_info,
                    );
                    warn!("{reason}");
                    pipeline.set_failed(feat_idx, *phase, reason);
                    break;
                }

                // Build full prompt from LoopContext (includes prior outputs + feature docs)
                let full_prompt = loop_ctx.build_prompt();

                // Run as WiggumLoop iteration
                let mut iterations_used: usize = 0;
                let mut completed_by_signal = false;
                let mut phase_output = String::new();
                let max_iters = loop_config.max_iterations;

                for iter in 1..=max_iters {
                    if cancellation_token.is_cancelled() {
                        break;
                    }
                    iterations_used = iter;

                    let prompt = if iter == 1 {
                        full_prompt.clone()
                    } else {
                        build_iteration_prompt(&loop_config, iter)
                    };

                    let span = trace_span!(
                        "pipeline_phase_iter",
                        feature = feature.name.as_str(),
                        phase = phase.label(),
                        iteration = iter,
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

                    if let Some(ref out) = result {
                        phase_output = out.clone();
                        // Check for phase-specific completion signal
                        if let Some(signal) = phase.completion_signal() {
                            if check_completion_promise(out, signal) {
                                completed_by_signal = true;
                                break;
                            }
                        }
                    }
                }

                // After SpecGeneration, write feature docs from output
                if *phase == LoopPhase::SpecGeneration && !phase_output.is_empty() {
                    if let Err(e) = loop_ctx.write_feature_docs(
                        &phase_output, "", "", "",
                    ) {
                        warn!("Failed to write feature docs: {e}");
                    }
                }

                // Collect loop output
                let loop_output = LoopOutput {
                    phase: *phase,
                    output: phase_output.clone(),
                    iterations_used,
                    completed_by_signal,
                };

                // Check gate condition and capture output for summary
                let loop_result = LoopResult {
                    output: loop_output.clone(),
                    gate_passed: pipeline.check_gate(*phase, &phase_output),
                };

                // Log phase result with output summary
                let output_preview = if loop_result.output.output.len() > 200 {
                    format!("{}...", &loop_result.output.output[..200])
                } else {
                    loop_result.output.output.clone()
                };
                tracing::info!(
                    "Phase {} result: gate={}, output_preview={}",
                    phase.label(),
                    loop_result.gate_passed,
                    output_preview,
                );
                prior_loop_outputs.push(loop_output);

                // TestRun → FixIssues cycling
                if *phase == LoopPhase::TestRun && !loop_result.gate_passed {
                    for cycle in 0..pipeline.config.test_fix_max_cycles {
                        if cancellation_token.is_cancelled() {
                            break;
                        }

                        // Run FixIssues phase
                        let fix_ctx = pipeline.create_loop_context(
                        feature,
                        LoopPhase::FixIssues,
                        prior_loop_outputs.clone(),
                    );
                        let fix_prompt = fix_ctx.build_prompt();
                        let fix_config = pipeline.build_loop_config(LoopPhase::FixIssues);

                        let mut fix_output = String::new();
                        for fix_iter in 1..=fix_config.max_iterations {
                            if cancellation_token.is_cancelled() { break; }

                            let p = if fix_iter == 1 {
                                fix_prompt.clone()
                            } else {
                                build_iteration_prompt(&fix_config, fix_iter)
                            };

                            let span = trace_span!("pipeline_fix", cycle = cycle, iteration = fix_iter);
                            sess.set_server_reasoning_included(false).await;
                            let r = run_turn(
                                Arc::clone(&sess), Arc::clone(&ctx),
                                vec![UserInput::Text { text: p, text_elements: vec![] }], None,
                                cancellation_token.child_token(),
                            ).instrument(span).await;
                            if let Some(ref o) = r {
                                fix_output = o.clone();
                                if let Some(signal) = LoopPhase::FixIssues.completion_signal() {
                                    if check_completion_promise(o, signal) { break; }
                                }
                            }
                        }

                        prior_loop_outputs.push(LoopOutput {
                            phase: LoopPhase::FixIssues,
                            output: fix_output,
                            iterations_used: 1,
                            completed_by_signal: false,
                        });

                        // Re-run TestRun
                        let retest_ctx = pipeline.create_loop_context(
                            feature,
                            LoopPhase::TestRun,
                            prior_loop_outputs.clone(),
                        );
                        let retest_prompt = retest_ctx.build_prompt();
                        let span = trace_span!("pipeline_retest", cycle = cycle);
                        sess.set_server_reasoning_included(false).await;
                        let retest_result = run_turn(
                            Arc::clone(&sess), Arc::clone(&ctx),
                            vec![UserInput::Text { text: retest_prompt, text_elements: vec![] }], None,
                            cancellation_token.child_token(),
                        ).instrument(span).await;

                        if let Some(ref out) = retest_result {
                            if pipeline.check_gate(LoopPhase::TestRun, out) {
                                prior_loop_outputs.push(LoopOutput {
                                    phase: LoopPhase::TestRun,
                                    output: out.clone(),
                                    iterations_used: 1,
                                    completed_by_signal: true,
                                });
                                break;
                            }
                        }
                    }
                }

                // After DocUpdate, log completion to global log
                if *phase == LoopPhase::DocUpdate {
                    if let Err(e) = pipeline.log_feature_completion(
                        feature,
                        &phase_output,
                        "Completed via pipeline",
                        &feature.affected_files,
                    ) {
                        warn!("Failed to log feature completion: {e}");
                    }
                }
            }

            // Cleanup feature docs after completing all phases
            let cleanup_ctx = pipeline.create_loop_context(
                feature,
                LoopPhase::DocUpdate,
                vec![],
            );
            if let Err(e) = cleanup_ctx.cleanup_feature_docs() {
                warn!("Failed to cleanup feature docs: {e}");
            }

            feature_summaries.push(format!(
                "- **{}** ({} phases completed)",
                feature.name,
                prior_loop_outputs.len(),
            ));
        }

        pipeline.set_completed();

        // Read final log
        let log_content = pipeline.global_log.read().unwrap_or_default();

        Some(format!(
            "# Pipeline Complete\n\n\
             **Features processed**: {}/{total_features}\n\n\
             ## Feature Summary\n{}\n\n\
             ## Implementation Log\n{log_content}",
            feature_summaries.len(),
            feature_summaries.join("\n"),
        ))
    }
}
