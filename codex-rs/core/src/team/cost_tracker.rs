//! Cost tracking: JSONL logging with budget enforcement.
//!
//! Each agent appends cost entries to `costs/costs.jsonl` after every session.
//! The team daemon and agents read this file to enforce budget caps.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// A single cost entry recorded after an agent session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostEntry {
    pub agent_id: String,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub task_id: String,
}

/// Manages cost tracking for the entire team.
pub struct CostTracker {
    /// Path to the JSONL cost log file.
    pub log_path: PathBuf,
    /// Maximum total budget (USD) across all agents.
    pub max_total_usd: f64,
    /// Maximum per-agent budget (USD).
    pub max_per_agent_usd: f64,
    /// Alert threshold as a percentage (0–100).
    pub alert_threshold_pct: u32,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum CostError {
    Io(PathBuf, std::io::Error),
    Parse(String),
}

impl std::fmt::Display for CostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(path, e) => write!(f, "cost tracker I/O error at '{}': {e}", path.display()),
            Self::Parse(msg) => write!(f, "cost tracker parse error: {msg}"),
        }
    }
}

impl std::error::Error for CostError {}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

impl CostTracker {
    /// Create a new tracker.
    pub fn new(log_path: PathBuf, max_total_usd: f64, max_per_agent_usd: f64, alert_threshold_pct: u32) -> Self {
        Self {
            log_path,
            max_total_usd,
            max_per_agent_usd,
            alert_threshold_pct,
        }
    }

    /// Record a cost entry (append to the JSONL file).
    pub fn record(&self, entry: &CostEntry) -> Result<(), CostError> {
        if let Some(parent) = self.log_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| CostError::Io(self.log_path.clone(), e))?;
        }

        let line =
            serde_json::to_string(entry).map_err(|e| CostError::Parse(e.to_string()))?;

        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.log_path)
            .map_err(|e| CostError::Io(self.log_path.clone(), e))?;

        writeln!(file, "{line}").map_err(|e| CostError::Io(self.log_path.clone(), e))?;

        Ok(())
    }

    /// Read all cost entries from the log file.
    pub fn read_all(&self) -> Result<Vec<CostEntry>, CostError> {
        if !self.log_path.exists() {
            return Ok(vec![]);
        }

        let file = std::fs::File::open(&self.log_path)
            .map_err(|e| CostError::Io(self.log_path.clone(), e))?;
        let reader = std::io::BufReader::new(file);

        let mut entries = Vec::new();
        for line in reader.lines() {
            let line = line.map_err(|e| CostError::Io(self.log_path.clone(), e))?;
            if line.trim().is_empty() {
                continue;
            }
            let entry: CostEntry =
                serde_json::from_str(&line).map_err(|e| CostError::Parse(e.to_string()))?;
            entries.push(entry);
        }
        Ok(entries)
    }

    /// Total cost across all agents.
    pub fn total_cost(&self) -> Result<f64, CostError> {
        Ok(self.read_all()?.iter().map(|e| e.cost_usd).sum())
    }

    /// Cost for a specific agent.
    pub fn agent_cost(&self, agent_id: &str) -> Result<f64, CostError> {
        Ok(self
            .read_all()?
            .iter()
            .filter(|e| e.agent_id == agent_id)
            .map(|e| e.cost_usd)
            .sum())
    }

    /// Per-agent cost breakdown.
    pub fn cost_breakdown(&self) -> Result<Vec<(String, f64)>, CostError> {
        let entries = self.read_all()?;
        let mut map = std::collections::HashMap::new();
        for entry in &entries {
            *map.entry(entry.agent_id.clone()).or_insert(0.0) += entry.cost_usd;
        }
        let mut pairs: Vec<_> = map.into_iter().collect();
        pairs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        Ok(pairs)
    }

    /// Check if the total budget has been exceeded.
    pub fn is_over_budget(&self) -> Result<bool, CostError> {
        Ok(self.total_cost()? >= self.max_total_usd)
    }

    /// Check if a specific agent has exceeded its budget.
    pub fn is_agent_over_budget(&self, agent_id: &str) -> Result<bool, CostError> {
        Ok(self.agent_cost(agent_id)? >= self.max_per_agent_usd)
    }

    /// Check if spending has crossed the alert threshold.
    pub fn should_alert(&self) -> Result<bool, CostError> {
        let total = self.total_cost()?;
        let threshold = self.max_total_usd * (self.alert_threshold_pct as f64 / 100.0);
        Ok(total >= threshold)
    }

    /// Summary string for display.
    pub fn summary(&self) -> Result<String, CostError> {
        let total = self.total_cost()?;
        let breakdown = self.cost_breakdown()?;

        let mut s = format!(
            "Total: ${:.2} / ${:.2} ({:.0}%)\n",
            total,
            self.max_total_usd,
            (total / self.max_total_usd) * 100.0
        );

        for (agent_id, cost) in &breakdown {
            s.push_str(&format!(
                "  {agent_id}: ${cost:.2} / ${:.2}\n",
                self.max_per_agent_usd
            ));
        }

        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_tracker(dir: &std::path::Path) -> CostTracker {
        CostTracker::new(
            dir.join("costs.jsonl"),
            500.0,
            100.0,
            80,
        )
    }

    fn make_entry(agent: &str, cost: f64) -> CostEntry {
        CostEntry {
            agent_id: agent.to_string(),
            session_id: format!("sess-{}", agent),
            timestamp: Utc::now(),
            input_tokens: 1000,
            output_tokens: 500,
            cost_usd: cost,
            task_id: "task-001".to_string(),
        }
    }

    #[test]
    fn record_and_read() {
        let dir = TempDir::new().unwrap();
        let tracker = make_tracker(dir.path());

        tracker.record(&make_entry("agent-1", 5.0)).unwrap();
        tracker.record(&make_entry("agent-2", 3.0)).unwrap();

        let entries = tracker.read_all().unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn total_cost() {
        let dir = TempDir::new().unwrap();
        let tracker = make_tracker(dir.path());

        tracker.record(&make_entry("agent-1", 10.0)).unwrap();
        tracker.record(&make_entry("agent-2", 20.0)).unwrap();
        tracker.record(&make_entry("agent-1", 5.0)).unwrap();

        assert!((tracker.total_cost().unwrap() - 35.0).abs() < 0.01);
    }

    #[test]
    fn agent_cost() {
        let dir = TempDir::new().unwrap();
        let tracker = make_tracker(dir.path());

        tracker.record(&make_entry("agent-1", 10.0)).unwrap();
        tracker.record(&make_entry("agent-2", 20.0)).unwrap();
        tracker.record(&make_entry("agent-1", 5.0)).unwrap();

        assert!((tracker.agent_cost("agent-1").unwrap() - 15.0).abs() < 0.01);
        assert!((tracker.agent_cost("agent-2").unwrap() - 20.0).abs() < 0.01);
    }

    #[test]
    fn budget_enforcement() {
        let dir = TempDir::new().unwrap();
        let tracker = CostTracker::new(
            dir.path().join("costs.jsonl"),
            50.0,   // low total cap
            25.0,   // low per-agent cap
            80,
        );

        tracker.record(&make_entry("agent-1", 30.0)).unwrap();
        assert!(tracker.is_agent_over_budget("agent-1").unwrap());
        assert!(!tracker.is_over_budget().unwrap());

        tracker.record(&make_entry("agent-2", 25.0)).unwrap();
        assert!(tracker.is_over_budget().unwrap());
    }

    #[test]
    fn alert_threshold() {
        let dir = TempDir::new().unwrap();
        let tracker = CostTracker::new(
            dir.path().join("costs.jsonl"),
            100.0,
            50.0,
            80, // alert at 80%
        );

        tracker.record(&make_entry("agent-1", 70.0)).unwrap();
        assert!(!tracker.should_alert().unwrap()); // 70% < 80%

        tracker.record(&make_entry("agent-2", 15.0)).unwrap();
        assert!(tracker.should_alert().unwrap()); // 85% >= 80%
    }

    #[test]
    fn empty_log_returns_zero() {
        let dir = TempDir::new().unwrap();
        let tracker = make_tracker(dir.path());
        assert!((tracker.total_cost().unwrap()).abs() < 0.001);
        assert!(!tracker.is_over_budget().unwrap());
    }
}
