//! 3-tier merge conflict resolution.
//!
//! Tier 1: Auto-merge (non-overlapping changes in different files)
//! Tier 2: Heuristic (changes in same file but different functions)
//! Tier 3: LLM-assisted (overlapping changes — send to LLM for resolution)

use serde::{Deserialize, Serialize};

/// Which tier resolved the conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConflictTier {
    AutoMerge,
    Heuristic,
    LlmAssisted,
}

/// Result of a conflict resolution attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConflictResolution {
    pub tier: ConflictTier,
    pub files_resolved: Vec<String>,
    pub prompt_tokens: Option<u64>,
}

// Phase 2 implementation — full resolution logic will be added here.
