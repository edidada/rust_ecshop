pub mod goods;
pub mod health;

use axum::Router;

pub use health::health_routes;

pub fn api_v1_routes() -> Router<crate::http::state::AppState> {
    use axum::routing::{get, post};
    Router::new()
        // goods context
        .route("/api/v1/home", get(goods::home))
        .route("/api/v1/catalog", get(goods::catalog))
        .route("/api/v1/goods", get(goods::goods_list))
        .route("/api/v1/goods/:id", get(goods::goods_detail))
        .route("/api/v1/goods/:id/gallery", get(goods::goods_gallery))
        .route("/api/v1/goods/:id/price-quote", post(goods::price_quote))
        .route("/api/v1/goods/:id/comments", get(goods::goods_comments))
        .route("/api/v1/brands", get(goods::brands))
        .route("/api/v1/brands/:id/goods", get(goods::brand_goods))
        .route("/api/v1/categories/:id/goods", get(goods::category_goods))
        .route("/api/v1/compare", get(goods::compare))
}
