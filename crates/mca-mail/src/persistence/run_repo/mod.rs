//! Processing run persistence: one run per inbound email per attempt.

mod agents;
mod pipeline;

pub use agents::{
    add_agent_cost, agent_run, agent_runs, agent_statistics, cost_since, finish_agent, start_agent,
    AgentRunSummary, AgentStats,
};
pub use pipeline::{
    add_cost, count, find_resumable, finish, get, list, record_stage, set_lead, start, RunFilter,
    RunSummary,
};
