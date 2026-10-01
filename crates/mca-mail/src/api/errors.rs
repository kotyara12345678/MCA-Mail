//! API error response formatting.

use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

use crate::error::AppError;

#[derive(Serialize)]
struct ApiError {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<serde_json::Value>,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.http_status();
        let code = self.code();
        let message = self.to_string();

        (
            axum::http::StatusCode::from_u16(status)
                .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
            Json(ApiError {
                code: code.into(),
                message,
                details: None,
            }),
        )
            .into_response()
    }
}

impl From<AppError> for (axum::http::StatusCode, Json<ApiError>) {
    fn from(err: AppError) -> Self {
        let status = err.http_status();
        (
            axum::http::StatusCode::from_u16(status)
                .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR),
            Json(ApiError {
                code: err.code().into(),
                message: err.to_string(),
                details: None,
            }),
        )
    }
}
