//! Multi-agent swarm orchestration system.
//!
//! Inspired by kimi-cli's LaborMarket pattern, this module provides:
//! - `LaborMarket`: a registry of named agents (fixed + dynamic)
//! - `AgentSpec`: YAML-based agent definitions with inheritance
//! - `TaskDispatch`: tool for delegating tasks to subagents with isolated context
//! - `CreateSubagent`: tool for dynamic agent creation at runtime

pub mod agent_spec;
pub mod create_subagent;
pub mod labor_market;
pub mod task_dispatch;

pub use agent_spec::AgentSpec;
pub use agent_spec::load_agent_spec;
pub use create_subagent::CreateSubagentParams;
pub use labor_market::AgentEntry;
pub use labor_market::LaborMarket;
pub use task_dispatch::TaskDispatchParams;
pub use task_dispatch::TaskDispatchResult;
