//! End-to-end backup tests against a real PostgreSQL.
//!
//! `pg_dump` and `pg_restore` are external tools, so these need both a real
//! database and the PostgreSQL client binaries. When either is missing the tests
//! report the skip on stderr rather than passing quietly — a fake success here
//! would be the worst possible outcome, since this is exactly what an operator
//! relies on after a server loss.

mod support;

#[path = "backup/dump.rs"]
mod dump;
#[path = "backup/harness.rs"]
mod harness;
#[path = "backup/safety.rs"]
mod safety;
