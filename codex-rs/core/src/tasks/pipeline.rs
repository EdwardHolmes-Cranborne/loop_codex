//! Feature Pipeline Orchestrator — TDD-driven multi-loop feature development.
//!
//! Drives each feature through a pipeline of gated WiggumLoops:
//!   SpecGeneration → TestWrite → TestReview → Implement → TestRun →
//!   FixIssues (cycle) → ReviewCommit → DocUpdate
//!
//! Each loop has isolated context, can spawn its own agent swarm, and
//! produces output that flows as read-only context to subsequent loops.

use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::Serialize;

use super::global_log::GlobalImplementationLog;
use super::loop_context::{FeatureItem, LoopContext, LoopOutput};
use super::loop_types::LoopPhase;
use super::safety_guard::SafetyGuard;
use super::wiggum_loop::WiggumLoopConfig;

/// Configuration for the feature development pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Maximum iterations for any single loop (can be overridden per phase).
    #[serde(default = "default_max_iterations")]
    pub max_iterations_per_loop: usize,

    /// Maximum test → fix cycles before giving up.
    #[serde(default = "default_test_fix_max_cycles")]
    pub test_fix_max_cycles: usize,

    /// Automatically commit after ReviewCommit phase.
    #[serde(default = "default_true")]
    pub auto_commit: bool,

    /// Base directory for pipeline docs (per-feature ephemeral docs).
    #[serde(default = "default_docs_base_dir")]
    pub docs_base_dir: PathBuf,

    /// Whether the safety guard is enabled (--safe flag).
    #[serde(default)]
    pub safety_guard_enabled: bool,
}

fn default_max_iterations() -> usize {
    25
}
fn default_test_fix_max_cycles() -> usize {
    5
}
fn default_true() -> bool {
    true
}
fn default_docs_base_dir() -> PathBuf {
    PathBuf::from(".codex/pipeline")
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_iterations_per_loop: default_max_iterations(),
            test_fix_max_cycles: default_test_fix_max_cycles(),
            auto_commit: default_true(),
            docs_base_dir: default_docs_base_dir(),
            safety_guard_enabled: false,
        }
    }
}

/// Status of the overall pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PipelineStatus {
    /// Not yet started.
    Pending,
    /// Currently executing feature at the given index.
    Running { current_feature_index: usize },
    /// A feature loop has failed and the pipeline is halted.
    Failed {
        feature_index: usize,
        phase: LoopPhase,
        reason: String,
    },
    /// All features completed successfully.
    Completed,
}

/// Result of a single loop execution.
#[derive(Debug, Clone)]
pub struct LoopResult {
    /// Output from the loop.
    pub output: LoopOutput,
    /// Whether the loop's gate condition was met.
    pub gate_passed: bool,
}

/// The top-level pipeline orchestrator.
///
/// Parses an implementation plan into features and drives each through the
/// full TDD pipeline of WiggumLoops.
#[derive(Debug, Clone)]
pub struct FeaturePipeline {
    /// Pipeline configuration.
    pub config: PipelineConfig,
    /// Features to implement, in order.
    pub features: Vec<FeatureItem>,
    /// Path to the global implementation log.
    pub global_log: GlobalImplementationLog,
    /// Current status.
    pub status: PipelineStatus,
    /// Safety guard for shell commands.
    pub safety_guard: SafetyGuard,
}

impl FeaturePipeline {
    /// Create a new pipeline from a list of features.
    pub fn new(
        features: Vec<FeatureItem>,
        config: PipelineConfig,
        project_root: &Path,
    ) -> Self {
        let safety_guard = SafetyGuard::new(config.safety_guard_enabled);
        Self {
            global_log: GlobalImplementationLog::new(project_root),
            config,
            features,
            status: PipelineStatus::Pending,
            safety_guard,
        }
    }

    /// Total number of features in the pipeline.
    pub fn feature_count(&self) -> usize {
        self.features.len()
    }

    /// Get the ordered list of phases for a single feature.
    ///
    /// This returns the "happy path" — the actual execution may cycle
    /// through TestRun/FixIssues multiple times.
    pub fn feature_phases() -> &'static [LoopPhase] {
        LoopPhase::all()
    }

    /// Build a WiggumLoopConfig for a specific phase.
    pub fn build_loop_config(&self, phase: LoopPhase) -> WiggumLoopConfig {
        let max_iter = std::cmp::min(
            self.config.max_iterations_per_loop,
            phase.default_max_iterations(),
        );

        WiggumLoopConfig {
            prompt: phase.system_prompt().to_string(),
            max_iterations: max_iter,
            completion_promise: phase
                .completion_signal()
                .unwrap_or("PHASE_COMPLETE")
                .to_string(),
        }
    }

    /// Create a LoopContext for a feature at a specific phase.
    pub fn create_loop_context(
        &self,
        feature: &FeatureItem,
        phase: LoopPhase,
        prior_outputs: Vec<LoopOutput>,
    ) -> LoopContext {
        LoopContext::new(
            &self.config.docs_base_dir,
            feature.clone(),
            phase,
            prior_outputs,
        )
    }

    /// Check if a loop's output satisfies the gate condition.
    pub fn check_gate(&self, phase: LoopPhase, output: &str) -> bool {
        match phase.completion_signal() {
            Some(signal) => output.contains(signal),
            // TestRun gate: check for test pass indicators
            None => {
                let lower = output.to_lowercase();
                (lower.contains("test result: ok") || lower.contains("tests passed"))
                    && !lower.contains("failed")
            }
        }
    }

    /// Execute the full pipeline for a single feature.
    ///
    /// Returns the ordered list of loop outputs from each phase.
    /// This is a synchronous orchestration method — the actual async
    /// execution happens through the SessionTask trait implementation.
    pub fn plan_feature_execution(&self, feature: &FeatureItem) -> FeatureExecutionPlan {
        FeatureExecutionPlan {
            feature: feature.clone(),
            test_fix_max_cycles: self.config.test_fix_max_cycles,
        }
    }

    /// Check a shell command through the safety guard.
    pub fn check_command_safety(&self, command: &str) -> super::safety_guard::SafetyVerdict {
        self.safety_guard.check(command)
    }

    /// Log a completed feature to the global implementation log.
    pub fn log_feature_completion(
        &self,
        feature: &FeatureItem,
        summary: &str,
        lessons: &str,
        files_changed: &[String],
    ) -> io::Result<()> {
        self.global_log
            .log_feature_completion(feature, summary, lessons, files_changed)
    }

    /// Mark the pipeline as running at a specific feature index.
    pub fn set_running(&mut self, feature_index: usize) {
        self.status = PipelineStatus::Running {
            current_feature_index: feature_index,
        };
    }

    /// Mark the pipeline as failed.
    pub fn set_failed(&mut self, feature_index: usize, phase: LoopPhase, reason: String) {
        self.status = PipelineStatus::Failed {
            feature_index,
            phase,
            reason,
        };
    }

    /// Mark the pipeline as completed.
    pub fn set_completed(&mut self) {
        self.status = PipelineStatus::Completed;
    }
}

/// Execution plan for a single feature, describing the phase sequence
/// including potential test-fix cycles.
#[derive(Debug, Clone)]
pub struct FeatureExecutionPlan {
    /// The feature to execute.
    pub feature: FeatureItem,
    /// Maximum test→fix cycles.
    pub test_fix_max_cycles: usize,
}

impl FeatureExecutionPlan {
    /// Generate the ordered sequence of phases to execute.
    ///
    /// This is the "expanded" sequence including worst-case test-fix cycles.
    /// The actual execution can short-circuit if tests pass on the first try.
    pub fn max_phase_sequence(&self) -> Vec<LoopPhase> {
        let mut phases = vec![
            LoopPhase::SpecGeneration,
            LoopPhase::TestWrite,
            LoopPhase::TestReview,
            LoopPhase::Implement,
        ];

        // TestRun + FixIssues cycles
        for _ in 0..self.test_fix_max_cycles {
            phases.push(LoopPhase::TestRun);
            phases.push(LoopPhase::FixIssues);
        }
        // Final TestRun if all fix cycles used
        phases.push(LoopPhase::TestRun);

        phases.push(LoopPhase::ReviewCommit);
        phases.push(LoopPhase::DocUpdate);

        phases
    }

    /// Describe the execution plan as a human-readable summary.
    pub fn describe(&self) -> String {
        format!(
            "Feature: {}\n\
            Phases: {}\n\
            Max test-fix cycles: {}\n\
            Total max phases: {}",
            self.feature.name,
            FeaturePipeline::feature_phases()
                .iter()
                .map(|p| p.label())
                .collect::<Vec<_>>()
                .join(" → "),
            self.test_fix_max_cycles,
            self.max_phase_sequence().len(),
        )
    }
}

/// Parse an implementation plan text into a list of features.
///
/// Expects a markdown document with features as H2 headings (## Feature Name)
/// followed by description text.
pub fn parse_implementation_plan(plan_text: &str) -> Vec<FeatureItem> {
    let mut features = Vec::new();
    let mut current_name: Option<String> = None;
    let mut current_desc = String::new();
    let mut index = 0;

    for line in plan_text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            // Save previous feature
            if let Some(name) = current_name.take() {
                features.push(FeatureItem {
                    name,
                    description: current_desc.trim().to_string(),
                    affected_files: vec![],
                    index,
                });
                index += 1;
                current_desc.clear();
            }
            current_name = Some(heading.trim().to_string());
        } else if current_name.is_some() {
            current_desc.push_str(line);
            current_desc.push('\n');
        }
    }

    // Save last feature
    if let Some(name) = current_name {
        features.push(FeatureItem {
            name,
            description: current_desc.trim().to_string(),
            affected_files: vec![],
            index,
        });
    }

    features
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn default_config() {
        let config = PipelineConfig::default();
        assert_eq!(config.max_iterations_per_loop, 25);
        assert_eq!(config.test_fix_max_cycles, 5);
        assert!(config.auto_commit);
        assert!(!config.safety_guard_enabled);
    }

    #[test]
    fn new_pipeline() {
        let tmp = TempDir::new().unwrap();
        let features = vec![FeatureItem {
            name: "Auth".to_string(),
            description: "Add auth".to_string(),
            affected_files: vec![],
            index: 0,
        }];
        let pipeline = FeaturePipeline::new(features, PipelineConfig::default(), tmp.path());

        assert_eq!(pipeline.feature_count(), 1);
        assert_eq!(pipeline.status, PipelineStatus::Pending);
    }

    #[test]
    fn pipeline_status_transitions() {
        let tmp = TempDir::new().unwrap();
        let mut pipeline = FeaturePipeline::new(vec![], PipelineConfig::default(), tmp.path());

        assert_eq!(pipeline.status, PipelineStatus::Pending);

        pipeline.set_running(0);
        assert_eq!(
            pipeline.status,
            PipelineStatus::Running {
                current_feature_index: 0
            }
        );

        pipeline.set_failed(0, LoopPhase::TestRun, "tests failed".into());
        assert!(matches!(pipeline.status, PipelineStatus::Failed { .. }));

        pipeline.set_completed();
        assert_eq!(pipeline.status, PipelineStatus::Completed);
    }

    #[test]
    fn build_loop_config() {
        let tmp = TempDir::new().unwrap();
        let pipeline = FeaturePipeline::new(vec![], PipelineConfig::default(), tmp.path());

        let config = pipeline.build_loop_config(LoopPhase::Implement);
        assert_eq!(config.max_iterations, 25);
        assert_eq!(config.completion_promise, "IMPLEMENTATION_COMPLETE");

        let config = pipeline.build_loop_config(LoopPhase::DocUpdate);
        assert_eq!(config.max_iterations, 5);
        assert_eq!(config.completion_promise, "DOC_UPDATE_COMPLETE");
    }

    #[test]
    fn check_gate_signal() {
        let tmp = TempDir::new().unwrap();
        let pipeline = FeaturePipeline::new(vec![], PipelineConfig::default(), tmp.path());

        assert!(pipeline.check_gate(
            LoopPhase::Implement,
            "Done! IMPLEMENTATION_COMPLETE"
        ));
        assert!(!pipeline.check_gate(LoopPhase::Implement, "Still working..."));
    }

    #[test]
    fn check_gate_test_run() {
        let tmp = TempDir::new().unwrap();
        let pipeline = FeaturePipeline::new(vec![], PipelineConfig::default(), tmp.path());

        assert!(pipeline.check_gate(LoopPhase::TestRun, "test result: ok. 5 tests passed"));
        assert!(!pipeline.check_gate(LoopPhase::TestRun, "test result: FAILED. 2 tests failed"));
    }

    #[test]
    fn feature_execution_plan() {
        let feature = FeatureItem {
            name: "Auth".to_string(),
            description: "Add auth".to_string(),
            affected_files: vec![],
            index: 0,
        };
        let tmp = TempDir::new().unwrap();
        let pipeline = FeaturePipeline::new(vec![feature.clone()], PipelineConfig::default(), tmp.path());
        let plan = pipeline.plan_feature_execution(&feature);

        let description = plan.describe();
        assert!(description.contains("Auth"));
        assert!(description.contains("Max test-fix cycles: 5"));

        let phases = plan.max_phase_sequence();
        // SpecGen + TestWrite + TestReview + Implement + 5*(TestRun+Fix) + TestRun + ReviewCommit + DocUpdate
        assert_eq!(phases.len(), 4 + 5 * 2 + 1 + 2);
    }

    #[test]
    fn parse_implementation_plan() {
        let plan = "\
# My Project Plan

## Add Authentication
Add JWT-based authentication to all API endpoints.
This should use the existing middleware pattern.

## Add Logging
Structured logging with tracing crate.
Include request IDs.

## Add Rate Limiting
Token bucket rate limiter on public endpoints.
";
        let features = super::parse_implementation_plan(plan);
        assert_eq!(features.len(), 3);
        assert_eq!(features[0].name, "Add Authentication");
        assert!(features[0].description.contains("JWT-based"));
        assert_eq!(features[0].index, 0);
        assert_eq!(features[1].name, "Add Logging");
        assert_eq!(features[1].index, 1);
        assert_eq!(features[2].name, "Add Rate Limiting");
        assert_eq!(features[2].index, 2);
    }

    #[test]
    fn parse_empty_plan() {
        let features = super::parse_implementation_plan("");
        assert!(features.is_empty());
    }

    #[test]
    fn parse_plan_no_features() {
        let features = super::parse_implementation_plan("# Just a title\nSome text.");
        assert!(features.is_empty());
    }

    #[test]
    fn safety_guard_integration() {
        let tmp = TempDir::new().unwrap();
        let mut config = PipelineConfig::default();
        config.safety_guard_enabled = true;
        let pipeline = FeaturePipeline::new(vec![], config, tmp.path());

        let verdict = pipeline.check_command_safety("rm -rf /");
        assert!(!verdict.safe);

        let verdict = pipeline.check_command_safety("cargo test");
        assert!(verdict.safe);
    }
}
