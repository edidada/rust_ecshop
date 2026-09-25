# URL 与功能清单

PHP 参考项目把页面、表单提交和 AJAX 混在 `xxx.php?act=...` / `flow.php?step=...` 入口中。C++ 版本将页面与 API 分离：浏览器页面后续可由 SPA/SSR 消费 JSON API；下表是实现优先级而非已实现清单。

## 前台核心映射

| 优先级 | PHP 入口（参考） | C++ REST API | 方法 | 主要表 |
| --- | --- | --- | --- |
| M0 | — | `/healthz` | GET | — |
| M1 | `index.php` | `/api/v1/home` | GET | goods, category, article |
| M3 | `catalog.php` | `/api/v1/catalog` | GET | category, goods, brand |
| M4 | `sitemaps.php` | `/sitemap.xml` | GET | category, goods, article_cat, article |
| M4 | `feed.php` | `/feed.xml` | GET | goods, category, goods_activity, exchange_goods, favourable_activity, article |
| M1 | `category.php?id=` | `/api/v1/categories/{id}/goods` | GET | category, goods, goods_cat |
| M1 | `goods.php?id=` | `/api/v1/goods/{id}` | GET | goods, goods_gallery, goods_attr |
| M1 | `goods.php?act=price` | `/api/v1/goods/{id}/price-quote` | POST | goods, goods_attr, member_price |
| M1 | `search.php?keywords=` | `/api/v1/goods` | GET | goods, keywords |
| M3 | `compare.php?goods[]=` | `/api/v1/compare?goods_ids=` | GET | goods |
| M3 | `activity.php` | `/api/v1/activities` | GET | favourable_activity |
| M3 | `package.php` | `/api/v1/packages` | GET | goods_activity, package_goods, goods |
| M3 | `topic.php?topic_id=` | `/api/v1/topics/{id}` | GET | topic, goods |
| M3 | `vote.php` | `/api/v1/votes/current`、`/api/v1/votes/{id}/responses` | GET/POST | vote, vote_option, vote_log |
| M3 | `exchange.php?act=list/view` | `/api/v1/exchange-goods`、`/api/v1/exchange-goods/{id}` | GET | exchange_goods, goods, category |
| M1 | `brand.php` | `/api/v1/brands`、`/api/v1/brands/{id}/goods` | GET | brand, goods |
| M1 | `article.php` / `article_cat.php` | `/api/v1/articles/{id}`、`/api/v1/article-categories/{id}/articles` | GET | article, article_cat |
| M1 | `region.php` | `/api/v1/regions?parent=&type=` | GET | region |
| M3 | `myship.php` | `/api/v1/shipping-options` | GET | region, shipping |
| M2 | `user.php?act=act_register` | `/api/v1/auth/register` | POST | users, reg_fields |
| M2 | `user.php?act=is_registered` | `/api/v1/auth/availability/username` | POST | users |
| M2 | `user.php?act=check_email` | `/api/v1/auth/availability/email` | POST | users |
| M2 | `user.php?act=act_login` | `/api/v1/auth/login` | POST | users, sessions |
| M2 | `user.php?act=logout` | `/api/v1/auth/logout` | POST | sessions |
| M2 | `user.php?act=profile` | `/api/v1/me` | GET/PATCH | users |
| M2 | `user.php?act=act_edit_password` | `/api/v1/me/password` | PATCH | users, sessions |
| M2 | `user.php?act=act_add_bonus` | `/api/v1/me/bonuses/claim` | POST | user_bonus, bonus_type |
| M2 | `user.php?act=bonus` | `/api/v1/me/bonuses` | GET | user_bonus, bonus_type |
| M2 | `user.php?act=send_hash_mail` | `/api/v1/me/email-verifications` | POST | email_verification_tokens, email_outbox |
| M2 | `user.php?act=validate_email` | `/api/v1/email-verifications/confirm` | POST | email_verification_tokens, email_verified_users |
| M2 | `user.php?act=get_password` / `send_pwd_email` | `/api/v1/password-resets` | POST | password_reset_tokens, email_outbox |
| M2 | `user.php?act=reset_password` | `/api/v1/password-resets/confirm` | POST | users, sessions, password_reset_tokens |
| M2 | `user.php?act=act_account` | `/api/v1/me/account/requests` | POST | account_balance, user_account, payment |
| M2 | `user.php?act=account_log` | `/api/v1/me/account/requests` | GET | account_balance, user_account |
| M2 | `user.php?act=cancel` | `/api/v1/me/account/requests/{id}` | DELETE | account_balance, user_account |
| M2 | `user.php?act=account_detail` | `/api/v1/me/account/transactions` | GET | account_balance, account_log |
| M2 | `user.php?act=pay` | `/api/v1/me/account/requests/{id}/payment` | POST | user_account, account_payment_intent, payment |
| M2 | `user.php?act=address_list` | `/api/v1/me/addresses` | GET/POST |
| M2 | `user.php?act=act_edit_address` | `/api/v1/me/addresses/{id}` | PATCH | user_address |
| M2 | `user.php?act=drop_consignee` | `/api/v1/me/addresses/{id}` | DELETE | user_address |
| M2 | `user.php?act=collection_list` | `/api/v1/me/favorites` | GET/POST | collect_goods, goods |
| M2 | `user.php?act=add_to_attention` / `del_attention` | `/api/v1/me/favorites/{id}` | PATCH | collect_goods |
| M2 | `user.php?act=delete_collection` | `/api/v1/me/favorites/{id}` | DELETE | collect_goods |
| M2 | `user.php?act=add_tag` | `/api/v1/goods/{id}/tags` | POST | tag, goods |
| M2 | `user.php?act=tag_list` | `/api/v1/me/tags` | GET | tag |
| M2 | `user.php?act=act_del_tag` | `/api/v1/me/tags` | DELETE | tag |
| M2 | `user.php?act=booking_list` | `/api/v1/me/bookings` | GET | booking_goods, goods |
| M2 | `user.php?act=act_add_booking` | `/api/v1/me/bookings` | POST | booking_goods, goods |
| M2 | `user.php?act=act_del_booking` | `/api/v1/me/bookings/{id}` | DELETE | booking_goods |
| M2 | `flow.php?step=cart` | `/api/v1/me/cart` | GET | cart, goods |
| M2 | `flow.php?step=add_to_cart` | `/api/v1/me/cart` | POST | cart, goods |
| M2 | `flow.php?step=update_cart` | `/api/v1/me/cart/{recId}` | PATCH | cart |
| M2 | `flow.php?step=drop_goods` | `/api/v1/me/cart/{recId}` | DELETE | cart |
| M2 | `flow.php?step=checkout` | `/api/v1/checkout/options`、`/api/v1/checkout/quote` | GET/POST | cart, user_address, shipping, payment |
| M2 | `flow.php?step=done` | `/api/v1/orders` | POST | order_info, order_goods, goods |
| M2 | `user.php?act=order_list` | `/api/v1/me/orders` | GET | order_info |
| M2 | `user.php?act=order_detail` | `/api/v1/me/orders/{id}` | GET | order_info, order_goods |
| M2 | `user.php?act=cancel_order` | `/api/v1/me/orders/{id}/cancel` | POST | order_info, order_goods, order_action, goods |
| M2 | `user.php?act=affirm_received` | `/api/v1/me/orders/{id}/received` | POST | order_info, order_action |
| M2 | `user.php?act=return_to_cart` | `/api/v1/me/orders/{id}/cart` | POST | order_info, order_goods, goods, cart |
| M2 | `user.php?act=merge_order` | `/api/v1/me/orders/merge` | POST | order_info, order_goods, pay_log, order_action |
| M2 | `user.php?act=order_query` | `/api/v1/me/orders/by-number/{order_sn}/status` | GET | order_info |
| M2 | `user.php?act=act_edit_surplus` | `/api/v1/me/orders/{id}/surplus` | PATCH | account_balance, account_log, order_info, order_balance_payment, order_action |
| M2 | `user.php?act=act_edit_payment` | `/api/v1/me/orders/{id}/payment` | PATCH | order_info, payment, order_action |
| M2 | `user.php?act=save_order_address` | `/api/v1/me/orders/{id}/address` | PATCH | order_info, order_delivery_address, order_action |
| M2 | `user.php?act=group_buy` / `group_buy_detail` | `/api/v1/me/group-buys`、`/api/v1/me/group-buys/{id}` | GET | order_info, order_promotion, goods_activity |
| M2 | `user.php?act=affiliate` | `/api/v1/me/affiliate` | GET | user_referral, order_info, affiliate_log |
| M2 | `user.php?act=email_list` | `/api/v1/newsletter-subscriptions`、`/api/v1/newsletter-*/confirm` | POST/DELETE | email_list, email_outbox |
| M2 | `respond.php` | `/api/v1/payments/{provider}/callback` | POST | pay_log, order_info, order_action |
| M2 | `comment.php` | `/api/v1/goods/{id}/comments` | GET/POST | comment, goods, users |
| M2 | `user.php?act=del_cmt` | `/api/v1/me/comments/{id}` | DELETE | comment |
| M2 | `user.php?act=comment_list` | `/api/v1/me/comments` | GET | comment, goods, article |
| M3 | `group_buy.php` / `auction.php` / `snatch.php` | `/api/v1/promotions`、`/api/v1/promotions/{id}` | GET | goods_activity, goods |
| M3 | `message.php` | `/api/v1/messages` | GET/POST | feedback |
| M2 | `user.php?act=del_msg` | `/api/v1/me/messages/{id}` | DELETE | feedback |
| M2 | `user.php?act=message_list` | `/api/v1/me/messages` | GET | feedback |
| M3 | `affiche.php?ad_id=` | `/api/v1/ads/{id}`、`/api/v1/ads/{id}/click` | GET/POST | ad, adsense |
| M4 | `cycle_image.php` | `/cycle-image.xml` | GET | ad |
| M4 | `goods_script.php` | `/goods-widget.js` | GET | goods, category, brand |
| M3 | `quotation.php?act=print_quotation` | `/api/v1/quotation` | GET | goods, products, category |
| M3 | `gallery.php?id=` | `/api/v1/goods/{id}/gallery` | GET | goods, goods_gallery |
| M3 | `tag_cloud.php` | `/api/v1/tags` | GET | tag, goods |
| M5 | `group_buy.php` / `auction.php` / `snatch.php` | `/api/v1/promotions/*` 写操作 | POST | goods_activity, auction_log, snatch_log |

## 旧入口覆盖范围

参考项目顶层还包括 `activity.php`、`api.php`、`captcha.php`、`compare.php`、`exchange.php`、`feed.php`、`package.php`、`tag_cloud.php`、`topic.php`、`vote.php`、`wholesale.php` 等。它们归入 M5 内容、营销或兼容模块，不应阻塞主交易链路。

后台 PHP 入口位于 `upload/admin/`，例如 `goods.php`、`order.php`、`users.php`、`category.php`、`payment.php`、`shipping.php`。新项目应另设 `/api/v1/admin/**`，使用角色权限而非沿用 PHP 文件名和 `act` 参数。

## 通用查询参数

`page` 从 1 开始；`page_size` 默认 20、最大 100；列表响应恒含 `page`、`page_size`、`total`、`items`。商品列表支持 `category_id`、`brand_id`、`q`、`sort`（`price_asc|price_desc|newest|sales`）。所有金额用十进制字符串，例如 `"99.90"`，不使用浮点数。
