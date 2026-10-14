//! Admin-panel API: dashboard, pipeline switches, cost, model profiles and
//! operational catalogs.
//!
//! Read endpoints require `viewer`; anything that changes state requires
//! `operator` (session cookies carry `admin`, so the panel itself passes
//! both).

mod drafts;
mod maintenance;
mod ops;
mod profiles;
mod settings;
mod summary;
mod usage;

use axum::Router;

use crate::api::SharedState;

pub fn routes() -> Router<SharedState> {
    Router::new()
        .merge(summary::routes())
        .merge(settings::routes())
        .merge(usage::routes())
        .merge(profiles::routes())
        .merge(ops::routes())
        .merge(drafts::routes())
        .merge(maintenance::routes())
}
