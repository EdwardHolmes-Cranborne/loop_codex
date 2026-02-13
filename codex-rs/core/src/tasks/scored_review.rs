//! ScoredReviewTask: confidence-scored multi-agent code review.
//!
//! Spawns 4 parallel reviewer sub-agents, each focusing on a different
//! aspect of code quality. Each produces findings with confidence scores
//! (0-100). Findings below a configurable threshold are filtered out.

use serde::Deserialize;
use serde::Serialize;

/// Configuration for a scored code review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredReviewConfig {
    /// Confidence threshold (0-100). Findings below this are filtered out.
    #[serde(default = "default_confidence_threshold")]
    pub confidence_threshold: u32,
    /// Optional model override for review agents.
    #[serde(default)]
    pub review_model: Option<String>,
    /// Whether to post comments (e.g., to GitHub PR).
    #[serde(default)]
    pub post_comments: bool,
}

fn default_confidence_threshold() -> u32 {
    80
}

impl Default for ScoredReviewConfig {
    fn default() -> Self {
        Self {
            confidence_threshold: default_confidence_threshold(),
            review_model: None,
            post_comments: false,
        }
    }
}

/// The 4 specialized reviewer agent roles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReviewerRole {
    /// Checks adherence to project guidelines (AGENTS.md, CLAUDE.md conventions).
    GuidelinesAuditorA,
    /// Second guidelines auditor for cross-validation.
    GuidelinesAuditorB,
    /// Focuses on bug detection in changed files.
    BugDetector,
    /// Analyzes git history context for the changed files.
    HistoryAnalyzer,
}

impl ReviewerRole {
    /// All reviewer roles.
    pub fn all() -> &'static [ReviewerRole] {
        &[
            Self::GuidelinesAuditorA,
            Self::GuidelinesAuditorB,
            Self::BugDetector,
            Self::HistoryAnalyzer,
        ]
    }

    /// Human-readable label for the role.
    pub fn label(&self) -> &'static str {
        match self {
            Self::GuidelinesAuditorA => "Guidelines Auditor A",
            Self::GuidelinesAuditorB => "Guidelines Auditor B",
            Self::BugDetector => "Bug Detector",
            Self::HistoryAnalyzer => "History Analyzer",
        }
    }

    /// System prompt for this reviewer role.
    pub fn system_prompt(&self) -> &'static str {
        match self {
            Self::GuidelinesAuditorA | Self::GuidelinesAuditorB => GUIDELINES_AUDITOR_PROMPT,
            Self::BugDetector => BUG_DETECTOR_PROMPT,
            Self::HistoryAnalyzer => HISTORY_ANALYZER_PROMPT,
        }
    }
}

// ----- Reviewer System Prompts -----

const GUIDELINES_AUDITOR_PROMPT: &str = "\
You are a code review agent specializing in project guidelines compliance.\n\n\
Review the changed files for:\n\
1. Adherence to AGENTS.md / CLAUDE.md / project conventions\n\
2. Naming conventions and code style\n\
3. Documentation requirements\n\
4. API design patterns used in the project\n\
5. Test coverage expectations\n\n\
For each finding, output JSON: {\"confidence\": 0-100, \"severity\": \"low|medium|high|critical\", \
\"file\": \"path\", \"line\": number, \"description\": \"...\", \"suggestion\": \"...\"}\n\n\
Only report findings you are confident about (confidence >= 50).";

const BUG_DETECTOR_PROMPT: &str = "\
You are a code review agent specializing in bug detection.\n\n\
Focus exclusively on the CHANGED files and look for:\n\
1. Logic errors and off-by-one mistakes\n\
2. Null/None/undefined handling issues\n\
3. Race conditions and concurrency bugs\n\
4. Resource leaks (files, connections, memory)\n\
5. Error handling gaps\n\
6. Security vulnerabilities (injection, overflow, etc.)\n\n\
For each finding, output JSON: {\"confidence\": 0-100, \"severity\": \"low|medium|high|critical\", \
\"file\": \"path\", \"line\": number, \"description\": \"...\", \"suggestion\": \"...\"}\n\n\
Be precise. Only report genuine bugs, not style issues.";

const HISTORY_ANALYZER_PROMPT: &str = "\
You are a code review agent specializing in git history context analysis.\n\n\
Analyze the changed files in the context of their git history:\n\
1. Are the changes consistent with the file's evolution?\n\
2. Do they conflict with recent changes by other authors?\n\
3. Are there regression risks based on past bug fixes in the same area?\n\
4. Do the changes follow the established patterns for this code area?\n\n\
For each finding, output JSON: {\"confidence\": 0-100, \"severity\": \"low|medium|high|critical\", \
\"file\": \"path\", \"line\": number, \"description\": \"...\", \"suggestion\": \"...\"}\n\n\
Focus on contextual risks that only history analysis can reveal.";

/// A single finding from a reviewer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewFinding {
    /// Confidence score (0-100).
    pub confidence: u32,
    /// Severity level.
    pub severity: FindingSeverity,
    /// File path.
    pub file: String,
    /// Line number (if applicable).
    pub line: Option<u32>,
    /// Description of the issue.
    pub description: String,
    /// Suggested fix.
    pub suggestion: Option<String>,
    /// Which reviewer role produced this finding.
    pub reviewer: ReviewerRole,
}

/// Severity levels for review findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl FindingSeverity {
    /// Emoji for display.
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Low => "💡",
            Self::Medium => "⚠️",
            Self::High => "🔴",
            Self::Critical => "🚨",
        }
    }
}

/// Aggregated output from the scored review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredReviewOutput {
    /// All findings above the threshold, sorted by confidence (descending).
    pub findings: Vec<ReviewFinding>,
    /// Total findings before filtering.
    pub total_raw_findings: usize,
    /// Number of findings filtered out.
    pub filtered_count: usize,
    /// Confidence threshold used.
    pub threshold: u32,
}

/// Filter findings by confidence threshold and sort by confidence descending.
pub fn filter_and_sort_findings(
    mut findings: Vec<ReviewFinding>,
    threshold: u32,
) -> ScoredReviewOutput {
    let total_raw = findings.len();
    findings.retain(|f| f.confidence >= threshold);
    findings.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    let filtered_count = total_raw - findings.len();

    ScoredReviewOutput {
        findings,
        total_raw_findings: total_raw,
        filtered_count,
        threshold,
    }
}

/// Parse reviewer output text into findings.
///
/// Attempts to extract JSON objects from the text. Each valid JSON object
/// matching the ReviewFinding schema is included. Non-JSON text is ignored.
pub fn parse_reviewer_output(text: &str, role: ReviewerRole) -> Vec<ReviewFinding> {
    let mut findings = Vec::new();

    // Try to parse the entire text as a JSON array first.
    if let Ok(array) = serde_json::from_str::<Vec<RawFinding>>(text) {
        for raw in array {
            findings.push(raw.into_finding(role));
        }
        return findings;
    }

    // Otherwise, extract individual JSON objects.
    let mut search_from = 0;
    while let Some(start) = text[search_from..].find('{') {
        let abs_start = search_from + start;
        if let Some(end) = find_matching_brace(&text[abs_start..]) {
            let abs_end = abs_start + end + 1;
            if let Ok(raw) = serde_json::from_str::<RawFinding>(&text[abs_start..abs_end]) {
                findings.push(raw.into_finding(role));
            }
            search_from = abs_end;
        } else {
            break;
        }
    }

    findings
}

/// Raw finding as it comes from the LLM (before we attach the reviewer role).
#[derive(Debug, Deserialize)]
struct RawFinding {
    confidence: u32,
    severity: FindingSeverity,
    file: String,
    line: Option<u32>,
    description: String,
    suggestion: Option<String>,
}

impl RawFinding {
    fn into_finding(self, reviewer: ReviewerRole) -> ReviewFinding {
        ReviewFinding {
            confidence: self.confidence,
            severity: self.severity,
            file: self.file,
            line: self.line,
            description: self.description,
            suggestion: self.suggestion,
            reviewer,
        }
    }
}

/// Find the index of the matching closing brace for a string starting with '{'.
fn find_matching_brace(s: &str) -> Option<usize> {
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;

    for (i, ch) in s.char_indices() {
        if escape {
            escape = false;
            continue;
        }
        match ch {
            '\\' if in_string => {
                escape = true;
            }
            '"' => {
                in_string = !in_string;
            }
            '{' if !in_string => {
                depth += 1;
            }
            '}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_findings_by_threshold() {
        let findings = vec![
            ReviewFinding {
                confidence: 95,
                severity: FindingSeverity::High,
                file: "main.rs".to_string(),
                line: Some(42),
                description: "Null check missing".to_string(),
                suggestion: None,
                reviewer: ReviewerRole::BugDetector,
            },
            ReviewFinding {
                confidence: 60,
                severity: FindingSeverity::Low,
                file: "utils.rs".to_string(),
                line: Some(10),
                description: "Style issue".to_string(),
                suggestion: None,
                reviewer: ReviewerRole::GuidelinesAuditorA,
            },
            ReviewFinding {
                confidence: 85,
                severity: FindingSeverity::Medium,
                file: "lib.rs".to_string(),
                line: None,
                description: "Missing docs".to_string(),
                suggestion: Some("Add documentation".to_string()),
                reviewer: ReviewerRole::GuidelinesAuditorB,
            },
        ];

        let output = filter_and_sort_findings(findings, 80);
        assert_eq!(output.total_raw_findings, 3);
        assert_eq!(output.filtered_count, 1);
        assert_eq!(output.findings.len(), 2);
        // Sorted by confidence descending
        assert_eq!(output.findings[0].confidence, 95);
        assert_eq!(output.findings[1].confidence, 85);
    }

    #[test]
    fn parse_json_array_output() {
        let text = r#"[
            {"confidence": 90, "severity": "high", "file": "main.rs", "line": 10, "description": "bug", "suggestion": "fix it"},
            {"confidence": 70, "severity": "low", "file": "util.rs", "line": null, "description": "style", "suggestion": null}
        ]"#;

        let findings = parse_reviewer_output(text, ReviewerRole::BugDetector);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].confidence, 90);
        assert_eq!(findings[1].file, "util.rs");
        assert_eq!(findings[0].reviewer, ReviewerRole::BugDetector);
    }

    #[test]
    fn parse_embedded_json_objects() {
        let text = r#"
            Here are my findings:
            {"confidence": 85, "severity": "medium", "file": "foo.rs", "line": 5, "description": "issue found", "suggestion": "try this"}
            And another:
            {"confidence": 45, "severity": "low", "file": "bar.rs", "line": 1, "description": "minor", "suggestion": null}
        "#;

        let findings = parse_reviewer_output(text, ReviewerRole::HistoryAnalyzer);
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0].confidence, 85);
        assert_eq!(findings[1].reviewer, ReviewerRole::HistoryAnalyzer);
    }

    #[test]
    fn parse_no_json() {
        let text = "No issues found. The code looks good!";
        let findings = parse_reviewer_output(text, ReviewerRole::GuidelinesAuditorA);
        assert!(findings.is_empty());
    }

    #[test]
    fn severity_emoji() {
        assert_eq!(FindingSeverity::Low.emoji(), "💡");
        assert_eq!(FindingSeverity::Critical.emoji(), "🚨");
    }

    #[test]
    fn reviewer_roles() {
        assert_eq!(ReviewerRole::all().len(), 4);
        assert_eq!(ReviewerRole::BugDetector.label(), "Bug Detector");
    }

    #[test]
    fn default_config() {
        let config = ScoredReviewConfig::default();
        assert_eq!(config.confidence_threshold, 80);
        assert!(!config.post_comments);
        assert!(config.review_model.is_none());
    }

    #[test]
    fn find_matching_brace_works() {
        assert_eq!(find_matching_brace(r#"{"a": "b"}"#), Some(9));
        assert_eq!(find_matching_brace(r#"{"a": {"b": 1}}"#), Some(14));
        assert_eq!(find_matching_brace(r#"{"a": "}"}"#), Some(9));
        assert!(find_matching_brace("{unclosed").is_none());
    }
}
