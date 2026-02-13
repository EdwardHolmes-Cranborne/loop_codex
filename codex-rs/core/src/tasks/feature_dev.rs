//! FeatureDevTask: 7-phase structured feature development workflow.
//!
//! Orchestrates: Discovery → Exploration → Clarification → Architecture →
//! Implementation → Review → Summary using specialized subagents per phase.

use serde::Deserialize;
use serde::Serialize;

/// The 7 phases of feature development.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FeatureDevPhase {
    /// Phase 1: Parse user request, identify scope and requirements.
    Discovery,
    /// Phase 2: Explore codebase for relevant files, patterns, dependencies.
    Exploration,
    /// Phase 3: Ask clarifying questions (may be skipped if requirements are clear).
    Clarification,
    /// Phase 4: Design the solution architecture and implementation plan.
    Architecture,
    /// Phase 5: Implement the feature (write code, run tests).
    Implementation,
    /// Phase 6: Review the implementation for quality and correctness.
    Review,
    /// Phase 7: Summarize all changes made during the workflow.
    Summary,
}

impl FeatureDevPhase {
    /// Returns all phases in order.
    pub fn all() -> &'static [FeatureDevPhase] {
        &[
            Self::Discovery,
            Self::Exploration,
            Self::Clarification,
            Self::Architecture,
            Self::Implementation,
            Self::Review,
            Self::Summary,
        ]
    }

    /// Human-readable name for the phase.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Discovery => "Discovery",
            Self::Exploration => "Codebase Exploration",
            Self::Clarification => "Clarifying Questions",
            Self::Architecture => "Architecture Design",
            Self::Implementation => "Implementation",
            Self::Review => "Quality Review",
            Self::Summary => "Summary",
        }
    }

    /// Name of the specialized subagent for this phase (if applicable).
    pub fn agent_role(&self) -> Option<&'static str> {
        match self {
            Self::Exploration => Some("code-explorer"),
            Self::Architecture => Some("code-architect"),
            Self::Review => Some("code-reviewer"),
            _ => None,
        }
    }
}

/// Configuration for a feature development workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureDevConfig {
    /// The user's feature request / description.
    pub feature_request: String,
    /// Number of parallel explorer agents for Phase 2.
    #[serde(default = "default_explorer_count")]
    pub explorer_count: usize,
    /// Number of parallel reviewer agents for Phase 6.
    #[serde(default = "default_reviewer_count")]
    pub reviewer_count: usize,
    /// Whether to skip the clarification phase.
    #[serde(default)]
    pub skip_clarification: bool,
}

fn default_explorer_count() -> usize {
    3
}

fn default_reviewer_count() -> usize {
    2
}

/// Status / progress of the feature development workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureDevStatus {
    /// Current phase.
    pub current_phase: FeatureDevPhase,
    /// Output collected from completed phases.
    pub phase_outputs: Vec<PhaseOutput>,
}

/// Output from a completed phase.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseOutput {
    pub phase: FeatureDevPhase,
    pub output: String,
}

// ----- System Prompt Templates -----

/// System prompt for the code explorer agent (Phase 2).
pub const EXPLORER_PROMPT: &str = "\
You are a code explorer agent. Your job is to analyze a codebase to find files, patterns, \
and dependencies relevant to a feature request. \n\n\
Instructions:\n\
1. Read file trees and search for relevant code\n\
2. Identify key files that will be modified or referenced\n\
3. Note existing patterns, conventions, and architectures\n\
4. Report your findings as structured JSON with: relevant_files, patterns_found, dependencies\n\n\
Focus only on exploration — do NOT write any code or make changes.";

/// System prompt for the code architect agent (Phase 4).
pub const ARCHITECT_PROMPT: &str = "\
You are a code architect agent. Your job is to design the implementation plan for a feature. \n\n\
You will receive:\n\
- The feature request\n\
- Exploration results from code-explorer agents\n\
- Any clarifications from the user\n\n\
Instructions:\n\
1. Design the solution architecture\n\
2. Specify which files to create, modify, or delete\n\
3. Define the implementation order (dependencies first)\n\
4. Identify potential risks and edge cases\n\
5. Output a structured implementation plan\n\n\
Focus only on design — do NOT write implementation code.";

/// System prompt for the code reviewer agent (Phase 6).
pub const REVIEWER_PROMPT: &str = "\
You are a code reviewer agent. Your job is to review code changes for quality and correctness.\n\n\
Instructions:\n\
1. Check for bugs, logic errors, and edge cases\n\
2. Verify adherence to project conventions and patterns\n\
3. Check test coverage and quality\n\
4. Look for security issues or performance problems\n\
5. Output findings as structured JSON with: severity, file, line, description, suggestion\n\n\
Be thorough but fair. Only flag genuine issues.";

/// Build the prompt for a specific phase, incorporating prior phase outputs.
pub fn build_phase_prompt(
    config: &FeatureDevConfig,
    phase: FeatureDevPhase,
    prior_outputs: &[PhaseOutput],
) -> String {
    let context: String = prior_outputs
        .iter()
        .map(|o| format!("## {} Output\n{}\n", o.phase.label(), o.output))
        .collect::<Vec<_>>()
        .join("\n");

    match phase {
        FeatureDevPhase::Discovery => {
            format!(
                "Analyze this feature request and identify the scope, requirements, \
                 and acceptance criteria.\n\nFEATURE REQUEST:\n{}",
                config.feature_request
            )
        }
        FeatureDevPhase::Exploration => {
            format!(
                "{EXPLORER_PROMPT}\n\n---\nFEATURE REQUEST:\n{}\n\n{context}",
                config.feature_request
            )
        }
        FeatureDevPhase::Clarification => {
            format!(
                "Based on the feature request and exploration results below, generate \
                 a list of clarifying questions for the user. If no clarification is needed, \
                 respond with \"NO_QUESTIONS_NEEDED\".\n\n\
                 FEATURE REQUEST:\n{}\n\n{context}",
                config.feature_request
            )
        }
        FeatureDevPhase::Architecture => {
            format!(
                "{ARCHITECT_PROMPT}\n\n---\nFEATURE REQUEST:\n{}\n\n{context}",
                config.feature_request
            )
        }
        FeatureDevPhase::Implementation => {
            format!(
                "Implement the feature according to the architecture plan below. \
                 Write all necessary code, tests, and documentation.\n\n\
                 FEATURE REQUEST:\n{}\n\n{context}",
                config.feature_request
            )
        }
        FeatureDevPhase::Review => {
            format!(
                "{REVIEWER_PROMPT}\n\n---\nFEATURE REQUEST:\n{}\n\n{context}",
                config.feature_request
            )
        }
        FeatureDevPhase::Summary => {
            format!(
                "Summarize all changes made during this feature development workflow. \
                 Include: files created/modified, key design decisions, test results, \
                 and any caveats or follow-up items.\n\n\
                 FEATURE REQUEST:\n{}\n\n{context}",
                config.feature_request
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_ordering() {
        let phases = FeatureDevPhase::all();
        assert_eq!(phases.len(), 7);
        assert_eq!(phases[0], FeatureDevPhase::Discovery);
        assert_eq!(phases[6], FeatureDevPhase::Summary);
    }

    #[test]
    fn agent_roles() {
        assert_eq!(
            FeatureDevPhase::Exploration.agent_role(),
            Some("code-explorer")
        );
        assert_eq!(
            FeatureDevPhase::Architecture.agent_role(),
            Some("code-architect")
        );
        assert_eq!(
            FeatureDevPhase::Review.agent_role(),
            Some("code-reviewer")
        );
        assert!(FeatureDevPhase::Discovery.agent_role().is_none());
        assert!(FeatureDevPhase::Implementation.agent_role().is_none());
    }

    #[test]
    fn build_phase_prompt_includes_feature_request() {
        let config = FeatureDevConfig {
            feature_request: "Add user authentication".to_string(),
            explorer_count: 3,
            reviewer_count: 2,
            skip_clarification: false,
        };

        for phase in FeatureDevPhase::all() {
            let prompt = build_phase_prompt(&config, *phase, &[]);
            assert!(
                prompt.contains("Add user authentication"),
                "Phase {:?} prompt should contain feature request",
                phase
            );
        }
    }

    #[test]
    fn build_phase_prompt_incorporates_prior_outputs() {
        let config = FeatureDevConfig {
            feature_request: "Add auth".to_string(),
            explorer_count: 3,
            reviewer_count: 2,
            skip_clarification: false,
        };

        let prior = vec![PhaseOutput {
            phase: FeatureDevPhase::Discovery,
            output: "Scope: JWT-based auth".to_string(),
        }];

        let prompt = build_phase_prompt(&config, FeatureDevPhase::Exploration, &prior);
        assert!(prompt.contains("JWT-based auth"));
        assert!(prompt.contains("Discovery Output"));
    }

    #[test]
    fn phase_labels() {
        assert_eq!(FeatureDevPhase::Discovery.label(), "Discovery");
        assert_eq!(FeatureDevPhase::Summary.label(), "Summary");
        assert_eq!(
            FeatureDevPhase::Clarification.label(),
            "Clarifying Questions"
        );
    }
}
