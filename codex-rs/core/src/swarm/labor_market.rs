//! LaborMarket: a registry of named agents (fixed + dynamic).
//!
//! Inspired by kimi-cli's LaborMarket pattern. Agents are registered either
//! at startup (from YAML specs) or dynamically at runtime via CreateSubagent.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;

use super::agent_spec::AgentSpec;
use super::agent_spec::load_agent_spec;

/// How an agent was registered in the LaborMarket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentOrigin {
    /// Declared in a YAML spec file — gets its own isolated LaborMarket.
    Fixed,
    /// Created at runtime via CreateSubagent — shares parent's LaborMarket.
    Dynamic,
}

/// An entry in the LaborMarket registry.
#[derive(Debug, Clone)]
pub struct AgentEntry {
    /// Unique name of the agent.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// The resolved agent specification.
    pub spec: AgentSpec,
    /// How this agent was registered.
    pub origin: AgentOrigin,
}

/// Thread-safe registry of named agents.
///
/// Fixed agents are loaded from YAML specs at startup. Dynamic agents are
/// added at runtime. The LaborMarket is shared across agent sessions.
#[derive(Debug, Clone)]
pub struct LaborMarket {
    agents: Arc<RwLock<HashMap<String, AgentEntry>>>,
}

impl Default for LaborMarket {
    fn default() -> Self {
        Self::new()
    }
}

impl LaborMarket {
    /// Create an empty LaborMarket.
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Load fixed agents from an agent spec's subagent declarations.
    ///
    /// For each subagent declared in the spec, loads its YAML definition and
    /// registers it as a fixed agent.
    pub fn load_from_spec(&self, spec: &AgentSpec) -> Result<(), LaborMarketError> {
        let source_dir = spec
            .source_dir
            .as_deref()
            .unwrap_or_else(|| Path::new("."));

        for decl in &spec.subagents {
            let sub_path = source_dir.join(&decl.path);
            let sub_spec = load_agent_spec(&sub_path).map_err(|e| LaborMarketError::SpecLoad {
                name: decl.name.clone(),
                source: e.to_string(),
            })?;

            self.register(AgentEntry {
                name: decl.name.clone(),
                description: decl.description.clone(),
                spec: sub_spec,
                origin: AgentOrigin::Fixed,
            })?;
        }

        Ok(())
    }

    /// Register an agent in the market.
    ///
    /// Returns an error if an agent with the same name already exists.
    pub fn register(&self, entry: AgentEntry) -> Result<(), LaborMarketError> {
        let mut agents = self.agents.write().map_err(|_| LaborMarketError::LockPoisoned)?;
        if agents.contains_key(&entry.name) {
            return Err(LaborMarketError::DuplicateName {
                name: entry.name.clone(),
            });
        }
        agents.insert(entry.name.clone(), entry);
        Ok(())
    }

    /// Look up an agent by name.
    pub fn get(&self, name: &str) -> Option<AgentEntry> {
        let agents = self.agents.read().ok()?;
        agents.get(name).cloned()
    }

    /// List all registered agents (name + description).
    pub fn list(&self) -> Vec<(String, String)> {
        let agents = self.agents.read().unwrap_or_else(|e| e.into_inner());
        agents
            .values()
            .map(|e| (e.name.clone(), e.description.clone()))
            .collect()
    }

    /// Number of registered agents.
    pub fn len(&self) -> usize {
        let agents = self.agents.read().unwrap_or_else(|e| e.into_inner());
        agents.len()
    }

    /// Whether the market is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Errors from LaborMarket operations.
#[derive(Debug)]
pub enum LaborMarketError {
    DuplicateName { name: String },
    SpecLoad { name: String, source: String },
    LockPoisoned,
}

impl std::fmt::Display for LaborMarketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateName { name } => {
                write!(f, "agent '{name}' is already registered in the LaborMarket")
            }
            Self::SpecLoad { name, source } => {
                write!(f, "failed to load spec for agent '{name}': {source}")
            }
            Self::LockPoisoned => write!(f, "LaborMarket lock poisoned"),
        }
    }
}

impl std::error::Error for LaborMarketError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(name: &str) -> AgentEntry {
        AgentEntry {
            name: name.to_string(),
            description: format!("{name} agent"),
            spec: AgentSpec::default(),
            origin: AgentOrigin::Dynamic,
        }
    }

    #[test]
    fn register_and_get() {
        let market = LaborMarket::new();
        market.register(make_entry("coder")).unwrap();

        let entry = market.get("coder").unwrap();
        assert_eq!(entry.name, "coder");
        assert_eq!(entry.origin, AgentOrigin::Dynamic);
    }

    #[test]
    fn duplicate_name_rejected() {
        let market = LaborMarket::new();
        market.register(make_entry("coder")).unwrap();
        let result = market.register(make_entry("coder"));
        assert!(matches!(result, Err(LaborMarketError::DuplicateName { .. })));
    }

    #[test]
    fn list_agents() {
        let market = LaborMarket::new();
        market.register(make_entry("coder")).unwrap();
        market.register(make_entry("reviewer")).unwrap();

        let list = market.list();
        assert_eq!(list.len(), 2);
        let names: Vec<_> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"coder"));
        assert!(names.contains(&"reviewer"));
    }

    #[test]
    fn get_missing_returns_none() {
        let market = LaborMarket::new();
        assert!(market.get("nonexistent").is_none());
    }

    #[test]
    fn empty_market() {
        let market = LaborMarket::new();
        assert!(market.is_empty());
        assert_eq!(market.len(), 0);
    }
}
