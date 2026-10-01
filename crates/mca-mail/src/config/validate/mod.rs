//! Startup validation.
//!
//! Split by subsystem so each rule sits next to the settings it constrains and
//! can be reviewed independently. Order matters: mail is checked before llm
//! because a production deployment that reaches a live mailbox must already
//! have a defensible transport configuration.

mod agents;
mod database;
mod llm;
mod mail;
mod security;

use crate::config::AppConfig;
use crate::error::ConfigError;

pub fn run(config: &AppConfig) -> Result<(), ConfigError> {
    database::check(config)?;
    mail::check(config)?;
    llm::check(config)?;
    agents::check(config)?;
    security::check(config)?;
    Ok(())
}
