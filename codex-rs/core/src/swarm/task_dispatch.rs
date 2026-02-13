//! TaskDispatch: delegate tasks to named subagents with isolated context.
//!
//! When the LLM wants to delegate a task to a subagent, it emits a
//! `TaskDispatch` tool call with the target agent name and task description.
//! The dispatcher looks up the agent in the LaborMarket, spawns an isolated
//! sub-codex conversation with the agent's system prompt, and returns the
//! result.

use serde::Deserialize;
use serde::Serialize;

use super::labor_market::LaborMarket;

/// Parameters for a task dispatch request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDispatchParams {
    /// Name of the target agent in the LaborMarket.
    pub agent_name: String,
    /// The task description / prompt to send to the subagent.
    pub task: String,
}

/// Result of a completed task dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskDispatchResult {
    /// Name of the agent that handled the task.
    pub agent_name: String,
    /// Whether the task completed successfully.
    pub success: bool,
    /// The agent's response / output.
    pub output: String,
}

/// Validate that a task dispatch request can be fulfilled.
///
/// Returns the agent entry if found, or an error message.
pub fn validate_dispatch(
    market: &LaborMarket,
    params: &TaskDispatchParams,
) -> Result<super::labor_market::AgentEntry, String> {
    market
        .get(&params.agent_name)
        .ok_or_else(|| {
            let available = market
                .list()
                .iter()
                .map(|(n, d)| format!("  - {n}: {d}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "Agent '{}' not found in LaborMarket.\nAvailable agents:\n{}",
                params.agent_name, available
            )
        })
}

/// Build the system prompt for a subagent dispatch.
///
/// Combines the agent's base system prompt with context isolation instructions.
pub fn build_subagent_prompt(agent_spec: &super::AgentSpec, task: &str) -> String {
    let base = agent_spec
        .system_prompt
        .as_deref()
        .unwrap_or("You are a helpful coding assistant.");

    // Apply template arguments
    let mut prompt = base.to_string();
    for (key, value) in &agent_spec.system_prompt_args {
        let placeholder = format!("${{{key}}}");
        prompt = prompt.replace(&placeholder, value);
    }

    format!(
        "{prompt}\n\n\
        ---\n\
        CONTEXT ISOLATION NOTICE: You are running as a subagent with an isolated conversation \
        context. You do not have access to the parent agent's conversation history. Focus on \
        completing the specific task below.\n\n\
        TASK:\n{task}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::swarm::agent_spec::AgentSpec;
    use crate::swarm::labor_market::{AgentEntry, AgentOrigin};

    #[test]
    fn validate_dispatch_finds_agent() {
        let market = LaborMarket::new();
        market
            .register(AgentEntry {
                name: "coder".to_string(),
                description: "Writes code".to_string(),
                spec: AgentSpec::default(),
                origin: AgentOrigin::Fixed,
            })
            .unwrap();

        let params = TaskDispatchParams {
            agent_name: "coder".to_string(),
            task: "Write hello world".to_string(),
        };
        let result = validate_dispatch(&market, &params);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name, "coder");
    }

    #[test]
    fn validate_dispatch_rejects_unknown_agent() {
        let market = LaborMarket::new();
        let params = TaskDispatchParams {
            agent_name: "unknown".to_string(),
            task: "something".to_string(),
        };
        let result = validate_dispatch(&market, &params);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[test]
    fn build_prompt_applies_template_args() {
        let spec = AgentSpec {
            system_prompt: Some("You are a ${ROLE} working on ${PROJECT}.".to_string()),
            system_prompt_args: {
                let mut m = std::collections::HashMap::new();
                m.insert("ROLE".to_string(), "code reviewer".to_string());
                m.insert("PROJECT".to_string(), "my-app".to_string());
                m
            },
            ..Default::default()
        };

        let prompt = build_subagent_prompt(&spec, "Review this PR");
        assert!(prompt.contains("You are a code reviewer working on my-app."));
        assert!(prompt.contains("TASK:\nReview this PR"));
        assert!(prompt.contains("CONTEXT ISOLATION NOTICE"));
    }

    #[test]
    fn build_prompt_uses_default_when_no_system_prompt() {
        let spec = AgentSpec::default();
        let prompt = build_subagent_prompt(&spec, "Do something");
        assert!(prompt.contains("You are a helpful coding assistant."));
    }
}
