pub mod health;

use axum::Router;

pub use health::health_routes;

pub fn api_v1_routes() -> Router<crate::http::state::AppState> {
    Router::new()
        // M1 routes will be added here: goods, categories, search...
        .route("/api/v1/ping", axum::routing::get(ping))
}

async fn ping() -> &'static str {
    "pong"
}
