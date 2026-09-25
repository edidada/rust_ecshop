pub mod account;
pub mod auth;
pub mod cart;
pub mod community;
pub mod content;
pub mod goods;
pub mod health;
pub mod marketing;
pub mod order;
pub mod payment;
pub mod widgets;

use axum::Router;

pub use health::health_routes;

pub fn api_v1_routes() -> Router<crate::http::state::AppState> {
    use axum::routing::{delete, get, patch, post};
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
        // widgets and XML documents (top-level paths)
        .route("/api/v1/ads/:id", get(widgets::ad_detail))
        .route("/api/v1/ads/:id/click", post(widgets::ad_click))
        .route("/cycle-image.xml", get(widgets::cycle_image))
        .route("/goods-widget.js", get(widgets::goods_widget))
        .route("/sitemap.xml", get(widgets::sitemap))
        .route("/feed.xml", get(widgets::feed))
        // auth context
        .route("/api/v1/auth/availability/username", post(auth::username_availability))
        .route("/api/v1/auth/availability/email", post(auth::email_availability))
        .route("/api/v1/auth/register", post(auth::register))
        .route("/api/v1/auth/login", post(auth::login))
        .route("/api/v1/auth/logout", post(auth::logout))
        .route("/api/v1/me", get(auth::me).patch(auth::me_patch))
        .route("/api/v1/me/password", patch(auth::me_password_patch))
        // account asset context
        .route("/api/v1/me/addresses", get(account::address_list).post(account::address_create))
        .route("/api/v1/me/addresses/:id", patch(account::address_update).delete(account::address_delete))
        .route("/api/v1/me/favorites", get(account::favorite_list).post(account::favorite_create))
        .route("/api/v1/me/favorites/:id", patch(account::favorite_update).delete(account::favorite_delete))
        .route("/api/v1/me/bookings", get(account::booking_list).post(account::booking_create))
        .route("/api/v1/me/bookings/:id", delete(account::booking_delete))
        .route("/api/v1/me/bonuses", get(account::bonus_list))
        .route("/api/v1/me/bonuses/claim", post(account::bonus_claim))
        .route("/api/v1/me/email-verifications", post(account::email_verification_request))
        .route("/api/v1/email-verifications/confirm", post(account::email_verification_confirm))
        .route("/api/v1/password-resets", post(account::password_reset_request))
        .route("/api/v1/password-resets/confirm", post(account::password_reset_confirm))
        .route("/api/v1/newsletter-subscriptions", post(account::newsletter_subscribe).delete(account::newsletter_unsubscribe))
        .route("/api/v1/newsletter-subscriptions/confirm", post(account::newsletter_confirm))
        .route("/api/v1/newsletter-unsubscriptions/confirm", post(account::newsletter_confirm))
        .route("/api/v1/me/account/requests", get(account::account_request_list).post(account::account_request_create))
        .route("/api/v1/me/account/requests/:id", delete(account::account_request_cancel))
        .route("/api/v1/me/account/requests/:id/payment", post(account::account_request_payment))
        .route("/api/v1/me/account/transactions", get(account::account_transactions))
        // cart and checkout context
        .route("/api/v1/me/cart", get(cart::cart_list).post(cart::cart_add))
        .route("/api/v1/me/cart/:id", patch(cart::cart_update).delete(cart::cart_delete))
        .route("/api/v1/checkout/options", get(cart::checkout_options))
        .route("/api/v1/checkout/quote", post(cart::checkout_quote))
        // order context
        .route("/api/v1/orders", post(order::order_create))
        .route("/api/v1/me/orders", get(order::order_list))
        .route("/api/v1/me/orders/merge", post(order::order_merge))
        .route("/api/v1/me/orders/by-number/:order_sn/status", get(order::order_status_by_sn))
        .route("/api/v1/me/orders/:id", get(order::order_detail))
        .route("/api/v1/me/orders/:id/cancel", post(order::order_cancel))
        .route("/api/v1/me/orders/:id/received", post(order::order_received))
        .route("/api/v1/me/orders/:id/cart", post(order::order_return_to_cart))
        .route("/api/v1/me/orders/:id/surplus", patch(order::order_surplus))
        .route("/api/v1/me/orders/:id/payment", patch(order::order_payment_patch))
        .route("/api/v1/me/orders/:id/address", patch(order::order_address_patch))
        .route("/api/v1/me/group-buys", get(order::me_group_buys))
        .route("/api/v1/me/group-buys/:id", get(order::me_group_buy_detail))
        // payment callback
        .route("/api/v1/payments/:provider/callback", post(payment::payment_callback))
}
