//! Global implementation log — persistent record across all features.
//!
//! The global log is the only artifact that persists after per-feature docs
//! are cleaned up. It records what was changed, what was learned, and serves
//! as a running history of the pipeline's work.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::loop_context::FeatureItem;

/// Manages the global implementation log file.
#[derive(Debug, Clone)]
pub struct GlobalImplementationLog {
    /// Path to the log file (default: ./implementation_log.md).
    pub path: PathBuf,
}

impl GlobalImplementationLog {
    /// Create a new log pointing to the default location in the project root.
    pub fn new(project_root: &Path) -> Self {
        Self {
            path: project_root.join("implementation_log.md"),
        }
    }

    /// Create a new log at a specific path.
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }

    /// Ensure the log file exists, creating it with a header if needed.
    pub fn ensure_exists(&self) -> io::Result<()> {
        if !self.path.exists() {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut f = fs::File::create(&self.path)?;
            writeln!(f, "# Implementation Log")?;
            writeln!(f)?;
            writeln!(
                f,
                "This file is automatically maintained by the Loop Codex pipeline."
            )?;
            writeln!(
                f,
                "It records changes made during feature development and lessons learned."
            )?;
            writeln!(f)?;
            writeln!(f, "---")?;
            writeln!(f)?;
        }
        Ok(())
    }

    /// Append a feature completion entry to the log.
    pub fn log_feature_completion(
        &self,
        feature: &FeatureItem,
        summary: &str,
        lessons_learned: &str,
        files_changed: &[String],
    ) -> io::Result<()> {
        self.ensure_exists()?;

        let mut f = fs::OpenOptions::new().append(true).open(&self.path)?;

        let timestamp = chrono_free_timestamp();
        let files_list = if files_changed.is_empty() {
            "_(none recorded)_".to_string()
        } else {
            files_changed
                .iter()
                .map(|f| format!("- `{f}`"))
                .collect::<Vec<_>>()
                .join("\n")
        };

        writeln!(f, "## Feature: {}", feature.name)?;
        writeln!(f, "**Date**: {timestamp}")?;
        writeln!(f, "**Feature #{} of plan**", feature.index + 1)?;
        writeln!(f)?;
        writeln!(f, "### Summary")?;
        writeln!(f, "{summary}")?;
        writeln!(f)?;
        writeln!(f, "### Files Changed")?;
        writeln!(f, "{files_list}")?;
        writeln!(f)?;
        writeln!(f, "### Lessons Learned")?;
        writeln!(f, "{lessons_learned}")?;
        writeln!(f)?;
        writeln!(f, "---")?;
        writeln!(f)?;

        Ok(())
    }

    /// Read the current log contents.
    pub fn read(&self) -> io::Result<String> {
        if self.path.exists() {
            fs::read_to_string(&self.path)
        } else {
            Ok(String::new())
        }
    }
}

/// Simple timestamp without chrono dependency.
fn chrono_free_timestamp() -> String {
    use std::time::SystemTime;
    match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => {
            let secs = d.as_secs();
            // Simple ISO-ish format: days since epoch * rough calc
            let days = secs / 86400;
            let years = 1970 + days / 365;
            let remaining_days = days % 365;
            let months = remaining_days / 30 + 1;
            let day = remaining_days % 30 + 1;
            format!("{years}-{months:02}-{day:02}")
        }
        Err(_) => "unknown".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_feature() -> FeatureItem {
        FeatureItem {
            name: "Add Authentication".to_string(),
            description: "JWT auth".to_string(),
            affected_files: vec![],
            index: 0,
        }
    }

    #[test]
    fn ensure_creates_log() {
        let tmp = TempDir::new().unwrap();
        let log = GlobalImplementationLog::new(tmp.path());

        assert!(!log.path.exists());
        log.ensure_exists().unwrap();
        assert!(log.path.exists());

        let content = fs::read_to_string(&log.path).unwrap();
        assert!(content.contains("# Implementation Log"));
    }

    #[test]
    fn ensure_idempotent() {
        let tmp = TempDir::new().unwrap();
        let log = GlobalImplementationLog::new(tmp.path());

        log.ensure_exists().unwrap();
        let before = fs::read_to_string(&log.path).unwrap();
        log.ensure_exists().unwrap();
        let after = fs::read_to_string(&log.path).unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn log_feature_completion() {
        let tmp = TempDir::new().unwrap();
        let log = GlobalImplementationLog::new(tmp.path());

        log.log_feature_completion(
            &test_feature(),
            "Added JWT authentication to all endpoints",
            "Use middleware pattern for auth checks",
            &["auth.rs".to_string(), "main.rs".to_string()],
        )
        .unwrap();

        let content = log.read().unwrap();
        assert!(content.contains("## Feature: Add Authentication"));
        assert!(content.contains("JWT authentication"));
        assert!(content.contains("middleware pattern"));
        assert!(content.contains("`auth.rs`"));
        assert!(content.contains("`main.rs`"));
    }

    #[test]
    fn multiple_features_appended() {
        let tmp = TempDir::new().unwrap();
        let log = GlobalImplementationLog::new(tmp.path());

        let feat1 = test_feature();
        let mut feat2 = test_feature();
        feat2.name = "Add Logging".to_string();
        feat2.index = 1;

        log.log_feature_completion(&feat1, "auth done", "lesson 1", &[])
            .unwrap();
        log.log_feature_completion(&feat2, "logging done", "lesson 2", &[])
            .unwrap();

        let content = log.read().unwrap();
        assert!(content.contains("Add Authentication"));
        assert!(content.contains("Add Logging"));
        assert!(content.contains("Feature #1"));
        assert!(content.contains("Feature #2"));
    }

    #[test]
    fn empty_files_list_shows_none() {
        let tmp = TempDir::new().unwrap();
        let log = GlobalImplementationLog::new(tmp.path());

        log.log_feature_completion(&test_feature(), "summary", "lessons", &[])
            .unwrap();

        let content = log.read().unwrap();
        assert!(content.contains("_(none recorded)_"));
    }
}
