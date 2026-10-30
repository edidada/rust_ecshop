#!/usr/bin/env bash
# curl-driven endpoint journey test (Linux bash and Git-for-Windows bash).
#
# Every business family is exercised the way a user would drive it:
#   create -> query -> mutate -> query -> delete -> query
# Bodies with runtime values (address ids, merge ids, pm receiver) are stamped
# into target/curl-api-tmp copies of the committed templates under docs/curl/.
# Every response lands in docs/curl/responses/NN-*.response.json (committed,
# tokens redacted). The script only speaks HTTP - the caller owns the server
# process with a FRESH database per run (see README / CI workflow).
#
# usage: scripts/curl_api_test.sh
# env:   CURL_API_BASE        default http://127.0.0.1:28995
#        CURL_API_RESPONSES   default docs/curl/responses
#        ECSHOP_CHINABANK_KEY / ECSHOP_ALIPAY_KEY / ECSHOP_ALIPAY_PARTNER /
#        ECSHOP_ALIPAY_ACCOUNT enable the legacy MD5 channel journeys.
set -u

BASE="${CURL_API_BASE:-http://127.0.0.1:28995}"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
BODIES="$HERE/docs/curl"
RES="${CURL_API_RESPONSES:-$BODIES/responses}"
TMP="$HERE/target/curl-api-tmp"
PY="$(command -v python || command -v python3)"
mkdir -p "$RES" "$TMP"
: > "$RES/run.log"
STEP=0
LAST=""
CODE=""
STOP() { echo "JOURNEY FAILED: $*"; tail -c 400 "$RES/$LAST" 2>/dev/null; echo; exit 1; }

run() { # run NAME METHOD PATH [curl extras...]
  local name="$1" method="$2" path="$3"; shift 3
  STEP=$((STEP + 1))
  local file; file=$(printf '%02d-%s.response.json' "$STEP" "$name")
  CODE=$(curl -sS -o "$RES/$file" -w '%{http_code}' -X "$method" "$@" "$BASE$path") || CODE=000
  LAST="$file"
  printf '%s\n' "$file <- $method $BASE$path" >> "$RES/run.log"
  echo "  $file ($CODE) $method $BASE$path"
}
tc() { [ "$CODE" = "$1" ] || STOP "status $CODE != $1"; }
grab() { "$PY" -c 'import json,sys;d=json.load(open(sys.argv[1],encoding="utf-8"));print(eval(sys.argv[2]))' "$RES/$1" "$2" || STOP "json path $2"; }
jv() { local got; got="$(grab "$1" "$2")"; [ "$got" = "$3" ] || STOP "$1 $2 = '$got' want '$3'"; }
redact() { "$PY" -c 'import json,sys;p=sys.argv[1];d=json.load(open(p,encoding="utf-8"));d["access_token"]="***redacted***";json.dump(d,open(p,"w",encoding="utf-8"),ensure_ascii=False,indent=2)' "$RES/$1"; }
dybody() { "$PY" -c 'import json,sys;d=json.load(open(sys.argv[1],encoding="utf-8"));d.update(json.loads(sys.argv[2]));json.dump(d,open(sys.argv[3],"w",encoding="utf-8"),ensure_ascii=False)' "$1" "$2" "$3" || exit 1; }
md5s() { "$PY" -c 'import hashlib,sys;print(hashlib.md5(sys.argv[1].encode()).hexdigest())' "$1"; }
runtext() { # runtext NAME PATH urlencoded-body-file    (legacy form posts, text reply)
  local name="$1" path="$2" form="$3"
  STEP=$((STEP + 1))
  local file; file=$(printf '%02d-%s.response.txt' "$STEP" "$name")
  CODE=$(curl -sS -o "$RES/$file" -w '%{http_code}' -X POST "$BASE$path" \
    --data-binary "@$form" -H 'Content-Type: application/x-www-form-urlencoded') || CODE=000
  LAST="$file"
  printf '%s\n' "$file <- POST $BASE$path (form)" >> "$RES/run.log"
  echo "  $file ($CODE) legacy $path"
}
ta() { local got; got="$(cat "$RES/$LAST")"; [ "$got" = "$1" ] || STOP "$LAST body '$got' want '$1'"; }
jsonform() { # urlencode a python-dict snapshot of runtime fields
  "$PY" -c 'import json,sys;from urllib.parse import urlencode;print(urlencode(json.loads(sys.argv[1])))' "$1" > "$TMP/form" || exit 1
}

J='Content-Type: application/json'

echo "### smoke: public catalog/articles/widgets URLs"
run healthz GET /healthz; tc 200
run home GET /api/v1/home; tc 200
run catalog GET /api/v1/catalog; tc 200
run goods-list GET /api/v1/goods; tc 200
run goods-detail GET /api/v1/goods/12; tc 200; jv "$LAST" "d['id']" "12"
run goods-price-quote POST /api/v1/goods/12/price-quote --data-binary "@$BODIES/cart-add.body.json" -H "$J"; tc 200
run goods-gallery GET /api/v1/goods/12/gallery; tc 200
run brands GET /api/v1/brands; tc 200
run brand-goods GET /api/v1/brands/1/goods; tc 200
run category-goods GET /api/v1/categories/1/goods; tc 200
run regions GET /api/v1/regions; tc 200
run article GET /api/v1/articles/1; tc 200
run article-category GET /api/v1/article-categories/1/articles; tc 200
run shipping-options GET /api/v1/shipping-options; tc 200
run quotation GET /api/v1/quotation; tc 200
run activities GET /api/v1/activities; tc 200
run packages GET /api/v1/packages; tc 200
run promotions GET /api/v1/promotions; tc 200
run promotion-detail GET /api/v1/promotions/1; tc 200
run topic-detail GET /api/v1/topics/1; tc 200
run votes-current GET /api/v1/votes/current; tc 200
run exchange-goods-list GET /api/v1/exchange-goods; tc 200
run exchange-goods-detail GET /api/v1/exchange-goods/12; tc 200
run tags-cloud GET /api/v1/tags; tc 200
run cycle-image GET /cycle-image.xml; tc 200
run goods-widget GET /goods-widget.js; tc 200
run sitemap GET /sitemap.xml; tc 200
run feed GET /feed.xml; tc 200
run compat-search GET /api.php; tc 200
run certi GET /api/v1/certi; tc 200

echo "### auth: availability, register, login, profile, password round-trip"
run auth-availability-username POST /api/v1/auth/availability/username --data-binary "@$BODIES/availability-username.body.json" -H "$J"; tc 200
run auth-availability-email POST /api/v1/auth/availability/email --data-binary "@$BODIES/availability-email.body.json" -H "$J"; tc 200
run register POST /api/v1/auth/register --data-binary "@$BODIES/register.body.json" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
jv "$LAST" "d['username']" "curl-demo"
UID1="$(grab "$LAST" "d['user_id']")"
redact "$LAST"
run login POST /api/v1/auth/login --data-binary "@$BODIES/login.body.json" -H "$J"; tc 200
[ "$(grab "$LAST" "d['user_id']")" = "$UID1" ] || STOP "login did not round-trip the created account"
TOKENA="$(grab "$LAST" "d['access_token']")"; redact "$LAST"
run me GET /api/v1/me -H "Authorization: Bearer $TOKENA"; tc 200; jv "$LAST" "d['username']" "curl-demo"
run me-patch PATCH /api/v1/me --data-binary "@$BODIES/me-patch.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 204
run me-after-patch GET /api/v1/me -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "d['email']" "curl-demo-update@example.test"
run password-patch PATCH /api/v1/me/password --data-binary "@$BODIES/change-password.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 204
run login-old-after-change POST /api/v1/auth/login --data-binary "@$BODIES/login.body.json" -H "$J"; tc 401
run login-new POST /api/v1/auth/login --data-binary "@$BODIES/login-changed.body.json" -H "$J"; tc 200
TOKENA="$(grab "$LAST" "d['access_token']")"; redact "$LAST"
run password-revert PATCH /api/v1/me/password --data-binary "@$BODIES/change-password-revert.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 204
run login-after-revert POST /api/v1/auth/login --data-binary "@$BODIES/login.body.json" -H "$J"; tc 200
TOKENA="$(grab "$LAST" "d['access_token']")"; redact "$LAST"

echo "### second user (messages receiver); pm template resolved here"
run register-2 POST /api/v1/auth/register --data-binary "@$BODIES/register-2.body.json" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
UID2="$(grab "$LAST" "d['user_id']")"; redact "$LAST"
dybody "$BODIES/login.body.json" '{"username": "curl-demo-2"}' "$TMP/login-2.json"
run login-2 POST /api/v1/auth/login --data-binary "@$TMP/login-2.json" -H "$J"; tc 200
TOKENB="$(grab "$LAST" "d['access_token']")"; redact "$LAST"
dybody "$BODIES/pm.body.json" "{\"to_user_id\": $UID2}" "$TMP/pm.json"

echo "### addresses: create, list, mutate, list, delete, list (ecs_user_address)"
run address-create POST /api/v1/me/addresses --data-binary "@$BODIES/address-create.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
ADDRESS_ID="$(grab "$LAST" "d['id']")"
run address-create-2 POST /api/v1/me/addresses --data-binary "@$BODIES/address-create.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
ADDRESS_ID2="$(grab "$LAST" "d['id']")"
run address-list GET /api/v1/me/addresses -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "2"
jv "$LAST" "next(x for x in d['items'] if x['id']==$ADDRESS_ID)['consignee']" "张三"
run address-update PATCH "/api/v1/me/addresses/$ADDRESS_ID2" --data-binary "@$BODIES/address-update.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 204
run address-after-update GET /api/v1/me/addresses -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "next(x for x in d['items'] if x['id']==$ADDRESS_ID2)['consignee']" "李四"
jv "$LAST" "next(x for x in d['items'] if x['id']==$ADDRESS_ID2)['mobile']" "13900000002"
run address-delete DELETE "/api/v1/me/addresses/$ADDRESS_ID2" -H "Authorization: Bearer $TOKENA"; tc 204
run address-after-delete GET /api/v1/me/addresses -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "1"
run address-delete-missing DELETE "/api/v1/me/addresses/$ADDRESS_ID2" -H "Authorization: Bearer $TOKENA"; tc 404
dybody "$BODIES/checkout-quote.body.json" "{\"address_id\": $ADDRESS_ID}" "$TMP/quote.json"
dybody "$BODIES/order-create.body.json" "{\"address_id\": $ADDRESS_ID}" "$TMP/order-create.json"

echo "### favorites: create, list, mutate, list, delete, list (ecs_collect_goods)"
run favorite-create POST /api/v1/me/favorites --data-binary "@$BODIES/favorite.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
FAV_ID="$(grab "$LAST" "d['id']")"
run favorite-list GET /api/v1/me/favorites -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "1"
jv "$LAST" "d['items'][0]['goods_id']" "12"
run favorite-update PATCH "/api/v1/me/favorites/$FAV_ID" --data-binary "@$BODIES/favorite-patch.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 204
run favorite-after-update GET /api/v1/me/favorites -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "d['items'][0]['attention']" "True"
run favorite-delete DELETE "/api/v1/me/favorites/$FAV_ID" -H "Authorization: Bearer $TOKENA"; tc 204
run favorite-after-delete GET /api/v1/me/favorites -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "0"

echo "### booking: create, list, delete, list (ecs_booking_goods)"
run booking-create POST /api/v1/me/bookings --data-binary "@$BODIES/booking.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
BOOK_ID="$(grab "$LAST" "d['id']")"
run booking-list GET /api/v1/me/bookings -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "1"
jv "$LAST" "d['items'][0]['goods_number']" "2"
run booking-delete DELETE "/api/v1/me/bookings/$BOOK_ID" -H "Authorization: Bearer $TOKENA"; tc 204
run booking-after-delete GET /api/v1/me/bookings -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "0"

echo "### private messages: send to partner, receiver lists, delete by receiver"
run pm-create POST /api/v1/me/pms --data-binary "@$TMP/pm.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
PM_ID="$(grab "$LAST" "d['id']")"
run pm-receiver-list GET /api/v1/me/pms -H "Authorization: Bearer $TOKENB"; tc 200
jv "$LAST" "len(d['items'])" "1"
jv "$LAST" "d['items'][0]['title']" "curl 站内信"
run pm-delete DELETE "/api/v1/me/pms/$PM_ID" -H "Authorization: Bearer $TOKENB"; tc 204
run pm-after-delete GET /api/v1/me/pms -H "Authorization: Bearer $TOKENB"; tc 200
jv "$LAST" "len(d['items'])" "0"

echo "### community: message, comment lifecycle, tag lifecycle (ecs_feedback)"
run message-create POST /api/v1/messages --data-binary "@$BODIES/message.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
run me-messages GET /api/v1/me/messages -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "1"
MSG_ID="$(grab "$LAST" "d['items'][0]['id']")"
run message-delete DELETE "/api/v1/me/messages/$MSG_ID" -H "Authorization: Bearer $TOKENA"; tc 204
run me-messages-after-delete GET /api/v1/me/messages -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "0"
run comment-create POST /api/v1/goods/12/comments --data-binary "@$BODIES/comment.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
run me-comments GET /api/v1/me/comments -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "1"
CM_ID="$(grab "$LAST" "d['items'][0]['id']")"
run me-comment-delete DELETE "/api/v1/me/comments/$CM_ID" -H "Authorization: Bearer $TOKENA"; tc 204
run me-comments-after GET /api/v1/me/comments -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "0"
run tag-create POST /api/v1/goods/12/tags --data-binary "@$BODIES/tag.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
run me-tags GET /api/v1/me/tags -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "1"
run me-tag-delete DELETE /api/v1/me/tags --data-binary "@$BODIES/tag.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 204
run me-tags-after GET /api/v1/me/tags -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "0"

echo "### cart: add, list, mutate, list, item delete, list (ecs_cart)"
run cart-clear DELETE /api/v1/me/cart -H "Authorization: Bearer $TOKENA"; tc 204
run cart-add POST /api/v1/me/cart --data-binary "@$BODIES/cart-add.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
CART_ID="$(grab "$LAST" "d['id']")"
CART_VER="$(grab "$LAST" "d['version']")"
dybody "$BODIES/cart-patch.body.json" "{\"quantity\": 2, \"version\": $CART_VER}" "$TMP/cart-patch.json"
run cart-get GET /api/v1/me/cart -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "1"
jv "$LAST" "d['items'][0]['price']" "49.90"
run cart-patch PATCH "/api/v1/me/cart/$CART_ID" --data-binary "@$TMP/cart-patch.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 204
run cart-after-patch GET /api/v1/me/cart -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "d['items'][0]['quantity']" "2"
run cart-item-delete DELETE "/api/v1/me/cart/$CART_ID" -H "Authorization: Bearer $TOKENA"; tc 204
run cart-after-item-delete GET /api/v1/me/cart -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "0"

echo "### checkout selections and funds"
run cart-add-checkout POST /api/v1/me/cart --data-binary "@$BODIES/cart-add.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
run checkout-options GET /api/v1/checkout/options -H "Authorization: Bearer $TOKENA"; tc 200
run checkout-quote POST /api/v1/checkout/quote --data-binary "@$TMP/quote.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 200
run checkout-funds GET /api/v1/checkout/funds -H "Authorization: Bearer $TOKENA"; tc 200
run select-pack PATCH /api/v1/checkout/selection/pack --data-binary "@$BODIES/select-pack.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 200
run select-card PATCH /api/v1/checkout/selection/card --data-binary "@$BODIES/select-card.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 200
run checkout-selection GET /api/v1/checkout/selection -H "Authorization: Bearer $TOKENA"; tc 200
run quote-after-selection POST /api/v1/checkout/quote --data-binary "@$TMP/quote.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 200

echo "### order A: create, query, payment mutate+mock pay, ship, receive"
run order-create POST /api/v1/orders --data-binary "@$TMP/order-create.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
OID1="$(grab "$LAST" "d['id']")"
ORDER_SN="$(grab "$LAST" "d['order_sn']")"
run order-detail GET "/api/v1/me/orders/$OID1" -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "d['status']" "pending_payment"
jv "$LAST" "d['goods_amount']" "49.90"
run order-status-by-sn GET "/api/v1/me/orders/by-number/$ORDER_SN/status" -H "Authorization: Bearer $TOKENA"; tc 200
run order-payment-patch PATCH "/api/v1/me/orders/$OID1/payment" --data-binary "@$BODIES/order-payment.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 200
jv "$LAST" "d['payment_fee']" "2.00"
run order-after-payment GET "/api/v1/me/orders/$OID1" -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "d['goods_amount']" "49.90"
run mock-pay POST /api/v1/mock/orders/$OID1/pay --data-binary "@$BODIES/mock-pay.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 200
jv "$LAST" "d['replayed']" "False"
run mock-pay-replay POST /api/v1/mock/orders/$OID1/pay --data-binary "@$BODIES/mock-pay.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 200
jv "$LAST" "d['replayed']" "True"
run order-after-pay GET "/api/v1/me/orders/$OID1" -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "d['status']" "paid"
run received-before-ship POST "/api/v1/me/orders/$OID1/received" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 409
run mock-ship POST /api/v1/mock/orders/$OID1/ship -H "Authorization: Bearer $TOKENA" -H "$J"; tc 200
$OID1="${OID1}"; run receive-php GET "/receive.php?id=$OID1" -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "d['status']" "received"
run received-after-ship POST "/api/v1/me/orders/$OID1/received" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 409

echo "### order B: cancel, query, cancel again 409, return-to-cart"
run cart-add-b POST /api/v1/me/cart --data-binary "@$BODIES/cart-add.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
run order-create-b POST /api/v1/orders --data-binary "@$TMP/order-create.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
OID2="$(grab "$LAST" "d['id']")"
run order-cancel POST "/api/v1/me/orders/$OID2/cancel" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 200
run order-after-cancel GET "/api/v1/me/orders/$OID2" -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "d['status']" "cancelled"
run order-cancel-again POST "/api/v1/me/orders/$OID2/cancel" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 409
run order-return-to-cart POST "/api/v1/me/orders/$OID2/cart" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 200
run cart-after-return GET /api/v1/me/cart -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['items'])" "1"

echo "### orders C+D merge -> merged order holds both order goods lines"
run cart-clear-c DELETE /api/v1/me/cart -H "Authorization: Bearer $TOKENA"; tc 204
run cart-add-c POST /api/v1/me/cart --data-binary "@$BODIES/cart-add.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
run order-create-c POST /api/v1/orders --data-binary "@$TMP/order-create.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
OIDC="$(grab "$LAST" "d['id']")"
run cart-clear-d DELETE /api/v1/me/cart -H "Authorization: Bearer $TOKENA"; tc 204
run cart-add-d POST /api/v1/me/cart --data-binary "@$BODIES/cart-add.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
run order-create-d POST /api/v1/orders --data-binary "@$TMP/order-create.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
OID_D="$(grab "$LAST" "d['id']")"
dybody "$BODIES/order-merge.body.json" "{\"from_order_id\": $OIDC, \"to_order_id\": $OID_D}" "$TMP/merge.json"
run order-merge POST /api/v1/me/orders/merge --data-binary "@$TMP/merge.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
jv "$LAST" "d['goods_amount']" "99.80"
OID_M="$(grab "$LAST" "d['id']")"
run merged-order-detail GET "/api/v1/me/orders/$OID_M" -H "Authorization: Bearer $TOKENA"; tc 200
jv "$LAST" "len(d['goods'])" "2"
jv "$LAST" "d['goods_amount']" "99.80"



echo "### legacy channels (env-gated): intents + chinabank/alipay MD5 + unknown plugin"
CH="${ECSHOP_CHINABANK_KEY:-}"
AL="${ECSHOP_ALIPAY_KEY:-}"
PARTNER="${ECSHOP_ALIPAY_PARTNER:-}"
ACCOUNT="${ECSHOP_ALIPAY_ACCOUNT:-}"
alsign() { # alsign <out.json> <fields-json>  (ksort md5 minus sign/sign_type/code)
  "$PY" -c '
import json,hashlib,sys
fields=json.loads(sys.argv[2])
src="&".join(f"{k}={fields[k]}" for k in sorted(fields) if k not in ("sign","sign_type","code"))+sys.argv[1]
fields["sign_type"]="MD5"; fields["sign"]=hashlib.md5(src.encode()).hexdigest()
print(json.dumps(fields))
' "$AL" "$2" > "$1" || exit 1
}
if [ -n "$CH" ]; then
  run intent-unknown-provider POST "/api/v1/me/orders/$OID_M/payment-intents" --data-binary "@$BODIES/payment-intent-unknown.body.json" -H "Authorization: Bearer $TOKENA" -H "$J"; tc 400
  run cart-clear-f DELETE /api/v1/me/cart -H "Authorization: Bearer $TOKENA"; tc 204
  run cart-add-for-f POST /api/v1/me/cart --data-binary "@$BODIES/cart-add.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
  run order-create-f POST /api/v1/orders --data-binary "@$TMP/order-create.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
  OIDF="$(grab "$LAST" "d['id']")"
  run intent-chinabank POST "/api/v1/me/orders/$OIDF/payment-intents" --data-binary "@$BODIES/payment-intent-chinabank.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
  SNF="$(grab "$LAST" "d['intent_sn']")"
  AMTF="$(grab "$LAST" "d['amount']")"
  SIGOK="$(md5s "${SNF}20${AMTF}CNY$CH" | tr 'a-z' 'A-Z')"
  jsonform "{\"v_oid\":\"$SNF\",\"v_pstatus\":\"20\",\"v_amount\":\"$AMTF\",\"v_moneytype\":\"CNY\",\"v_md5str\":\"$SIGOK\"}"
  runtext chinabank-ok /chinabank_receive.php "$TMP/form"; tc 200; ta "ok"
  run order-after-chinabank GET "/api/v1/me/orders/$OIDF" -H "Authorization: Bearer $TOKENA"; tc 200
  jv "$LAST" "d['status']" "paid"
  runtext chinabank-replay /chinabank_receive.php "$TMP/form"; tc 200; ta "ok"
  jsonform "{\"v_oid\":\"$SNF\",\"v_pstatus\":\"20\",\"v_amount\":\"$AMTF\",\"v_moneytype\":\"CNY\",\"v_md5str\":\"00000000000000000000000000000000\"}"
  runtext chinabank-forged /chinabank_receive.php "$TMP/form"; tc 200; ta "error"
  SIGBAD="$(md5s "${SNF}20${AMTF}CNY$CH" | tr 'a-z' 'A-Z')"
  jsonform "{\"v_oid\":\"$SNF\",\"v_pstatus\":\"30\",\"v_amount\":\"$AMTF\",\"v_moneytype\":\"CNY\",\"v_md5str\":\"$SIGBAD\"}"
  runtext chinabank-checked /chinabank_receive.php "$TMP/form"; tc 200; ta "error"
  jsonform "{\"v_oid\":\"$SNF\",\"v_pstatus\":\"20\",\"v_amount\":\"1.00\",\"v_moneytype\":\"CNY\",\"v_md5str\":\"$SIGOK\"}"
  runtext chinabank-amount-mismatch /chinabank_receive.php "$TMP/form"; tc 200; ta "error"
fi
if [ -n "$AL" ]; then
  run cart-clear-g DELETE /api/v1/me/cart -H "Authorization: Bearer $TOKENA"; tc 204
  run cart-add-for-g POST /api/v1/me/cart --data-binary "@$BODIES/cart-add.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
  run order-create-g POST /api/v1/orders --data-binary "@$TMP/order-create.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
  OIDG="$(grab "$LAST" "d['id']")"
  run intent-alipay POST "/api/v1/me/orders/$OIDG/payment-intents" --data-binary "@$BODIES/payment-intent-alipay.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
  SNG="$(grab "$LAST" "d['intent_sn']")"
  AMTG="$(grab "$LAST" "d['amount']")"
  alsign "$TMP/ali-ok.json" "{\"out_trade_no\":\"$SNG\",\"total_fee\":\"$AMTG\",\"subject\":\"curl order\",\"trade_no\":\"ALI-$SNG\",\"trade_status\":\"TRADE_SUCCESS\",\"partner\":\"$PARTNER\",\"seller_email\":\"$ACCOUNT\"}"
  jsonform "$(cat "$TMP/ali-ok.json")"
  runtext alipay-ok "/respond.php?code=alipay" "$TMP/form"; tc 200; ta "success"
  run order-after-alipay GET "/api/v1/me/orders/$OIDG" -H "Authorization: Bearer $TOKENA"; tc 200
  jv "$LAST" "d['status']" "paid"
  "$PY" -c 'import json,sys;d=json.load(open(sys.argv[1],encoding="utf-8"));d["sign"]="0"*32;print(json.dumps(d))' "$TMP/ali-ok.json" > "$TMP/ali-forged.json"
  jsonform "$(cat "$TMP/ali-forged.json")"
  runtext alipay-forged "/respond.php?code=alipay" "$TMP/form"; tc 200; ta "fail"
  run cart-clear-h DELETE /api/v1/me/cart -H "Authorization: Bearer $TOKENA"; tc 204
  run cart-add-for-h POST /api/v1/me/cart --data-binary "@$BODIES/cart-add.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
  run order-create-h POST /api/v1/orders --data-binary "@$TMP/order-create.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
  OIDI="$(grab "$LAST" "d['id']")"
  run intent-alipay-i POST "/api/v1/me/orders/$OIDI/payment-intents" --data-binary "@$BODIES/payment-intent-alipay.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
  SNI="$(grab "$LAST" "d['intent_sn']")"
  AMTI="$(grab "$LAST" "d['amount']")"
  alsign "$TMP/ali-wait.json" "{\"out_trade_no\":\"$SNI\",\"total_fee\":\"$AMTI\",\"subject\":\"curl order\",\"trade_no\":\"ALI-$SNI\",\"trade_status\":\"WAIT_BUYER_PAY\",\"partner\":\"$PARTNER\",\"seller_email\":\"$ACCOUNT\"}"
  jsonform "$(cat "$TMP/ali-wait.json")"
  runtext alipay-intermediate "/respond.php?code=alipay" "$TMP/form"; tc 200; ta "fail"
  run order-still-pending GET "/api/v1/me/orders/$OIDI" -H "Authorization: Bearer $TOKENA"; tc 200
  jv "$LAST" "d['status']" "pending_payment"
  run intent-alipay-i2 POST "/api/v1/me/orders/$OIDI/payment-intents" --data-binary "@$BODIES/payment-intent-alipay.body.json" -H "Authorization: Bearer $TOKENA" -H "$J" -H "Idempotency-Key: order-$STEP"; tc 201
  SNI2="$(grab "$LAST" "d['intent_sn']")"
  alsign "$TMP/ali-stale.json" "{\"out_trade_no\":\"$SNI2\",\"total_fee\":\"1.00\",\"subject\":\"curl order\",\"trade_no\":\"ALI-$SNI2\",\"trade_status\":\"TRADE_SUCCESS\",\"partner\":\"$PARTNER\",\"seller_email\":\"$ACCOUNT\"}"
  jsonform "$(cat "$TMP/ali-stale.json")"
  runtext alipay-amount-mismatch "/respond.php?code=alipay" "$TMP/form"; tc 200; ta "fail"
  alsign "$TMP/ali-evil.json" "{\"out_trade_no\":\"$SNI2\",\"total_fee\":\"$AMTI\",\"subject\":\"curl order\",\"trade_no\":\"ALI-$SNI2\",\"trade_status\":\"TRADE_SUCCESS\",\"partner\":\"evil-partner\",\"seller_email\":\"$ACCOUNT\"}"
  jsonform "$(cat "$TMP/ali-evil.json")"
  runtext alipay-partner-forged "/respond.php?code=alipay" "$TMP/form"; tc 200; ta "fail"
  printf '{"payment_status": "Completed"}' > "$TMP/paypal.json"
  runtext unknown-plugin "/respond.php?code=paypal" "$TMP/paypal.json"; tc 400
fi

echo "### logout invalidates the session (delete, query -> 401)"
run logout POST /api/v1/auth/logout -H "Authorization: Bearer $TOKENB" -H "$J"; tc 204
run me-after-logout GET /api/v1/me -H "Authorization: Bearer $TOKENB"; tc 401

echo
echo "JOURNEY OK: $STEP recorded responses in $RES"
