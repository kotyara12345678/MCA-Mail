//! Flat environment variable names mapped onto the nested configuration tree.
//!
//! MCA's operators work from a flat `.env`, and the documented variable names
//! (`DATABASE_URL`, `MAIL_IMAP_PORT`, `LLM_MODEL`, ...) are part of the
//! deployment contract, so the mapping is declared explicitly instead of being
//! inferred from a prefix rule. Adding a setting means adding one row.

mod backup;
mod database;
mod llm;
mod mail;
mod process;
mod retention;

#[cfg(test)]
mod tests;

/// Every mapping table, grouped by subsystem. Kept as slices so adding a
/// variable means editing one table, not a single ever-growing list.
pub const TABLES: &[&[(&str, &str)]] = &[
    process::APP,
    database::DATABASE,
    mail::MAIL,
    backup::BACKUP,
    llm::LLM,
    process::AGENTS,
    process::SECURITY,
    retention::RESEARCH,
    retention::RETENTION,
];

/// Look up the nested configuration path a flat variable name maps to.
pub fn nested_target(flat: &str) -> Option<&'static str> {
    TABLES
        .iter()
        .flat_map(|table| table.iter())
        .find(|(name, _)| *name == flat)
        .map(|(_, nested)| *nested)
}

/// All (flat name, nested path) pairs across every table.
pub fn all_pairs() -> Vec<(&'static str, &'static str)> {
    TABLES
        .iter()
        .flat_map(|table| table.iter())
        .map(|(flat, nested)| (*flat, *nested))
        .collect()
}
