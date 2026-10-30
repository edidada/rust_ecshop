"""Remote-only HTTP acceptance: no real payment, email, carrier, or UCenter calls."""
import json
import hashlib
import os
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
    with sqlite3.connect(DB, timeout=20) as connection:
        connection.execute(sql, params)


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
    DB.parent.mkdir(exist_ok=True)
    if DB.exists():
        DB.unlink()
    env = dict(os.environ, ECSHOP_PORT="28991", ECSHOP_DATABASE_URL="sqlite://" + DB.as_posix(), ECSHOP_INTEGRATION_MODE="mock", ECSHOP_ENV="test")
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
