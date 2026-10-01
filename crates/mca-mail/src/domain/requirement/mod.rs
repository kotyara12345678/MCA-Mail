//! Commercial parameters MCA collects per lead.
//!
//! The field list is a closed enum rather than a free-form map: the database
//! enforces NOT NULL per field, the qualification prompt is generated from the
//! definition itself, and API consumers get a stable contract.

mod field;
mod record;
mod rules;
mod scope;

#[cfg(test)]
mod tests;

pub use field::RequirementField;
pub use record::{LeadRequirement, RequirementSource};
pub use scope::RequirementScope;
