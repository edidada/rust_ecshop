pub mod community;
pub mod content;
pub mod goods;
pub mod health;
pub mod marketing;

use axum::Router;

pub use health::health_routes;

pub fn api_v1_routes() -> Router<crate::http::state::AppState> {
    use axum::routing::{delete, get, post};
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
        // content context
        .route("/api/v1/regions", get(content::regions))
        .route("/api/v1/articles/:id", get(content::article_detail))
        .route(
            "/api/v1/article-categories/:id/articles",
            get(content::article_category_articles),
        )
        .route("/api/v1/shipping-options", get(content::shipping_options))
        .route("/api/v1/quotation", get(content::quotation))
        // marketing context
        .route("/api/v1/activities", get(marketing::activities))
        .route("/api/v1/packages", get(marketing::packages))
        .route("/api/v1/promotions", get(marketing::promotions))
        .route("/api/v1/promotions/:id", get(marketing::promotion_detail))
        .route("/api/v1/topics/:id", get(marketing::topic_detail))
        .route("/api/v1/votes/current", get(marketing::votes_current))
        .route(
            "/api/v1/votes/:id/responses",
            post(marketing::vote_respond),
        )
        .route("/api/v1/exchange-goods", get(marketing::exchange_goods))
        .route(
            "/api/v1/exchange-goods/:id",
            get(marketing::exchange_goods_detail),
        )
        // community context
        .route("/api/v1/messages", get(community::messages_list).post(community::messages_create))
        .route("/api/v1/me/messages", get(community::me_messages))
        .route("/api/v1/me/messages/:id", delete(community::me_message_delete))
        .route(
            "/api/v1/goods/:id/comments",
            get(goods::goods_comments).post(community::goods_comment_create),
        )
        .route("/api/v1/me/comments", get(community::me_comments))
        .route("/api/v1/me/comments/:id", delete(community::me_comment_delete))
        .route("/api/v1/tags", get(community::tags_cloud))
        .route("/api/v1/me/tags", get(community::me_tags).delete(community::me_tag_delete))
        .route("/api/v1/goods/:id/tags", post(community::goods_tag_create))
}
