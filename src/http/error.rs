//! Error response factories, mirroring docs/10_api_error_factories.md.

use crate::shared::error::AppError;

/// 400 validation_error
pub fn validation(message: impl Into<String>) -> AppError {
    AppError::Validation(message.into())
}

/// 404 not_found
pub fn not_found(message: impl Into<String>) -> AppError {
    AppError::NotFound(message.into())
}

/// 409 conflict (generic)
pub fn conflict(code: &str, message: impl Into<String>) -> AppError {
    let _ = code;
    AppError::Conflict(message.into())
}

/// 409 conflict (out of stock shortcut)
pub fn out_of_stock() -> AppError {
    AppError::Conflict("insufficient stock or unavailable goods".to_string())
}

/// 422 invalid_state
pub fn invalid_state(message: impl Into<String>) -> AppError {
    AppError::InvalidState(message.into())
}
