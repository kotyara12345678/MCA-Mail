//! Read-only mode (`MAIL_MODE=read_only`) end to end.
//!
//! The mode is a precise split: the customer's mailbox is untouchable, but the
//! agent's own database *is* the product. A customer who asks for analysis-only
//! must still get classification, leads and drafts. So the contract has two
//! halves, and a change that satisfies only one of them is a bug:
//!
//! - `database` runs the real repositories against a real PostgreSQL.
//! - `transport` asserts the mailbox half refused, with no mailbox involved.
//!
//! No production mailbox is contacted by either half.

mod support;

#[path = "read_only/database.rs"]
mod database;
#[path = "read_only/fixtures.rs"]
mod fixtures;
#[path = "read_only/transport.rs"]
mod transport;
