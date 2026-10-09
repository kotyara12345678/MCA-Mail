//! MCA Mail — AI email agent for MCA Logistics.
//!
//! The crate is a modular monolith: `domain` holds pure types and traits,
//! `application` orchestrates use cases, and the remaining modules implement
//! the ports it depends on (mail, llm, persistence, http).

#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(clippy::let_and_return)]
#![allow(clippy::unused_async)]
#![allow(clippy::extra_unused_type_parameters)]
#![allow(clippy::unnecessary_literal_unwrap)]
#![allow(clippy::comparison_chain)]

pub mod agents;
pub mod api;
pub mod application;
pub mod backup;
pub mod cli;
pub mod config;
pub mod domain;
pub mod error;
pub mod llm;
pub mod mail;
pub mod observability;
pub mod orchestration;
pub mod persistence;
pub mod shutdown;
pub mod tools;
pub mod voice;

pub use config::AppConfig;
pub use error::{AppError, ConfigError, LlmError, MailError, PolicyError, ToolError};
