//! CreateSubagent: dynamically register new agents at runtime.
//!
//! This allows the LLM to create specialized agents on-the-fly,
//! immediately available for TaskDispatch.

use serde::Deserialize;
use serde::Serialize;

use super::agent_spec::AgentSpec;
use super::labor_market::AgentEntry;
use super::labor_market::AgentOrigin;
use super::labor_market::LaborMarket;

/// Parameters for creating a new dynamic subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSubagentParams {
    /// Unique name for the new agent.
    pub name: String,
    /// Human-readable description of the agent's purpose.
    pub description: String,
    /// System prompt for the new agent.
    pub system_prompt: String,
}

/// Result of creating a subagent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateSubagentResult {
    /// Whether creation succeeded.
    pub success: bool,
    /// Message describing the outcome.
    pub message: String,
}

/// Create and register a dynamic subagent in the LaborMarket.
///
/// The new agent is immediately available for TaskDispatch. Dynamic agents
/// share the parent's LaborMarket (unlike fixed agents which get their own).
///
/// The agent will have `task_dispatch` and `create_subagent` excluded to
/// prevent infinite recursion.
pub fn create_subagent(
    market: &LaborMarket,
    params: CreateSubagentParams,
) -> CreateSubagentResult {
    if params.name.trim().is_empty() {
        return CreateSubagentResult {
            success: false,
            message: "Agent name cannot be empty.".to_string(),
        };
    }

    let spec = AgentSpec {
        name: Some(params.name.clone()),
        description: Some(params.description.clone()),
        system_prompt: Some(params.system_prompt),
        excluded_tools: vec![
            "task_dispatch".to_string(),
            "create_subagent".to_string(),
        ],
        ..Default::default()
    };

    let entry = AgentEntry {
        name: params.name.clone(),
        description: params.description,
        spec,
        origin: AgentOrigin::Dynamic,
    };

    match market.register(entry) {
        Ok(()) => CreateSubagentResult {
            success: true,
            message: format!(
                "Agent '{}' created successfully. Use TaskDispatch to delegate tasks to it.",
                params.name
            ),
        },
        Err(e) => CreateSubagentResult {
            success: false,
            message: format!("Failed to create agent: {e}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_use_dynamic_agent() {
        let market = LaborMarket::new();
        let result = create_subagent(
            &market,
            CreateSubagentParams {
                name: "rust-expert".to_string(),
                description: "Specializes in Rust async programming".to_string(),
                system_prompt: "You are an expert Rust developer.".to_string(),
            },
        );
        assert!(result.success);
        assert!(result.message.contains("created successfully"));

        // Verify it's in the market
        let entry = market.get("rust-expert").unwrap();
        assert_eq!(entry.origin, AgentOrigin::Dynamic);
        assert!(entry.spec.excluded_tools.contains(&"task_dispatch".to_string()));
        assert!(entry.spec.excluded_tools.contains(&"create_subagent".to_string()));
    }

    #[test]
    fn reject_duplicate_name() {
        let market = LaborMarket::new();
        create_subagent(
            &market,
            CreateSubagentParams {
                name: "agent-a".to_string(),
                description: "First".to_string(),
                system_prompt: "prompt".to_string(),
            },
        );
        let result = create_subagent(
            &market,
            CreateSubagentParams {
                name: "agent-a".to_string(),
                description: "Duplicate".to_string(),
                system_prompt: "prompt".to_string(),
            },
        );
        assert!(!result.success);
        assert!(result.message.contains("already registered"));
    }

    #[test]
    fn reject_empty_name() {
        let market = LaborMarket::new();
        let result = create_subagent(
            &market,
            CreateSubagentParams {
                name: "  ".to_string(),
                description: "empty".to_string(),
                system_prompt: "prompt".to_string(),
            },
        );
        assert!(!result.success);
        assert!(result.message.contains("cannot be empty"));
    }
}
