//! Processing run persistence: one run per inbound email per attempt.

mod agents;
mod pipeline;

pub use agents::{
    add_agent_cost, agent_run, agent_runs, agent_runs_for_email, agent_statistics, cost_since,
    finish_agent, start_agent, total_cost, usage_by_day, usage_by_model, AgentRunSummary,
    AgentStats, DayUsage, ModelUsage,
};
pub use pipeline::{
    add_cost, count, find_resumable, finish, get, list, record_stage, set_lead, start, RunFilter,
    RunSummary,
};
