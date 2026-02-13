//! Loop phase types for the Feature Pipeline Orchestrator.
//!
//! Each phase represents a distinct step in the TDD-driven feature development
//! pipeline. Each phase runs as a WiggumLoop with its own agent swarm and
//! context.

use serde::Deserialize;
use serde::Serialize;

/// The phases of the feature development pipeline.
///
/// Each feature passes through these phases in order, with the TestRun/FixIssues
/// phases potentially cycling until tests pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LoopPhase {
    /// Generate feature spec, requirements, implementation plan, and subtasks.
    SpecGeneration,
    /// Write tests before implementation (TDD).
    TestWrite,
    /// Review the written tests for quality and correctness.
    TestReview,
    /// Implement the feature according to the plan.
    Implement,
    /// Run the test suite.
    TestRun,
    /// Fix issues found by failing tests.
    FixIssues,
    /// Review all changes and create a git commit.
    ReviewCommit,
    /// Update the global implementation log with summary and lessons learned.
    DocUpdate,
}

impl LoopPhase {
    /// All phases in pipeline order.
    pub fn all() -> &'static [LoopPhase] {
        &[
            Self::SpecGeneration,
            Self::TestWrite,
            Self::TestReview,
            Self::Implement,
            Self::TestRun,
            Self::FixIssues,
            Self::ReviewCommit,
            Self::DocUpdate,
        ]
    }

    /// Human-readable label.
    pub fn label(&self) -> &'static str {
        match self {
            Self::SpecGeneration => "Spec Generation",
            Self::TestWrite => "Test Writing (TDD)",
            Self::TestReview => "Test Review",
            Self::Implement => "Implementation",
            Self::TestRun => "Test Execution",
            Self::FixIssues => "Fix Issues",
            Self::ReviewCommit => "Review & Commit",
            Self::DocUpdate => "Documentation Update",
        }
    }

    /// The completion signal string for this phase.
    ///
    /// TestRun uses exit code checking instead of a signal string.
    pub fn completion_signal(&self) -> Option<&'static str> {
        match self {
            Self::SpecGeneration => Some("SPEC_GENERATION_COMPLETE"),
            Self::TestWrite => Some("TEST_WRITING_COMPLETE"),
            Self::TestReview => Some("REVIEW_PASSED"),
            Self::Implement => Some("IMPLEMENTATION_COMPLETE"),
            Self::TestRun => None, // uses exit code
            Self::FixIssues => Some("FIX_COMPLETE"),
            Self::ReviewCommit => Some("REVIEW_APPROVED"),
            Self::DocUpdate => Some("DOC_UPDATE_COMPLETE"),
        }
    }

    /// Default max iterations for this phase.
    pub fn default_max_iterations(&self) -> usize {
        match self {
            Self::SpecGeneration => 10,
            Self::TestWrite => 15,
            Self::TestReview => 10,
            Self::Implement => 25,
            Self::TestRun => 3,
            Self::FixIssues => 15,
            Self::ReviewCommit => 10,
            Self::DocUpdate => 5,
        }
    }

    /// System prompt for the agent handling this phase.
    pub fn system_prompt(&self) -> &'static str {
        match self {
            Self::SpecGeneration => SPEC_GENERATION_PROMPT,
            Self::TestWrite => TEST_WRITE_PROMPT,
            Self::TestReview => TEST_REVIEW_PROMPT,
            Self::Implement => IMPLEMENT_PROMPT,
            Self::TestRun => TEST_RUN_PROMPT,
            Self::FixIssues => FIX_ISSUES_PROMPT,
            Self::ReviewCommit => REVIEW_COMMIT_PROMPT,
            Self::DocUpdate => DOC_UPDATE_PROMPT,
        }
    }
}

// ----- Phase System Prompts -----

const SPEC_GENERATION_PROMPT: &str = "\
You are a spec generation agent. Create detailed documentation for implementing a feature.\n\n\
Generate the following files in the feature docs directory:\n\
1. feature_spec.md — Detailed specification of what to build\n\
2. requirements.md — Acceptance criteria and constraints\n\
3. implementation_plan.md — Step-by-step implementation plan\n\
4. subtasks.md — Checklist of subtasks to complete\n\n\
When all files are created and complete, include SPEC_GENERATION_COMPLETE in your response.";

const TEST_WRITE_PROMPT: &str = "\
You are a test-first development agent. Write tests BEFORE implementation.\n\n\
Based on the feature spec and requirements:\n\
1. Create test files following the project's test conventions\n\
2. Write comprehensive unit tests covering all acceptance criteria\n\
3. Include edge cases and error handling tests\n\
4. Tests should compile but are expected to fail (no implementation yet)\n\n\
When all tests are written, include TEST_WRITING_COMPLETE in your response.";

const TEST_REVIEW_PROMPT: &str = "\
You are a test review agent. Review the tests for quality and completeness.\n\n\
Check:\n\
1. Test coverage of all requirements and acceptance criteria\n\
2. Edge cases and error handling coverage\n\
3. Test naming conventions and readability\n\
4. Test isolation (no shared mutable state between tests)\n\
5. Assertion quality (specific, not just 'is not null')\n\n\
If tests pass review, include REVIEW_PASSED in your response.\n\
If changes are needed, describe what needs fixing — do NOT include REVIEW_PASSED.";

const IMPLEMENT_PROMPT: &str = "\
You are an implementation agent. Implement the feature according to the spec and plan.\n\n\
Follow:\n\
1. The implementation plan and subtask checklist\n\
2. Project conventions and patterns\n\
3. Make the previously-written tests pass\n\
4. Write clean, documented code\n\n\
When implementation is complete, include IMPLEMENTATION_COMPLETE in your response.";

const TEST_RUN_PROMPT: &str = "\
You are a test execution agent. Run the test suite and report results.\n\n\
1. Run the relevant test commands for this feature\n\
2. Capture all output including failures and errors\n\
3. Report a clear summary: total tests, passed, failed, errors\n\n\
Output the raw test command output followed by your analysis.";

const FIX_ISSUES_PROMPT: &str = "\
You are a bug-fix agent. Fix the failing tests.\n\n\
You have:\n\
- The test output showing failures\n\
- The implementation code\n\
- The feature spec and requirements\n\n\
Fix the issues causing test failures. Do NOT modify the tests unless they contain \
genuine bugs — the tests define the expected behavior.\n\n\
When all fixes are applied, include FIX_COMPLETE in your response.";

const REVIEW_COMMIT_PROMPT: &str = "\
You are a review and commit agent. Review all changes and create a git commit.\n\n\
1. Review all modified/created files for quality\n\
2. Check for accidental debug code, TODOs, or incomplete work\n\
3. Verify the implementation matches the spec\n\
4. Create a descriptive git commit with conventional commit format\n\n\
If everything looks good and commit is created, include REVIEW_APPROVED in your response.\n\
If changes are needed, describe what needs fixing — do NOT include REVIEW_APPROVED.";

const DOC_UPDATE_PROMPT: &str = "\
You are a documentation agent. Update the global implementation log.\n\n\
Add an entry to implementation_log.md with:\n\
1. Feature name and date\n\
2. Summary of changes made\n\
3. Files created/modified\n\
4. Lessons learned during implementation\n\
5. Any caveats or follow-up items\n\n\
When the log is updated, include DOC_UPDATE_COMPLETE in your response.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_phases_covered() {
        assert_eq!(LoopPhase::all().len(), 8);
    }

    #[test]
    fn phase_labels_non_empty() {
        for phase in LoopPhase::all() {
            assert!(!phase.label().is_empty());
        }
    }

    #[test]
    fn completion_signals() {
        // TestRun uses exit code, not a signal string
        assert!(LoopPhase::TestRun.completion_signal().is_none());
        // All others have signals
        for phase in LoopPhase::all() {
            if *phase != LoopPhase::TestRun {
                assert!(phase.completion_signal().is_some(), "{:?} missing signal", phase);
            }
        }
    }

    #[test]
    fn system_prompts_exist() {
        for phase in LoopPhase::all() {
            assert!(!phase.system_prompt().is_empty());
        }
    }

    #[test]
    fn max_iterations_positive() {
        for phase in LoopPhase::all() {
            assert!(phase.default_max_iterations() > 0);
        }
    }
}
