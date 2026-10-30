# 注册 URL 清单

自动从 src/http/routes/mod.rs 提取；业务边界见 [实现与 mock 说明](12_url_implementation_and_mock.md)。

| URL | HTTP 方法 |
| --- | --- |
| `/api/v1/home` | GET |
| `/api/v1/catalog` | GET |
| `/api/v1/goods` | GET |
| `/api/v1/goods/:id` | GET |
| `/api/v1/goods/:id/gallery` | GET |
| `/api/v1/goods/:id/price-quote` | POST |
| `/api/v1/brands` | GET |
| `/api/v1/brands/:id/goods` | GET |
| `/api/v1/categories/:id/goods` | GET |
| `/api/v1/compare` | GET |
| `/api/v1/regions` | GET |
| `/api/v1/articles/:id` | GET |
| `/api/v1/article-categories/:id/articles` | GET |
| `/api/v1/shipping-options` | GET |
| `/api/v1/quotation` | GET |
| `/api/v1/activities` | GET |
| `/api/v1/packages` | GET |
| `/api/v1/promotions` | GET |
| `/api/v1/promotions/:id` | GET |
| `/api/v1/topics/:id` | GET |
| `/api/v1/votes/current` | GET |
| `/api/v1/votes/:id/responses` | POST |
| `/api/v1/exchange-goods` | GET |
| `/api/v1/exchange-goods/:id` | GET |
| `/api/v1/exchange-goods/:id/redemptions` | POST |
| `/api/v1/promotions/:id/bids` | POST |
| `/api/v1/promotions/:id/orders` | POST |
| `/api/v1/messages` | GET, POST |
| `/api/v1/me/messages` | GET |
| `/api/v1/me/messages/:id` | DELETE |
| `/api/v1/goods/:id/comments` | GET, POST |
| `/api/v1/me/comments` | GET |
| `/api/v1/me/comments/:id` | DELETE |
| `/api/v1/tags` | GET |
| `/api/v1/me/tags` | GET, DELETE |
| `/api/v1/goods/:id/tags` | POST |
| `/api/v1/ads/:id` | GET |
| `/api/v1/ads/:id/click` | POST |
| `/cycle-image.xml` | GET |
| `/goods-widget.js` | GET |
| `/sitemap.xml` | GET |
| `/feed.xml` | GET |
| `/api/v1/auth/availability/username` | POST |
| `/api/v1/auth/availability/email` | POST |
| `/api/v1/auth/register` | POST |
| `/api/v1/auth/login` | POST |
| `/api/v1/auth/logout` | POST |
| `/api/v1/me` | GET, PATCH |
| `/api/v1/me/password` | PATCH |
| `/api/v1/me/addresses` | GET, POST |
| `/api/v1/me/addresses/:id` | PATCH, DELETE |
| `/api/v1/me/favorites` | GET, POST |
| `/api/v1/me/favorites/:id` | PATCH, DELETE |
| `/api/v1/me/bookings` | GET, POST |
| `/api/v1/me/bookings/:id` | DELETE |
| `/api/v1/me/bonuses` | GET |
| `/api/v1/me/bonuses/claim` | POST |
| `/api/v1/me/email-verifications` | POST |
| `/api/v1/email-verifications/confirm` | POST |
| `/api/v1/password-resets` | POST |
| `/api/v1/password-resets/confirm` | POST |
| `/api/v1/newsletter-subscriptions` | POST, DELETE |
| `/api/v1/newsletter-subscriptions/confirm` | POST |
| `/api/v1/newsletter-unsubscriptions/confirm` | POST |
| `/api/v1/me/account/requests` | GET, POST |
| `/api/v1/me/account/requests/:id` | DELETE |
| `/api/v1/me/account/requests/:id/payment` | POST |
| `/api/v1/me/account/transactions` | GET |
| `/api/v1/me/cart` | GET, POST, DELETE |
| `/api/v1/me/cart/:id` | PATCH, DELETE |
| `/api/v1/checkout/options` | GET |
| `/api/v1/checkout/quote` | POST |
| `/api/v1/me/cart/linked-buy` | POST |
| `/api/v1/me/cart/packages/:id` | POST |
| `/api/v1/me/cart/favourables/:id` | POST |
| `/api/v1/me/cart/:id/favorite` | POST |
| `/api/v1/checkout/selection` | GET |
| `/api/v1/checkout/selection/:field` | PATCH |
| `/api/v1/checkout/funds` | GET |
| `/api/v1/checkout/bonuses/:id/validation` | POST |
| `/api/v1/orders` | POST |
| `/api/v1/me/orders` | GET |
| `/api/v1/me/orders/merge` | POST |
| `/api/v1/me/orders/by-number/:order_sn/status` | GET |
| `/api/v1/me/orders/:id` | GET |
| `/api/v1/me/orders/:id/cancel` | POST |
| `/api/v1/me/orders/:id/received` | POST |
| `/api/v1/me/orders/:id/cart` | POST |
| `/api/v1/me/orders/:id/surplus` | PATCH |
| `/api/v1/me/orders/:id/payment` | PATCH |
| `/api/v1/me/orders/:id/address` | PATCH |
| `/api/v1/me/group-buys` | GET |
| `/api/v1/me/group-buys/:id` | GET |
| `/api/v1/mock/orders/:id/pay` | POST |
| `/api/v1/mock/orders/:id/ship` | POST |
| `/api/v1/mock/deposits/:id/pay` | POST |
| `/api/v1/mock/mailbox` | GET |
| `/respond.php` | POST |
| `/receive.php` | POST |
| `/chinabank_receive.php` | POST |
| `/api/v1/payments/:provider/callback` | POST |
| `/api/v1/me/affiliate` | GET |
| `/api/v1/me/shipments` | GET |
| `/api/v1/me/points/conversion-options` | GET |
| `/api/v1/me/points/conversions` | POST |
| `/api/v1/me/browsing-history` | DELETE |
| `/api/v1/me/pms` | GET, POST |
| `/api/v1/me/pms/:id` | DELETE |
| `/api/v1/pick-out` | GET |
| `/api/v1/wholesale-goods` | GET |
| `/api/v1/wholesale/quote` | POST |
| `/api/v1/captcha` | GET |
| `/api/v1/captcha/verify` | POST |
| `/api.php` | GET |
| `/api/v1/certi` | GET |
