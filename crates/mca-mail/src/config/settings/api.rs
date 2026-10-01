use std::net::{IpAddr, Ipv4Addr};

use serde::{Deserialize, Serialize};

use super::super::Secret;

/// HTTP bind configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiSettings {
    pub host: IpAddr,
    pub port: u16,
    /// Bearer token protecting every `/api/v1` route except health probes.
    /// Generated on first boot when left empty, so the service never starts
    /// with an unauthenticated admin surface.
    pub admin_token: Secret,
    /// Serve the Swagger UI page.
    pub enable_swagger_ui: bool,
    /// `*` disables CORS entirely. Recommended for a private dashboard.
    pub cors_allow_origin: Vec<String>,
    pub request_timeout_seconds: u64,
    pub max_page_size: i64,
    pub default_page_size: i64,
}

impl Default for ApiSettings {
    fn default() -> Self {
        Self {
            host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            port: 8080,
            admin_token: Secret::empty(),
            enable_swagger_ui: true,
            cors_allow_origin: Vec::new(),
            request_timeout_seconds: 30,
            max_page_size: 200,
            default_page_size: 50,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_token_is_not_set_by_default() {
        assert!(!ApiSettings::default().admin_token.is_present());
    }

    #[test]
    fn paging_defaults_are_bounded() {
        let api = ApiSettings::default();
        assert!(api.default_page_size <= api.max_page_size);
        assert!(api.max_page_size > 0);
    }
}
