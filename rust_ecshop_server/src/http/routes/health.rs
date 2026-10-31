use axum::routing::get;
use axum::Router;

pub fn health_routes() -> Router<crate::http::state::AppState> {
    Router::new().route("/healthz", get(healthz))
}

async fn healthz() -> &'static str {
    "ok"
}
