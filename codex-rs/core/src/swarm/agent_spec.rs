//! YAML-based agent specification with inheritance support.

use serde::Deserialize;
use serde::Serialize;
use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;

/// A subagent declaration within an agent spec.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentDecl {
    /// Name of the subagent.
    pub name: String,
    /// Path to the subagent's YAML spec (relative to parent spec).
    pub path: String,
    /// Human-readable description of what this subagent does.
    pub description: String,
}

/// An agent specification loaded from YAML.
///
/// Supports inheritance via the `extends` field: a child spec merges with its
/// parent, overriding fields that are explicitly set.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentSpec {
    /// Optional base spec to inherit from (relative path).
    #[serde(default)]
    pub extends: Option<String>,

    /// Agent name. If not set, derived from the filename.
    #[serde(default)]
    pub name: Option<String>,

    /// Human-readable description of the agent's purpose.
    #[serde(default)]
    pub description: Option<String>,

    /// System prompt template. May contain `${VAR}` placeholders.
    #[serde(default)]
    pub system_prompt: Option<String>,

    /// Additional arguments injected into the system prompt template.
    #[serde(default)]
    pub system_prompt_args: std::collections::HashMap<String, String>,

    /// Tools this agent is allowed to use. If empty, all tools are available.
    #[serde(default)]
    pub tools: Vec<String>,

    /// Tools explicitly excluded from this agent.
    #[serde(default)]
    pub excluded_tools: Vec<String>,

    /// Fixed subagents declared in this spec.
    #[serde(default)]
    pub subagents: Vec<SubagentDecl>,

    /// The resolved directory this spec was loaded from (not serialized).
    #[serde(skip)]
    pub source_dir: Option<PathBuf>,
}

impl AgentSpec {
    /// Returns the effective set of excluded tool names.
    pub fn excluded_tool_set(&self) -> HashSet<String> {
        self.excluded_tools.iter().cloned().collect()
    }

    /// Merge a child spec on top of this (parent) spec.
    /// Child fields override parent fields when set.
    pub fn merge_child(&self, child: &AgentSpec) -> AgentSpec {
        AgentSpec {
            extends: None, // inheritance resolved
            name: child.name.clone().or_else(|| self.name.clone()),
            description: child.description.clone().or_else(|| self.description.clone()),
            system_prompt: child
                .system_prompt
                .clone()
                .or_else(|| self.system_prompt.clone()),
            system_prompt_args: {
                let mut merged = self.system_prompt_args.clone();
                merged.extend(child.system_prompt_args.clone());
                merged
            },
            tools: if child.tools.is_empty() {
                self.tools.clone()
            } else {
                child.tools.clone()
            },
            excluded_tools: if child.excluded_tools.is_empty() {
                self.excluded_tools.clone()
            } else {
                child.excluded_tools.clone()
            },
            subagents: if child.subagents.is_empty() {
                self.subagents.clone()
            } else {
                child.subagents.clone()
            },
            source_dir: child.source_dir.clone().or_else(|| self.source_dir.clone()),
        }
    }
}

/// Load an agent spec from a YAML file, resolving inheritance.
///
/// If the spec has an `extends` field, the parent spec is loaded first and
/// merged, recursively up to a depth limit of 10.
pub fn load_agent_spec(path: &Path) -> Result<AgentSpec, AgentSpecError> {
    load_agent_spec_inner(path, 0)
}

fn load_agent_spec_inner(path: &Path, depth: usize) -> Result<AgentSpec, AgentSpecError> {
    if depth > 10 {
        return Err(AgentSpecError::InheritanceDepthExceeded);
    }

    let content = std::fs::read_to_string(path).map_err(|e| AgentSpecError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    let mut spec: AgentSpec =
        serde_yaml::from_str(&content).map_err(|e| AgentSpecError::Parse {
            path: path.to_path_buf(),
            source: e,
        })?;

    let spec_dir = path.parent().unwrap_or(Path::new(".")).to_path_buf();
    spec.source_dir = Some(spec_dir.clone());

    if let Some(ref base_path) = spec.extends {
        let parent_path = spec_dir.join(base_path);
        let parent = load_agent_spec_inner(&parent_path, depth + 1)?;
        spec = parent.merge_child(&spec);
    }

    Ok(spec)
}

/// Errors that can occur when loading agent specs.
#[derive(Debug)]
pub enum AgentSpecError {
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_yaml::Error,
    },
    InheritanceDepthExceeded,
}

impl std::fmt::Display for AgentSpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "failed to read {}: {source}", path.display()),
            Self::Parse { path, source } => {
                write!(f, "failed to parse {}: {source}", path.display())
            }
            Self::InheritanceDepthExceeded => write!(f, "agent spec inheritance depth exceeded (max 10)"),
        }
    }
}

impl std::error::Error for AgentSpecError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn parse_simple_spec() {
        let dir = TempDir::new().unwrap();
        let spec_path = dir.path().join("agent.yaml");
        let mut f = std::fs::File::create(&spec_path).unwrap();
        writeln!(
            f,
            r#"
name: test-agent
description: A test agent
system_prompt: "You are a helpful assistant."
tools:
  - shell
  - file_read
excluded_tools:
  - task_dispatch
subagents:
  - name: coder
    path: sub.yaml
    description: Writes code
"#
        )
        .unwrap();

        let spec = load_agent_spec(&spec_path).unwrap();
        assert_eq!(spec.name.as_deref(), Some("test-agent"));
        assert_eq!(spec.tools, vec!["shell", "file_read"]);
        assert_eq!(spec.excluded_tools, vec!["task_dispatch"]);
        assert_eq!(spec.subagents.len(), 1);
        assert_eq!(spec.subagents[0].name, "coder");
    }

    #[test]
    fn inheritance_merges_specs() {
        let dir = TempDir::new().unwrap();

        // Parent spec
        let parent_path = dir.path().join("base.yaml");
        let mut f = std::fs::File::create(&parent_path).unwrap();
        writeln!(
            f,
            r#"
name: base-agent
description: Base agent
system_prompt: "You are a base agent."
tools:
  - shell
  - file_read
  - task_dispatch
"#
        )
        .unwrap();

        // Child spec
        let child_path = dir.path().join("child.yaml");
        let mut f = std::fs::File::create(&child_path).unwrap();
        writeln!(
            f,
            r#"
extends: base.yaml
name: child-agent
excluded_tools:
  - task_dispatch
  - create_subagent
system_prompt_args:
  ROLE: coder
"#
        )
        .unwrap();

        let spec = load_agent_spec(&child_path).unwrap();
        assert_eq!(spec.name.as_deref(), Some("child-agent"));
        // description inherited from parent
        assert_eq!(spec.description.as_deref(), Some("Base agent"));
        // tools inherited from parent
        assert_eq!(spec.tools, vec!["shell", "file_read", "task_dispatch"]);
        // excluded_tools from child
        assert_eq!(spec.excluded_tools, vec!["task_dispatch", "create_subagent"]);
        // system_prompt_args from child
        assert_eq!(spec.system_prompt_args.get("ROLE").unwrap(), "coder");
    }

    #[test]
    fn depth_limit_prevents_infinite_recursion() {
        let dir = TempDir::new().unwrap();
        let spec_path = dir.path().join("loop.yaml");
        let mut f = std::fs::File::create(&spec_path).unwrap();
        writeln!(f, "extends: loop.yaml\nname: loop").unwrap();

        let result = load_agent_spec(&spec_path);
        assert!(matches!(result, Err(AgentSpecError::InheritanceDepthExceeded)));
    }

    #[test]
    fn excluded_tool_set() {
        let spec = AgentSpec {
            excluded_tools: vec!["task_dispatch".into(), "create_subagent".into()],
            ..Default::default()
        };
        let set = spec.excluded_tool_set();
        assert!(set.contains("task_dispatch"));
        assert!(set.contains("create_subagent"));
        assert!(!set.contains("shell"));
    }
}
