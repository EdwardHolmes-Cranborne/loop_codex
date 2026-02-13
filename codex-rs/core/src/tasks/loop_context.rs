//! Context isolation and ephemeral document lifecycle for pipeline loops.
//!
//! Each feature in the pipeline gets its own docs directory that is created
//! at the start and cleaned up after the feature completes. The only persistent
//! artifact is the global implementation log.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde::Serialize;

use super::loop_types::LoopPhase;

/// A single feature extracted from an implementation plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureItem {
    /// Short name / slug for the feature (used in directory names).
    pub name: String,
    /// Full description of the feature.
    pub description: String,
    /// Files expected to be affected.
    #[serde(default)]
    pub affected_files: Vec<String>,
    /// Index in the overall plan (0-based).
    pub index: usize,
}

impl FeatureItem {
    /// Generate a filesystem-safe slug from the feature name.
    pub fn slug(&self) -> String {
        self.name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' })
            .collect::<String>()
            .trim_matches('_')
            .to_string()
    }
}

/// Output from a completed loop phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopOutput {
    /// Which phase produced this output.
    pub phase: LoopPhase,
    /// The agent's final output text.
    pub output: String,
    /// How many iterations were used.
    pub iterations_used: usize,
    /// Whether the phase completed via its completion signal.
    pub completed_by_signal: bool,
}

/// Isolated context for a single loop execution within the pipeline.
///
/// Each loop gets its own context with read-only access to prior loop outputs
/// and read-write access to the feature docs directory.
#[derive(Debug, Clone)]
pub struct LoopContext {
    /// Root docs directory for this feature (e.g. .codex/pipeline/add-auth/)
    pub docs_dir: PathBuf,
    /// The feature being worked on.
    pub feature: FeatureItem,
    /// Current phase.
    pub phase: LoopPhase,
    /// Outputs from prior loops (read-only context for the agent).
    pub prior_loop_outputs: Vec<LoopOutput>,
}

/// Standard doc filenames within a feature's docs directory.
pub const FEATURE_SPEC_FILE: &str = "feature_spec.md";
pub const REQUIREMENTS_FILE: &str = "requirements.md";
pub const IMPL_PLAN_FILE: &str = "implementation_plan.md";
pub const SUBTASKS_FILE: &str = "subtasks.md";

impl LoopContext {
    /// Create a new loop context for a feature and phase.
    pub fn new(
        pipeline_docs_base: &Path,
        feature: FeatureItem,
        phase: LoopPhase,
        prior_outputs: Vec<LoopOutput>,
    ) -> Self {
        let docs_dir = pipeline_docs_base.join(feature.slug());
        Self {
            docs_dir,
            feature,
            phase,
            prior_loop_outputs: prior_outputs,
        }
    }

    /// Create the feature docs directory.
    pub fn create_feature_docs_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.docs_dir)
    }

    /// Write the initial feature spec stub files.
    ///
    /// Called after SpecGeneration phase completes. These files are then
    /// read by subsequent phases.
    pub fn write_feature_docs(
        &self,
        spec: &str,
        requirements: &str,
        plan: &str,
        subtasks: &str,
    ) -> io::Result<()> {
        self.create_feature_docs_dir()?;

        write_doc(&self.docs_dir.join(FEATURE_SPEC_FILE), spec)?;
        write_doc(&self.docs_dir.join(REQUIREMENTS_FILE), requirements)?;
        write_doc(&self.docs_dir.join(IMPL_PLAN_FILE), plan)?;
        write_doc(&self.docs_dir.join(SUBTASKS_FILE), subtasks)?;

        Ok(())
    }

    /// Read all feature docs back as context for the agent.
    pub fn read_feature_docs_context(&self) -> String {
        let mut ctx = String::new();

        for (label, file) in &[
            ("Feature Spec", FEATURE_SPEC_FILE),
            ("Requirements", REQUIREMENTS_FILE),
            ("Implementation Plan", IMPL_PLAN_FILE),
            ("Subtasks", SUBTASKS_FILE),
        ] {
            let path = self.docs_dir.join(file);
            if let Ok(content) = fs::read_to_string(&path) {
                ctx.push_str(&format!("## {label}\n{content}\n\n"));
            }
        }

        ctx
    }

    /// Build the full prompt for the current phase.
    ///
    /// Combines the phase system prompt with feature context and prior outputs.
    pub fn build_prompt(&self) -> String {
        let phase_prompt = self.phase.system_prompt();
        let feature_docs = self.read_feature_docs_context();
        let prior_context = self.format_prior_outputs();

        format!(
            "{phase_prompt}\n\n\
            ---\n\
            FEATURE: {name}\n\
            DESCRIPTION: {desc}\n\n\
            {feature_docs}\
            {prior_context}",
            name = self.feature.name,
            desc = self.feature.description,
        )
    }

    /// Format prior loop outputs as context for the agent.
    fn format_prior_outputs(&self) -> String {
        if self.prior_loop_outputs.is_empty() {
            return String::new();
        }

        let mut out = String::from("## Prior Phase Outputs\n\n");
        for lo in &self.prior_loop_outputs {
            out.push_str(&format!(
                "### {} (iterations: {}, completed: {})\n{}\n\n",
                lo.phase.label(),
                lo.iterations_used,
                lo.completed_by_signal,
                lo.output,
            ));
        }
        out
    }

    /// Delete all feature-specific docs after the pipeline completes.
    pub fn cleanup_feature_docs(&self) -> io::Result<()> {
        if self.docs_dir.exists() {
            fs::remove_dir_all(&self.docs_dir)?;
        }
        Ok(())
    }
}

fn write_doc(path: &Path, content: &str) -> io::Result<()> {
    let mut f = fs::File::create(path)?;
    f.write_all(content.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_feature() -> FeatureItem {
        FeatureItem {
            name: "Add User Auth".to_string(),
            description: "Add JWT-based user authentication".to_string(),
            affected_files: vec!["auth.rs".into(), "main.rs".into()],
            index: 0,
        }
    }

    #[test]
    fn feature_slug() {
        let feat = test_feature();
        assert_eq!(feat.slug(), "add_user_auth");
    }

    #[test]
    fn feature_slug_special_chars() {
        let feat = FeatureItem {
            name: "Fix Bug #123 (urgent!)".to_string(),
            description: String::new(),
            affected_files: vec![],
            index: 0,
        };
        assert_eq!(feat.slug(), "fix_bug__123__urgent");
    }

    #[test]
    fn create_and_cleanup_docs() {
        let tmp = TempDir::new().unwrap();
        let ctx = LoopContext::new(
            tmp.path(),
            test_feature(),
            LoopPhase::SpecGeneration,
            vec![],
        );

        ctx.write_feature_docs("spec", "reqs", "plan", "tasks")
            .unwrap();
        assert!(ctx.docs_dir.join(FEATURE_SPEC_FILE).exists());
        assert!(ctx.docs_dir.join(REQUIREMENTS_FILE).exists());
        assert!(ctx.docs_dir.join(IMPL_PLAN_FILE).exists());
        assert!(ctx.docs_dir.join(SUBTASKS_FILE).exists());

        ctx.cleanup_feature_docs().unwrap();
        assert!(!ctx.docs_dir.exists());
    }

    #[test]
    fn read_feature_docs_context() {
        let tmp = TempDir::new().unwrap();
        let ctx = LoopContext::new(
            tmp.path(),
            test_feature(),
            LoopPhase::Implement,
            vec![],
        );
        ctx.write_feature_docs("the spec", "the reqs", "the plan", "the tasks")
            .unwrap();

        let context = ctx.read_feature_docs_context();
        assert!(context.contains("the spec"));
        assert!(context.contains("the reqs"));
        assert!(context.contains("the plan"));
        assert!(context.contains("the tasks"));
    }

    #[test]
    fn build_prompt_includes_feature_info() {
        let tmp = TempDir::new().unwrap();
        let ctx = LoopContext::new(
            tmp.path(),
            test_feature(),
            LoopPhase::Implement,
            vec![LoopOutput {
                phase: LoopPhase::TestWrite,
                output: "Created 5 test cases".to_string(),
                iterations_used: 3,
                completed_by_signal: true,
            }],
        );
        ctx.create_feature_docs_dir().unwrap();

        let prompt = ctx.build_prompt();
        assert!(prompt.contains("Add User Auth"));
        assert!(prompt.contains("JWT-based"));
        assert!(prompt.contains("Created 5 test cases"));
        assert!(prompt.contains("Test Writing (TDD)"));
    }

    #[test]
    fn cleanup_nonexistent_dir_is_ok() {
        let tmp = TempDir::new().unwrap();
        let ctx = LoopContext::new(
            tmp.path(),
            test_feature(),
            LoopPhase::DocUpdate,
            vec![],
        );
        // Should not error if dir doesn't exist
        ctx.cleanup_feature_docs().unwrap();
    }
}
