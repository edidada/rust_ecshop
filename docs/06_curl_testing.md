# curl HTTP 接口测试

下列命令是当前已实现 URL 的验收示例。开发服务器假设监听 `127.0.0.1:8080`，SQLite 初始化数据存在商品 `12`。全部 curl 显式禁用代理，避免本机代理配置干扰 localhost 测试。

```bash
export BASE_URL=http://127.0.0.1:8080
export COOKIE_JAR="$(mktemp)"
curl --noproxy '*' -fsS "$BASE_URL/healthz" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/goods/12" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/catalog" | jq .
curl --noproxy '*' -fsS "$BASE_URL/goods-widget.js?cat_id=1&goods_num=4&intro_type=is_new"
curl --noproxy '*' -fsS "$BASE_URL/api/v1/quotation?category_id=1&q=C%2B%2B&page=1&page_size=20" | jq .
curl --noproxy '*' -fsS "$BASE_URL/sitemap.xml"
curl --noproxy '*' -fsS "$BASE_URL/feed.xml?cat=1"
curl --noproxy '*' -fsS "$BASE_URL/feed.xml?type=group_buy"
curl --noproxy '*' -fsS "$BASE_URL/api/v1/goods?category_id=1&page=1&page_size=20" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/compare?goods_ids=12,14" | jq .
```

## 账号与购物车

```bash
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/auth/availability/username" \
  -H 'Content-Type: application/json' -d '{"username":"curl-user"}' | jq .
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/auth/availability/email" \
  -H 'Content-Type: application/json' -d '{"email":"curl-user@example.test"}' | jq .
REGISTER=$(curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/auth/register" \
  -H 'Content-Type: application/json' \
  -d '{"username":"curl-user","email":"curl-user@example.test","password":"correct-horse-battery-staple"}')
TOKEN=$(printf '%s' "$REGISTER" | jq -r .access_token)

AUTH=(-H "Authorization: Bearer $TOKEN")

# SQLite 初始化脚本提供的一次性学习红包；重复领取应返回 409。
curl --noproxy '*' -i -sS -X POST "$BASE_URL/api/v1/me/bonuses/claim" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -d '{"bonus_sn":"202609230001"}'
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/bonuses" "${AUTH[@]}" | jq .

curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/me/email-verifications" \
  "${AUTH[@]}" | jq .

# 开发环境用 SQLite outbox 模拟邮件发送器取得 token；生产环境由邮件 worker 消费 outbox。
VERIFY_TOKEN=$(sqlite3 data/ecshop.sqlite3 \
  "SELECT json_extract(payload,'$.token') FROM ecs_email_outbox WHERE template_name='verify_email' ORDER BY message_id DESC LIMIT 1")
curl --noproxy '*' -i -sS -X POST "$BASE_URL/api/v1/email-verifications/confirm" \
  -H 'Content-Type: application/json' -d "{\"token\":\"$VERIFY_TOKEN\"}"

curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/password-resets" \
  -H 'Content-Type: application/json' \
  -d '{"email":"curl-user@example.test"}' | jq .

DEPOSIT_REQUEST=$(curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/me/account/requests" "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"kind":"deposit","amount":"100.00","payment_id":1,"note":"curl deposit"}')
printf '%s\n' "$DEPOSIT_REQUEST" | jq .
ACCOUNT_REQUEST_ID=$(printf '%s' "$DEPOSIT_REQUEST" | jq -r .id)
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/account/requests" "${AUTH[@]}" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/account/transactions" "${AUTH[@]}" | jq .
curl --noproxy '*' -fsS -X POST \
  "$BASE_URL/api/v1/me/account/requests/$ACCOUNT_REQUEST_ID/payment" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -d '{"payment_id":1}' | jq .
curl --noproxy '*' -i -sS -X DELETE \
  "$BASE_URL/api/v1/me/account/requests/$ACCOUNT_REQUEST_ID" "${AUTH[@]}"

ADD=$(curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/me/cart" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -d '{"goods_id":12,"quantity":2}')
printf '%s\n' "$ADD" | jq .
ITEM_ID=$(printf '%s' "$ADD" | jq -r .id)
VERSION=$(printf '%s' "$ADD" | jq -r .version)

curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/cart" "${AUTH[@]}" | jq .
curl --noproxy '*' -i -fsS -X PATCH "$BASE_URL/api/v1/me/cart/$ITEM_ID" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -d "{\"quantity\":3,\"version\":$VERSION}"
curl --noproxy '*' -i -sS -X DELETE "$BASE_URL/api/v1/me/cart/$ITEM_ID" "${AUTH[@]}"
```

可直接执行完整、无代理的购物车验收：`bash scripts/curl_cart_example.sh`。脚本创建一个带时间戳的本地测试用户，不显示 access token，并验证旧版本 PATCH 的 HTTP 409。

## 商品收藏

```bash
FAVORITE=$(curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/me/favorites" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -d '{"goods_id":12}')
printf '%s\n' "$FAVORITE" | jq .
FAVORITE_ID=$(printf '%s' "$FAVORITE" | jq -r .id)

curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/favorites" "${AUTH[@]}" | jq .
curl --noproxy '*' -i -sS -X PATCH "$BASE_URL/api/v1/me/favorites/$FAVORITE_ID" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -d '{"attention":true}'
curl --noproxy '*' -i -sS -X DELETE "$BASE_URL/api/v1/me/favorites/$FAVORITE_ID" "${AUTH[@]}"
```

重复收藏同一商品应返回 409；使用另一个用户的 token 更新或删除该收藏应返回 404。

## 留言板

```bash
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/messages" "${AUTH[@]}" | jq .
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/messages" \
  -H 'Content-Type: application/json' \
  -d '{"username":"curl visitor","email":"visitor@example.test","type":0,"title":"curl message","content":"请问何时发货？"}' | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/messages" | jq .
curl --noproxy '*' -fsS -o /dev/null -w '%{http_code}\n' -X DELETE \
  "$BASE_URL/api/v1/me/messages/$MESSAGE_ID" "${AUTH[@]}"
```

也可在 POST 中携带 `${AUTH[@]}` 绑定已登录用户；空 `content`、非整数 `type` 或超过字段长度限制应返回 400。

## 广告

```bash
curl --noproxy '*' -fsS "$BASE_URL/cycle-image.xml"
curl --noproxy '*' -fsS "$BASE_URL/api/v1/ads/1" | jq .
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/ads/1/click" \
  -H 'Content-Type: application/json' -d '{"referer":"curl.example"}' | jq .
```

不存在、未启用或不在投放时间窗内的广告读取和点击均返回 404。

## 商品图库

```bash
curl --noproxy '*' -fsS "$BASE_URL/api/v1/goods/12/gallery" | jq .
```

无图库或不存在商品应返回 404。

## 地址与报价

```bash
curl --noproxy '*' -fsS "$BASE_URL/api/v1/shipping-options?country_id=1&province_id=2&city_id=52&district_id=500" | jq .
ADDRESS=$(curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/me/addresses" "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"consignee":"测试用户","country_id":1,"province_id":2,"city_id":52,"district_id":500,"address":"中山路 1 号","mobile":"13800000000","zipcode":"200000","is_default":true}')
ADDRESS_ID=$(printf '%s' "$ADDRESS" | jq -r .id)

curl --noproxy '*' -i -sS -X DELETE "$BASE_URL/api/v1/me/addresses/$ADDRESS_ID" "${AUTH[@]}"

curl --noproxy '*' -fsS "$BASE_URL/api/v1/checkout/options" "${AUTH[@]}" | jq .
QUOTE=$(curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/checkout/quote" "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d "{\"address_id\":$ADDRESS_ID,\"shipping_id\":1,\"payment_id\":1}")
printf '%s\n' "$QUOTE" | jq .
```

## 创建订单

```bash
ORDER_BODY="{\"address_id\":$ADDRESS_ID,\"shipping_id\":1,\"payment_id\":1,\"remark\":\"curl test\"}"
ORDER=$(curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/orders" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -H 'Idempotency-Key: curl-order-001' \
  -d "$ORDER_BODY")
printf '%s\n' "$ORDER" | jq .

# 重放同一个请求：HTTP 200，返回相同订单和 replayed:true
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/orders" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -H 'Idempotency-Key: curl-order-001' \
  -d "$ORDER_BODY" | jq .

curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/orders" "${AUTH[@]}" | jq .
ORDER_ID=$(printf '%s' "$ORDER" | jq -r .id)
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/orders/$ORDER_ID" "${AUTH[@]}" | jq .

# 仅用于 SQLite 本地验收：为学习用户准备 20 元可用余额。UAT/生产由充值回调入账。
sqlite3 data/ecshop.sqlite3 \
  "INSERT INTO ecs_account_balance(user_id,available_cents,frozen_cents) SELECT user_id,2000,0 FROM ecs_users WHERE user_name='curl-user' ON CONFLICT(user_id) DO UPDATE SET available_cents=2000,frozen_cents=0"
curl --noproxy '*' -fsS -X PATCH "$BASE_URL/api/v1/me/orders/$ORDER_ID/surplus" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -d '{"amount":"10.00"}' | jq .
curl --noproxy '*' -fsS -X PATCH "$BASE_URL/api/v1/me/orders/$ORDER_ID/payment" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -d '{"payment_id":2}' | jq .
curl --noproxy '*' -fsS -X PATCH "$BASE_URL/api/v1/me/orders/$ORDER_ID/address" "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"consignee":"Alice Updated","email":"alice@example.com","address":"Shanghai updated address","zipcode":"200000","tel":"021-12345678","mobile":"13900000000","sign_building":"Building A","best_time":"18:00-20:00"}' | jq .

# 用户团购历史；普通订单未关联 order_promotion 时列表为空。
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/group-buys" "${AUTH[@]}" | jq .
curl --noproxy '*' -sS "$BASE_URL/api/v1/me/group-buys/1" "${AUTH[@]}" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/affiliate?page=1&page_size=20" "${AUTH[@]}" | jq .

# 邮件订阅使用 outbox；本地 SQLite 从 payload 读取模拟邮件中的一次性 token。
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/newsletter-subscriptions" \
  -H 'Content-Type: application/json' -d '{"email":"newsletter@example.com"}' | jq .
NEWSLETTER_TOKEN=$(sqlite3 data/ecshop.sqlite3 \
  "SELECT json_extract(payload,'$.token') FROM ecs_email_outbox WHERE template_name='newsletter_subscribe' ORDER BY message_id DESC LIMIT 1")
curl --noproxy '*' -sS -o /dev/null -w '%{http_code}\n' -X POST \
  "$BASE_URL/api/v1/newsletter-subscriptions/confirm" -H 'Content-Type: application/json' \
  -d "{\"token\":\"$NEWSLETTER_TOKEN\"}"
curl --noproxy '*' -fsS -X DELETE "$BASE_URL/api/v1/newsletter-subscriptions" \
  -H 'Content-Type: application/json' -d '{"email":"newsletter@example.com"}' | jq .
NEWSLETTER_UNSUBSCRIBE_TOKEN=$(sqlite3 data/ecshop.sqlite3 \
  "SELECT json_extract(payload,'$.token') FROM ecs_email_outbox WHERE template_name='newsletter_unsubscribe' ORDER BY message_id DESC LIMIT 1")
curl --noproxy '*' -sS -o /dev/null -w '%{http_code}\n' -X POST \
  "$BASE_URL/api/v1/newsletter-unsubscriptions/confirm" -H 'Content-Type: application/json' \
  -d "{\"token\":\"$NEWSLETTER_UNSUBSCRIBE_TOKEN\"}"

# 仅 pending_payment 订单可取消；成功后库存回补，已使用的 10 元余额也会退款。
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/me/orders/$ORDER_ID/cancel" "${AUTH[@]}" | jq .

# 已支付订单可确认收货；待支付/已取消/已收货订单返回 409。
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/me/orders/$ORDER_ID/received" "${AUTH[@]}" | jq .

# 把订单中当前仍可售、有库存的商品恢复到购物车。
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/me/orders/$ORDER_ID/cart" "${AUTH[@]}" | jq .

# 合并两个待支付订单（请替换为两个实际订单 ID）。
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/me/orders/merge" "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"from_order_id":101,"to_order_id":102}' | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/orders/by-number/$ORDER_SN/status" "${AUTH[@]}" | jq .
```

将 `quantity` 改成 0 应得到 400 + `validation_error`；地址 ID 换成其他用户的地址或不存在的 ID，应得到 404。同一幂等键传入不同 `remark` 应得到 409；订单成功后 GET `/me/cart` 应为空。重复发送取消订单请求应得到 409，且库存只回补一次。

## 商品评论

```bash
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/comments" "${AUTH[@]}" | jq .
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/goods/12/comments" "${AUTH[@]}" \
  -H 'Content-Type: application/json' -d '{"content":"curl comment"}' | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/goods/12/comments" | jq .
curl --noproxy '*' -fsS -o /dev/null -w '%{http_code}\n' -X DELETE \
  "$BASE_URL/api/v1/me/comments/$COMMENT_ID" "${AUTH[@]}"
```

不带 Bearer token 发表应得到 401；向下架或不存在商品发表/读取应得到 404。

## 商品标签

```bash
curl --noproxy '*' -fsS "$BASE_URL/api/v1/tags" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/tags" "${AUTH[@]}" | jq .
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/goods/12/tags" "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"tag":"curl-tag-a,curl-tag-b,curl-tag-a"}' | jq .
curl --noproxy '*' -fsS -o /dev/null -w '%{http_code}\n' -X DELETE "$BASE_URL/api/v1/me/tags" "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"tag":"curl-tag-a"}'

# 当前用户的缺货登记
curl --noproxy '*' -fsS "$BASE_URL/api/v1/me/bookings" "${AUTH[@]}" | jq .
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/me/bookings" "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"goods_id":12,"goods_number":2,"description":"curl booking","linkman":"Alice","email":"alice@example.com","telephone":"13800000000"}' | jq .
curl --noproxy '*' -fsS -o /dev/null -w '%{http_code}\n' -X DELETE \
  "$BASE_URL/api/v1/me/bookings/$BOOKING_ID" "${AUTH[@]}"
```

同一用户对同一商品重复提交相同标签时，响应中该标签计数不应增加；不带 `${AUTH[@]}` 应得到 401。

## 支付回调

开发环境的回调 secret 来自 `config/app.dev.conf`；生产请改为环境变量。订单创建成功后：

```bash
CALLBACK_BODY="{\"provider_trade_no\":\"curl-trade-001\",\"order_sn\":\"$(printf '%s' \"$ORDER\" | jq -r .order_sn)\",\"amount\":\"$(printf '%s' \"$ORDER\" | jq -r .order_amount)\"}"
SIGNATURE=$(printf '%s' "$CALLBACK_BODY" | openssl dgst -sha256 -hmac "$PAYMENT_CALLBACK_SECRET" -hex | sed 's/^.* //')
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/payments/mockpay/callback" \
  -H 'Content-Type: application/json' -H "X-Payment-Signature: $SIGNATURE" \
  -d "$CALLBACK_BODY" | jq .
```

同一 body 和签名重发应返回 `replayed:true`。错误签名应得到 401；金额不同应得到 409。

## 促销活动

```bash
curl --noproxy '*' -fsS "$BASE_URL/api/v1/promotions" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/promotions?type=1" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/promotions/1" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/activities" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/packages" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/topics/1" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/votes/current" | jq .
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/votes/1/responses" \
  -H 'Content-Type: application/json' -d '{"option_ids":[1]}' | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/exchange-goods?category_id=1&integral_min=400&integral_max=900&sort=exchange_integral&order=asc" | jq .
curl --noproxy '*' -fsS "$BASE_URL/api/v1/exchange-goods/12" | jq .
```

`type` 不是 0--4 的整数应得到 400；不在活动时间窗内或已结束活动的详情应得到 404。

## 修改密码（最后执行）

该接口成功后会撤销当前用户的全部 token，所以应放在其他认证接口验收之后：

```bash
# 模拟密码重置邮件，确认后重新登录取得新 token。
RESET_TOKEN=$(sqlite3 data/ecshop.sqlite3 \
  "SELECT json_extract(payload,'$.token') FROM ecs_email_outbox WHERE template_name='password_reset' ORDER BY message_id DESC LIMIT 1")
curl --noproxy '*' -i -sS -X POST "$BASE_URL/api/v1/password-resets/confirm" \
  -H 'Content-Type: application/json' \
  -d "{\"token\":\"$RESET_TOKEN\",\"new_password\":\"reset-correct-horse-battery-staple\"}"
LOGIN=$(curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"username":"curl-user","password":"reset-correct-horse-battery-staple"}')
TOKEN=$(printf '%s' "$LOGIN" | jq -r .access_token)
AUTH=(-H "Authorization: Bearer $TOKEN")

curl --noproxy '*' -i -sS -X PATCH "$BASE_URL/api/v1/me/password" "${AUTH[@]}" \
  -H 'Content-Type: application/json' \
  -d '{"current_password":"reset-correct-horse-battery-staple","new_password":"new-correct-horse-battery-staple"}'

# 旧 token 已失效，应返回 401。
curl --noproxy '*' -sS -o /dev/null -w '%{http_code}\n' \
  "$BASE_URL/api/v1/me" "${AUTH[@]}"

# 新密码可以重新登录。
curl --noproxy '*' -fsS -X POST "$BASE_URL/api/v1/auth/login" \
  -H 'Content-Type: application/json' \
  -d '{"username":"curl-user","password":"new-correct-horse-battery-staple"}' | jq .
```

测试结束后删除临时 cookie：`rm -f "$COOKIE_JAR"`。
