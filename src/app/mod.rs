//! Dependency assembly and startup.

use crate::http::state::{AppState, AppConfig};
use crate::infrastructure::db;
use crate::shared::error::AppError;

pub async fn run() -> Result<(), AppError> {
    let config = AppConfig::from_env();
    config
        .validate()
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    let rb = db::init(&config.database_url)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    let state = AppState {
        config: std::sync::Arc::new(config),
        rb,
    };
    crate::http::run(state).await
}
