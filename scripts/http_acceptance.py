"""Remote-only HTTP acceptance: no real payment, email, carrier, or UCenter calls."""
import json
import hashlib
import os
import urllib.parse
from pathlib import Path
import re
import sqlite3
import subprocess
import time
import urllib.request
import urllib.error

BASE = "http://127.0.0.1:28991"
ROOT = Path(__file__).resolve().parents[1]
DB = ROOT / "target" / "http-acceptance.sqlite3"
RESULTS = []
TOKEN = None

# Server database URL comes from the environment (sqlite by default, or a
# mysql:// URL for the real-MySQL CI job). Seeding uses the matching client.
DATABASE_URL = "sqlite://" + DB.as_posix()


def mysql_connect():
    import mysql.connector

    url = urllib.parse.urlparse(DATABASE_URL)
    return mysql.connector.connect(
        host=url.hostname or "127.0.0.1",
        port=url.port or 3306,
        user=url.username or "root",
        password=urllib.parse.unquote(url.password or ""),
        database=url.path.lstrip("/"),
        autocommit=True,
    )


def translate_sql(sql):
    sql = sql.replace("INSERT OR IGNORE", "INSERT IGNORE")
    sql = sql.replace("INSERT OR REPLACE", "REPLACE")
    return sql


def request(method, path, body=None, status=200, auth=True, key=None):
    headers = {"Content-Type": "application/json"}
    if auth and TOKEN:
        headers["Authorization"] = "Bearer " + TOKEN
    if key:
        headers["Idempotency-Key"] = key
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(BASE + path, data=data, headers=headers, method=method)
    try:
        response = urllib.request.urlopen(req, timeout=20)
    except urllib.error.HTTPError as error:
        response = error
    raw = response.read().decode()
    RESULTS.append({"method": method, "path": path, "status": response.code})
    assert response.code == status, (method, path, response.code, status, raw)
    return json.loads(raw) if raw and raw[0] in "[{" else raw


def seed(sql, params=()):
    if DATABASE_URL.startswith("mysql:"):
        with mysql_connect() as connection:
            connection.cursor().execute(translate_sql(sql), params)
    else:
        with sqlite3.connect(DB, timeout=20) as connection:
            connection.execute(sql, params)


def post_form(path, fields, status=200):
    """Legacy urlencoded probes (plain-text gateways)."""
    data = urllib.parse.urlencode(fields).encode()
    req = urllib.request.Request(
        BASE + path,
        data=data,
        headers={"Content-Type": "application/x-www-form-urlencoded"},
        method="POST",
    )
    try:
        response = urllib.request.urlopen(req, timeout=20)
    except urllib.error.HTTPError as error:
        response = error
    raw = response.read().decode()
    RESULTS.append({"method": "POST-form", "path": path, "status": response.code, "body": raw})
    assert response.code == status, ("POST-form", path, response.code, status, raw)
    return raw


def md5_hex(text):
    return hashlib.md5(text.encode()).hexdigest()


def run_legacy_payments(aid, uid):
    """Chinabank/Alipay MD5 adapters against server-side payment intents.

    Both channels verify signatures first; only then does the server-side
    intent settle the order once. Replays stay idempotent, every forged or
    rejected notice leaves the order pending.
    """
    chinabank_key = os.environ.get("ECSHOP_CHINABANK_KEY", "")
    alipay_key = os.environ.get("ECSHOP_ALIPAY_KEY", "")
    alipay_partner = os.environ.get("ECSHOP_ALIPAY_PARTNER", "")
    alipay_account = os.environ.get("ECSHOP_ALIPAY_ACCOUNT", "")
    if not (chinabank_key and alipay_key and alipay_partner and alipay_account):
        RESULTS.append({"skipped": "legacy payment journeys", "reason": "channel env not configured"})
        return

    request("DELETE", "/api/v1/me/cart", status=204)

    def start_order(key):
        request("POST", "/api/v1/me/cart", {"goods_id": 12, "quantity": 1}, 201)
        return request(
            "POST", "/api/v1/orders",
            {"address_id": aid, "shipping_id": 1, "payment_id": 1}, 201, key=key,
        )

    def order_status(order):
        return request("GET", f'/api/v1/me/orders/{order["id"]}')["status"]

    def new_intent(order, provider):
        return request(
            "POST", f'/api/v1/me/orders/{order["id"]}/payment-intents',
            {"provider": provider}, 201,
        )

    # ---- Chinabank: signature = upper(md5(v_oid+v_pstatus+v_amount+v_moneytype+key)) ----
    cb = start_order("cb-order")
    intent = new_intent(cb, "chinabank")
    assert intent["amount"] == cb["order_amount"], intent

    def cb_form(intent_row, pstatus="20", amount=None, moneytype="CNY"):
        amount = amount or intent_row["amount"]
        signature = md5_hex(
            f"{intent_row['intent_sn']}{pstatus}{amount}{moneytype}{chinabank_key}"
        ).upper()
        return {
            "v_oid": intent_row["intent_sn"],
            "v_pstatus": pstatus,
            "v_amount": amount,
            "v_moneytype": moneytype,
            "v_md5str": signature,
        }

    assert post_form("/chinabank_receive.php", cb_form(intent)) == "ok"
    assert order_status(cb) == "paid"
    # Verified replay of the settled channel transaction stays idempotent.
    assert post_form("/chinabank_receive.php", cb_form(intent)) == "ok"
    forged = cb_form(intent)
    forged["v_md5str"] = "0" * 32
    assert post_form("/chinabank_receive.php", forged) == "error"

    cb2 = start_order("cb-order-2")
    intent2 = new_intent(cb2, "chinabank")
    # Verified failure and protocol mismatches never touch the order state.
    assert post_form("/chinabank_receive.php", cb_form(intent2, pstatus="30")) == "error"
    assert order_status(cb2) == "pending_payment"
    assert post_form("/chinabank_receive.php", cb_form(intent2, moneytype="USD")) == "error"
    assert post_form("/chinabank_receive.php", cb_form(intent2, amount="1.00")) == "error"
    assert order_status(cb2) == "pending_payment"
    request("POST", f'/api/v1/me/orders/{cb2["id"]}/cancel', {})

    # ---- Alipay: ksort full-field MD5 verified at /respond.php ----
    ali = start_order("ali-order")
    intent3 = new_intent(ali, "alipay")

    def ali_form(intent_row, trade_no, total=None, status="TRADE_SUCCESS", partner=alipay_partner):
        fields = {
            "trade_no": trade_no,
            "out_trade_no": intent_row["intent_sn"],
            "total_fee": total or intent_row["amount"],
            "subject": "cpp-books",
            "trade_status": status,
            "partner": partner,
            "seller_email": alipay_account,
        }
        source = "&".join(f"{key}={value}" for key, value in sorted(fields.items())) + alipay_key
        fields["sign_type"] = "MD5"
        fields["sign"] = md5_hex(source)
        return fields

    fields = ali_form(intent3, f"ALI-{intent3['intent_sn']}")
    assert post_form("/respond.php?code=alipay", fields) == "success"
    assert order_status(ali) == "paid"
    assert post_form("/respond.php?code=alipay", fields) == "success"  # replay
    forged = dict(fields)
    forged["sign"] = "0" * 32
    assert post_form("/respond.php?code=alipay", forged) == "fail"

    pend = start_order("ali-order-2")
    intent4 = new_intent(pend, "alipay")
    # Verified intermediate status: signature ok, but the order never settles.
    fields_b = ali_form(intent4, f"ALI-{intent4['intent_sn']}", status="WAIT_BUYER_PAY")
    assert post_form("/respond.php?code=alipay", fields_b) == "fail"
    assert order_status(pend) == "pending_payment"
    # A fresh intent invalidates the previous one of the same order.
    intent5 = new_intent(pend, "alipay")
    assert post_form("/respond.php?code=alipay", ali_form(intent4, f"ALI-{intent4['intent_sn']}")) == "fail"
    # Amount tampering on the live intent fails even with a valid signature.
    assert post_form("/respond.php?code=alipay", ali_form(intent5, f"ALI-{intent5['intent_sn']}", total="1.00")) == "fail"
    assert order_status(pend) == "pending_payment"

    other = start_order("ali-order-3")
    intent6 = new_intent(other, "alipay")
    # Cross-order trade-number reuse is rejected despite a valid signature.
    fields_c = ali_form(intent6, f"ALI-{intent3['intent_sn']}")
    assert post_form("/respond.php?code=alipay", fields_c) == "fail"
    assert order_status(other) == "pending_payment"
    request("POST", f'/api/v1/me/orders/{other["id"]}/cancel', {})

    standalone = start_order("ali-order-4")
    intent7 = new_intent(standalone, "alipay")
    fields_d = ali_form(intent7, f"ALI-{intent7['intent_sn']}", partner="evil-partner")
    assert post_form("/respond.php?code=alipay", fields_d) == "fail"
    assert order_status(standalone) == "pending_payment"
    request("POST", f'/api/v1/me/orders/{standalone["id"]}/cancel', {})
    request("POST", f'/api/v1/me/orders/{pend["id"]}/cancel', {})

    # Unknown historical plugins stay explicitly disabled.
    post_form("/respond.php?code=paypal", {"payment_status": "Completed"}, status=400)


def run():
    global TOKEN
    register = request("POST", "/api/v1/auth/register", {"username": "acceptance", "email": "acceptance@example.test", "password": "acceptance-password", "agreement_accepted": True}, 201)
    TOKEN = register["access_token"]
    uid = register["user_id"]
    seed("INSERT INTO ecs_account_balance(user_id,available_cents) VALUES(?,100000)", (uid,))
    seed("UPDATE ecs_users SET pay_points=10000 WHERE user_id=?", (uid,))
    address = request("POST", "/api/v1/me/addresses", {"consignee": "Test", "address": "Test address", "mobile": "13800000000", "country_id": 1, "province_id": 2, "city_id": 0, "district_id": 0}, 201)
    aid = address["id"]
    order_body = {"address_id": aid, "shipping_id": 1, "payment_id": 2}
    # Nonzero fee must be included in balance settlement.
    request("POST", "/api/v1/me/cart", {"goods_id": 12, "quantity": 1}, 201)
    quote = request("POST", "/api/v1/checkout/quote", order_body)
    order = request("POST", "/api/v1/orders", order_body, 201, key="order-1")
    oid = order["id"]
    assert order["order_amount"] == quote["order_amount"]
    assert request("POST", "/api/v1/orders", order_body, key="order-1")["replayed"]
    request("PATCH", f"/api/v1/me/orders/{oid}/surplus", {"amount": "10.00"})
    request("POST", f"/api/v1/mock/orders/{oid}/pay", {"provider_trade_no": "mock-1"})
    assert request("POST", f"/api/v1/mock/orders/{oid}/pay", {"provider_trade_no": "mock-1"})["replayed"]
    request("POST", f"/api/v1/me/orders/{oid}/received", {}, 409)
    request("POST", f"/api/v1/mock/orders/{oid}/ship", {})
    request("POST", f"/api/v1/me/orders/{oid}/received", {})
    # Selections influence quote and order, and cancellation restores assets.
    request("POST", "/api/v1/me/cart/linked-buy", {"goods_id": 12, "quantity": 1})
    for field, value in [("pack", 1), ("card", 1), ("integral", 100), ("surplus", "1.00"), ("invoice", 1), ("invoice-title", "Test invoice"), ("oos", "cancel"), ("insure", 1)]:
        request("PATCH", f"/api/v1/checkout/selection/{field}", {"value": value})
    quote = request("POST", "/api/v1/checkout/quote", order_body)
    order = request("POST", "/api/v1/orders", order_body, 201, key="order-2")
    assert order["order_amount"] == quote["order_amount"]
    request("POST", f'/api/v1/me/orders/{order["id"]}/cancel', {})
    request("POST", f'/api/v1/me/orders/{order["id"]}/cancel', {}, 409)
    # Product disabled after adding to cart cannot be ordered.
    request("POST", "/api/v1/me/cart", {"goods_id": 12, "quantity": 1}, 201)
    seed("UPDATE ecs_goods SET is_on_sale=0 WHERE goods_id=12")
    request("POST", "/api/v1/orders", order_body, 409, key="unavailable")
    seed("UPDATE ecs_goods SET is_on_sale=1 WHERE goods_id=12")
    request("DELETE", "/api/v1/me/cart", status=204)
    request("POST", "/api/v1/me/cart/packages/2", {})
    request("POST", "/api/v1/checkout/quote", order_body)
    request("DELETE", "/api/v1/me/cart", status=204)
    redemption = request("POST", "/api/v1/exchange-goods/12/redemptions", {"quantity": 1}, 201, key="exchange-1")
    assert redemption["points"] == 500
    assert request("POST", "/api/v1/exchange-goods/12/redemptions", {"quantity": 1}, key="exchange-1")["replayed"]
    request("POST", "/api/v1/exchange-goods/12/redemptions", {"quantity": 2}, 409, key="exchange-1")
    request("POST", "/api/v1/promotions/1/orders", dict(order_body, quantity=1), 201, key="group-1")
    seed("INSERT INTO ecs_goods_activity(act_id,act_name,act_type,goods_id,start_time,end_time,ext_info) VALUES(99,'Test auction',0,12,0,4102444800,?)", (json.dumps({"start_price": "1.00", "amplitude": "1.00", "deposit": "2.00"}),))
    request("POST", "/api/v1/promotions/99/bids", {"price": "1.00"}, 201)
    request("POST", "/api/v1/promotions/99/bids", {"price": "2.00"}, 409)
    request("POST", "/api/v1/promotions/99/bids", {"price": "0.50"}, 400)
    seed("UPDATE ecs_users SET is_validated=1 WHERE user_id=?", (uid,))
    request("PATCH", "/api/v1/me", {"email": "new@example.test"}, 204)
    assert not request("GET", "/api/v1/me")["email_verified"]
    request("POST", "/api/v1/me/email-verifications", {}, 202)
    assert request("GET", "/api/v1/mock/mailbox")["items"]
    request("GET", "/api/v1/checkout/funds")
    request("GET", "/api/v1/checkout/selection")
    # SKU quote and order prices share the same calculator and reserve SKU stock.
    request("DELETE", "/api/v1/me/cart", status=204)
    sku = {"goods_id": 12, "quantity": 1, "product_id": 81, "attribute_ids": [1001, 1005]}
    quoted = request("POST", "/api/v1/goods/12/price-quote", sku)
    request("POST", "/api/v1/me/cart", sku, 201)
    cart = request("GET", "/api/v1/me/cart")
    assert cart["items"][0]["price"] == quoted["unit_price"]
    order = request("POST", "/api/v1/orders", order_body, 201, key="sku-order")
    request("POST", f'/api/v1/me/orders/{order["id"]}/cancel', {})
    # --- Membership and tier pricing share the server-side calculator ---
    seed("INSERT IGNORE INTO ecs_user_rank(rank_id,rank_name,discount_percent) VALUES(1,'VIP 会员',90)")
    seed("INSERT IGNORE INTO ecs_user_membership(user_id,rank_id) VALUES(?,1)", (uid,))
    seed("INSERT IGNORE INTO ecs_member_price(goods_id,rank_id,user_price) VALUES(12,1,'39.90')")
    seed("INSERT IGNORE INTO ecs_volume_price(goods_id,volume_number,volume_price) VALUES(14,3,'60.00')")
    request("DELETE", "/api/v1/me/cart", status=204)
    first = request("POST", "/api/v1/me/cart", {"goods_id": 12, "quantity": 1}, 201)
    second = request("POST", "/api/v1/me/cart", {"goods_id": 14, "quantity": 3}, 201)
    cart = request("GET", "/api/v1/me/cart")
    prices = {i["goods_id"]: i["price"] for i in cart["items"]}
    assert prices[12] == "39.90", prices  # member price beats rank discount
    assert prices[14] == "60.00", prices  # volume tier at quantity >= 3
    request("PATCH", f'/api/v1/me/cart/{second["id"]}', {"quantity": 1, "version": second["version"]}, 204)
    cart = request("GET", "/api/v1/me/cart")
    prices = {i["goods_id"]: i["price"] for i in cart["items"]}
    assert prices[14] == "71.91", prices  # 90% rank discount below the tier line
    quote = request("POST", "/api/v1/checkout/quote", {"address_id": aid, "shipping_id": 1, "payment_id": 1})
    member_order = request("POST", "/api/v1/orders", {"address_id": aid, "shipping_id": 1, "payment_id": 1}, 201, key="member-order")
    assert member_order["order_amount"] == quote["order_amount"]
    request("POST", f'/api/v1/me/orders/{member_order["id"]}/cancel', {})
    seed("UPDATE ecs_user_membership SET rank_id=0 WHERE user_id=?", (uid,))
    # --- Scoped favourable offers, rank gating and free gifts ---
    schema = ",act_range_ext,min_amount,max_amount,act_type,act_type_ext,gift"
    seed(
        "INSERT IGNORE INTO ecs_favourable_activity(act_id,act_name,start_time,end_time,user_rank,act_range"
        + schema + ") VALUES(3,'全场减五',0,4102444800,'0,1',0,'','0.00','0.00',1,'5.00','[]')",
    )
    seed(
        "INSERT IGNORE INTO ecs_favourable_activity(act_id,act_name,start_time,end_time,user_rank,act_range"
        + schema + ") VALUES(4,'VIP 专属',0,4102444800,'2',0,'','0.00','0.00',1,'5.00','[]')",
    )
    seed(
        "INSERT IGNORE INTO ecs_favourable_activity(act_id,act_name,start_time,end_time,user_rank,act_range"
        + schema + ") VALUES(5,'分类赠品',0,4102444800,'0',1,'1','0.00','0.00',0,1,'[{\"id\":14,\"name\":\"C++ 进阶商品\"}]')",
    )
    request("POST", "/api/v1/me/cart/favourables/3", {}, 200)
    request("POST", "/api/v1/me/cart", {"goods_id": 12, "quantity": 1}, 201)
    quote = request("POST", "/api/v1/checkout/quote", {"address_id": aid, "shipping_id": 1, "payment_id": 1})
    assert quote["discount"] == "5.00", quote
    promo_order = request("POST", "/api/v1/orders", {"address_id": aid, "shipping_id": 1, "payment_id": 1}, 201, key="promo-order")
    assert promo_order["order_amount"] == quote["order_amount"]
    request("POST", "/api/v1/me/cart/favourables/4", {}, 403)
    request("POST", "/api/v1/me/cart/favourables/5", {"gifts": [{"goods_id": 13, "quantity": 1}]}, 400)
    request("POST", "/api/v1/me/cart/favourables/5", {"gifts": [{"goods_id": 14, "quantity": 2}]}, 400)
    stock_before = request("GET", "/api/v1/goods/14")["stock"]
    request("POST", "/api/v1/me/cart/favourables/5", {"gifts": [{"goods_id": 14, "quantity": 1}]}, 200)
    request("POST", "/api/v1/me/cart", {"goods_id": 12, "quantity": 1}, 201)
    quote = request("POST", "/api/v1/checkout/quote", {"address_id": aid, "shipping_id": 1, "payment_id": 1})
    assert len(quote["gifts"]) == 1 and quote["gifts"][0]["price"] == "0.00", quote
    gift_order = request("POST", "/api/v1/orders", {"address_id": aid, "shipping_id": 1, "payment_id": 1}, 201, key="gift-order")
    detail = request("GET", f'/api/v1/me/orders/{gift_order["id"]}')
    assert any(
        g["goods_id"] == 14 and g["price"] == "0.00" for g in detail["goods"]
    ), detail["goods"]
    assert request("GET", "/api/v1/goods/14")["stock"] == stock_before - 1
    request("POST", f'/api/v1/me/orders/{gift_order["id"]}/cancel', {})
    assert request("GET", "/api/v1/goods/14")["stock"] == stock_before  # gift stock restored
    # --- Legacy gateway adapters (keys from the environment) ---
    run_legacy_payments(aid, uid)
    # Both fixed newsletter paths must supply their action without a Path extractor.
    for action, pending, token in [("subscriptions", "subscribe", "a"*64), ("unsubscriptions", "unsubscribe", "b"*64)]:
        seed("INSERT OR REPLACE INTO ecs_email_list(email,status,token_hash,pending_action,token_expires_at) VALUES('newsletter@example.test',0,?,?,4102444800)", (hashlib.sha256(token.encode()).hexdigest(), pending))
        request("POST", f"/api/v1/newsletter-{action}/confirm", {"token":token}, 204)
        request("POST", f"/api/v1/newsletter-{action}/confirm", {"token":token}, 409)
    # Every registered URL is probed; successful business journeys above are distinct
    # from this route/error smoke coverage. Unknown resource IDs may return 404.
    source = (ROOT / "src/http/routes/mod.rs").read_text(encoding="utf-8-sig")
    routes = re.findall(r'\.route\(\s*"([^"]+)"\s*,\s*(.*?)(?=\n\s*\.(?:route|nest)|\n\})', source, re.S)
    for template, handler in routes:
        path = re.sub(r":\w+", "1", template)
        for method in re.findall(r"\b(get|post|patch|delete)\(", handler):
            req = urllib.request.Request(BASE + path, method=method.upper(), data=b"{}" if method != "get" else None, headers={"Content-Type": "application/json"})
            try:
                response = urllib.request.urlopen(req, timeout=20)
            except urllib.error.HTTPError as error:
                response = error
            raw = response.read().decode()
            RESULTS.append({"method": method.upper(), "path": path, "status": response.code, "kind": "route-smoke"})
            assert response.code < 500 and response.code != 405, (method, path, response.code, raw)
    assert len(routes) >= 100, len(routes)
    seed("UPDATE ecs_sessions SET created_at=0 WHERE user_id=?", (uid,))
    request("GET", "/api/v1/me", status=401)
    print(f"PASS: {len(RESULTS)} requests, {len(routes)} registered URL paths")


if __name__ == "__main__":
    # A mysql:// ECSHOP_DATABASE_URL (CI) is passed through to the server and
    # seedings use the connector; sqlite remains the local default.
    if os.environ.get("ECSHOP_DATABASE_URL"):
        DATABASE_URL = os.environ["ECSHOP_DATABASE_URL"]
    if DATABASE_URL.startswith("sqlite:"):
        DB.parent.mkdir(exist_ok=True)
        if DB.exists():
            DB.unlink()
    env = dict(os.environ, ECSHOP_PORT="28991", ECSHOP_DATABASE_URL=DATABASE_URL, ECSHOP_INTEGRATION_MODE="mock", ECSHOP_ENV="test")
    executable = ROOT / "target/debug" / ("rust_ecshop.exe" if os.name == "nt" else "rust_ecshop")
    with (ROOT / "target/http-server.log").open("w", encoding="utf-8") as log:
        process = subprocess.Popen([str(executable)], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            for attempt in range(60):
                if process.poll() is not None:
                    raise RuntimeError("server exited; inspect target/http-server.log")
                try:
                    urllib.request.urlopen(BASE + "/healthz", timeout=1).close()
                    break
                except (OSError, urllib.error.URLError):
                    time.sleep(0.5)
            else:
                raise RuntimeError("server readiness timeout")
            run()
        finally:
            process.terminate()
            process.wait(timeout=10)
            (ROOT / "target/http-results.json").write_text(json.dumps(RESULTS, indent=2), encoding="utf-8")
