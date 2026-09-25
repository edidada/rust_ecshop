//! Dependency assembly and startup.

use crate::http::state::{AppState, AppConfig};
use crate::infrastructure::sqlite;
use crate::shared::error::AppError;

pub async fn run() -> Result<(), AppError> {
    let config = AppConfig::from_env();
    config
        .validate()
        .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;
    let db = sqlite::open_sqlite(&config.database_url).map_err(AppError::Internal)?;
    let state = AppState {
        config: std::sync::Arc::new(config),
        db,
    };
    crate::http::run(state).await
}
