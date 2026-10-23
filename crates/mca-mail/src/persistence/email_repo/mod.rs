//! Inbound email persistence.
//!
//! Every statement is static text; all user input is bound. The single
//! dynamic element is the column list, which is a compile-time constant.

mod attachments;
mod dedup;
mod query;
mod update;
mod write;

/// Attempts a message gets before it stops coming back to the queue by
/// itself. Shared by the claim query and the failure bookkeeping so the two
/// can never disagree about when a retry is spent.
pub const MAX_ATTEMPTS: i32 = 3;

pub use attachments::{attachments, insert_attachments, purge_attachment_text};
pub use dedup::{dedup_key_for, scoped_dedup_key_for};
pub use query::{
    claim_batch, count, find, get, list, order_flag, spam_verdict, status_counts, thread_has_order,
    thread_messages, EmailFilter,
};
pub use update::{
    anonymize, attach_lead, raise_order_flag, record_error, release, requeue, set_category,
    set_classification, set_spam_verdict, set_status,
};

pub use write::{insert_inbound, InsertOutcome};
