//! Lead persistence. Split into reads, writes and the row mapping so no single
//! file carries both the schema knowledge and the query set.

mod query;
mod row;
mod write;

pub use query::{count, get, list, LeadFilter, LeadSummary};
pub use row::to_domain;
pub use write::{
    create_manual, ensure, find_by_inn, find_by_key, is_automation_locked, lock_automation,
    mark_callback, release_automation, source_of, update_identity, update_status, update_summary,
};
