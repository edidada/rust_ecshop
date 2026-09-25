use axum::Router;
use tower_http::trace::TraceLayer;

use crate::http::routes;
use crate::http::state::AppState;
use crate::shared::error::AppError;

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .merge(routes::health_routes())
        .merge(routes::api_v1_routes())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn run(state: AppState) -> Result<(), AppError> {
    let addr = format!("{}:{}", state.config.host, state.config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    tracing::info!("listening on http://{addr}");
    axum::serve(listener, build_router(state))
        .await
        .map_err(|e| AppError::Internal(e.into()))?;
    Ok(())
}
