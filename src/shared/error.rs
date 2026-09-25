//! Shared layer: errors, results, pagination, logging helpers.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

/// Application error, converted to the unified JSON error body at the HTTP edge.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("validation error: {0}")]
    Validation(String),
    #[error("unauthenticated")]
    Unauthenticated,
    #[error("forbidden")]
    Forbidden,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid state: {0}")]
    InvalidState(String),
    #[error("internal error")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AppError::Validation(m) => (StatusCode::BAD_REQUEST, "validation_error", m.clone()),
            AppError::Unauthenticated => (
                StatusCode::UNAUTHORIZED,
                "unauthenticated",
                "missing or invalid session".to_string(),
            ),
            AppError::Forbidden => (
                StatusCode::FORBIDDEN,
                "forbidden",
                "no permission for this resource".to_string(),
            ),
            AppError::NotFound(m) => (StatusCode::NOT_FOUND, "not_found", m.clone()),
            AppError::Conflict(m) => (StatusCode::CONFLICT, "conflict", m.clone()),
            AppError::InvalidState(m) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "invalid_state",
                m.clone(),
            ),
            AppError::Internal(e) => {
                tracing::error!("internal error: {e:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "internal server error".to_string(),
                )
            }
        };
        (status, Json(json!({ "code": code, "message": message }))).into_response()
    }
}

pub type AppResult<T> = Result<T, AppError>;
