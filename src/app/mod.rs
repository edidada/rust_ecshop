//! Dependency assembly and startup.

use crate::http::state::{AppState, AppConfig};
use crate::shared::error::AppError;

pub async fn run() -> Result<(), AppError> {
    let config = AppConfig::from_env();
    let state = AppState {
        config: std::sync::Arc::new(config),
    };
    crate::http::run(state).await
}
