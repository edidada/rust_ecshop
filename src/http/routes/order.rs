//! Order context: create (idempotent), list, detail, lifecycle actions, merge, surplus, payment/address changes, group buys.
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use rbatis::rbatis::RBatis;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::rbutil::{
    col_i64, col_str, col_cents, col_f64, e, insert_id, q, q1,
};
use crate::shared::util::{cents_to_string, parse_money_cents, unix_now, PageParams};

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation("id must be a positive integer".to_string())),
    }
}

const ORDER_SUMMARY_COLS: &str =
    "order_id AS order_id, order_sn AS order_sn, order_status AS order_status, goods_amount AS goods_amount,
     shipping_fee AS shipping_fee, payment_fee AS payment_fee, order_amount AS order_amount, created_at AS created_at";

fn order_summary(row: &Value) -> Value {
    json!({
        "id": col_i64(row, "order_id"),
        "order_sn": col_str(row, "order_sn"),
        "status": col_str(row, "order_status"),
        "goods_amount": cents_to_string(col_cents(row, "goods_amount")),
        "shipping_fee": cents_to_string(col_cents(row, "shipping_fee")),
        "payment_fee": cents_to_string(col_cents(row, "payment_fee")),
        "order_amount": cents_to_string(col_cents(row, "order_amount")),
        "created_at": col_i64(row, "created_at"),
    })
}

async fn fetch_order_summary_tx<E: rbatis::executor::Executor>(
    tx: &E,
    order_id: i64,
    user_id: i64,
) -> Result<Value, AppError> {
    q1(
        tx,
        &format!("SELECT {ORDER_SUMMARY_COLS} FROM ecs_order_info WHERE order_id = ? AND user_id = ?"),
        vec![json!(order_id), json!(user_id)],
    )
    .await?
    .map(|r| order_summary(&r))
    .ok_or_else(|| AppError::NotFound("order not found".to_string()))
}

fn gen_order_sn() -> String {
    let now = unix_now();
    let rand_part: u32 = rand::random::<u32>() % 10_000;
    format!("EC{now}{rand_part:04}")
}

#[derive(Deserialize, serde::Serialize)]
pub struct OrderCreateRequest {
    address_id: i64,
    shipping_id: i64,
    payment_id: i64,
    #[serde(default)]
    remark: String,
}

/// POST /api/v1/orders — requires Idempotency-Key; re-reads cart, conditional stock deduction,
/// order + order_goods snapshots, cart clear, all in one transaction.
pub async fn order_create(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: HeaderMap,
    Json(body): Json<OrderCreateRequest>,
) -> Result<axum::response::Response, AppError> {
    let idempotency_key = headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Validation("Idempotency-Key header is required".to_string()))?;
    let fingerprint = crate::infrastructure::crypto::sha256_hex(
        serde_json::to_string(&body).unwrap_or_default().as_bytes(),
    );
    let rb = state.rb;
    let user_id = auth.user_id;
    let tx = crate::infrastructure::db::begin(rb).await?;
    // Idempotency replay handling.
    let existing = q1(
        &tx,
        "SELECT order_id AS order_id, request_fingerprint AS request_fingerprint
         FROM ecs_order_info WHERE user_id = ? AND idempotency_key = ?",
        vec![json!(user_id), json!(idempotency_key)],
    )
    .await?;
    if let Some(existing) = existing {
        if col_str(&existing, "request_fingerprint") == fingerprint {
            let mut v = fetch_order_summary_tx(&tx, col_i64(&existing, "order_id"), user_id).await?;
            v["replayed"] = json!(true);
            return Ok((StatusCode::OK, Json(v)).into_response());
        }
        return Err(AppError::Conflict("idempotency key reused with different body".to_string()));
    }
    // Address must belong to the user.
    let address = q1(
        &tx,
        "SELECT consignee AS consignee, address AS address, mobile AS mobile
         FROM ecs_user_address WHERE address_id = ? AND user_id = ?",
        vec![json!(body.address_id), json!(user_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("address not found".to_string()))?;
    let shipping_fee = q1(
        &tx,
        "SELECT shipping_fee AS shipping_fee FROM ecs_shipping WHERE shipping_id = ? AND enabled = 1",
        vec![json!(body.shipping_id)],
    )
    .await?
    .map(|r| col_cents(&r, "shipping_fee"))
    .ok_or_else(|| AppError::NotFound("shipping not found or disabled".to_string()))?;
    let payment_fee = q1(
        &tx,
        "SELECT pay_fee AS pay_fee FROM ecs_payment WHERE pay_id = ? AND enabled = 1",
        vec![json!(body.payment_id)],
    )
    .await?
    .map(|r| col_cents(&r, "pay_fee"))
    .ok_or_else(|| AppError::NotFound("payment not found or disabled".to_string()))?;
    // Re-read cart inside the transaction.
    let cart_rows = q(
        &tx,
        "SELECT c.goods_id AS goods_id, c.goods_number AS goods_number, g.goods_name AS goods_name, g.shop_price AS shop_price
         FROM ecs_cart c JOIN ecs_goods g ON g.goods_id = c.goods_id
         WHERE c.user_id = ? ORDER BY c.goods_id",
        vec![json!(user_id)],
    )
    .await?;
    if cart_rows.is_empty() {
        return Err(AppError::Validation("cart is empty".to_string()));
    }
    // Conditional stock deduction: UPDATE ... WHERE stock >= quantity.
    for row in &cart_rows {
        let goods_id = col_i64(row, "goods_id");
        let quantity = col_i64(row, "goods_number");
        let updated = e(
            &tx,
            "UPDATE ecs_goods SET goods_number = goods_number - ? WHERE goods_id = ? AND goods_number >= ?",
            vec![json!(quantity), json!(goods_id), json!(quantity)],
        )
        .await?;
        if updated.rows_affected != 1 {
            return Err(AppError::Conflict("insufficient stock or unavailable goods".to_string()));
        }
    }
    let goods_amount: i64 = cart_rows
        .iter()
        .try_fold(0i64, |acc, row| {
            let price = col_cents(row, "shop_price");
            let qty = col_i64(row, "goods_number");
            acc.checked_add(price.checked_mul(qty)?)
        })
        .ok_or_else(|| AppError::Validation("amount overflow".to_string()))?;
    let order_amount = goods_amount + shipping_fee + payment_fee;
    let order_sn = gen_order_sn();
    let result = e(
        &tx,
        "INSERT INTO ecs_order_info (order_sn, user_id, order_status, consignee, address, mobile, shipping_id, pay_id, goods_amount, shipping_fee, payment_fee, order_amount, idempotency_key, request_fingerprint, remark, created_at)
         VALUES (?, ?, 'pending_payment', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            json!(order_sn),
            json!(user_id),
            json!(col_str(&address, "consignee")),
            json!(col_str(&address, "address")),
            json!(col_str(&address, "mobile")),
            json!(body.shipping_id),
            json!(body.payment_id),
            json!(goods_amount as f64 / 100.0),
            json!(shipping_fee as f64 / 100.0),
            json!(payment_fee as f64 / 100.0),
            json!(order_amount as f64 / 100.0),
            json!(idempotency_key),
            json!(fingerprint),
            json!(body.remark),
            json!(unix_now()),
        ],
    )
    .await?;
    let order_id = insert_id(&result);
    for row in &cart_rows {
        e(
            &tx,
            "INSERT INTO ecs_order_goods (order_id, goods_id, goods_name, goods_number, goods_price)
             VALUES (?, ?, ?, ?, ?)",
            vec![
                json!(order_id),
                json!(col_i64(row, "goods_id")),
                json!(col_str(row, "goods_name")),
                json!(col_i64(row, "goods_number")),
                json!(col_f64(row, "shop_price")),
            ],
        )
        .await?;
    }
    e(
        &tx,
        "DELETE FROM ecs_cart WHERE user_id = ?",
        vec![json!(user_id)],
    )
    .await?;
    e(
        &tx,
        "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?, ?, 'create', '', ?)",
        vec![json!(order_id), json!(user_id), json!(unix_now())],
    )
    .await?;
    let mut v = fetch_order_summary_tx(&tx, order_id, user_id).await?;
    tx.commit().await?;
    v["replayed"] = json!(false);
    Ok((StatusCode::CREATED, Json(v)).into_response())
}

/// GET /api/v1/me/orders
pub async fn order_list(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(page): Query<PageParams>,
) -> Result<Json<Value>, AppError> {
    let (page_no, page_size, offset) = page.resolve();
    let rb = state.rb;
    let total = q1(
        rb,
        "SELECT COUNT(*) AS cnt FROM ecs_order_info WHERE user_id = ?",
        vec![json!(auth.user_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    let items = q(
        rb,
        &format!(
            "SELECT {ORDER_SUMMARY_COLS} FROM ecs_order_info WHERE user_id = ? ORDER BY order_id DESC LIMIT ? OFFSET ?"
        ),
        vec![json!(auth.user_id), json!(page_size), json!(offset)],
    )
    .await?
    .iter()
    .map(order_summary)
    .collect::<Vec<_>>();
    Ok(Json(json!({"page": page_no, "page_size": page_size, "total": total, "items": items})))
}

/// GET /api/v1/me/orders/{id}
pub async fn order_detail(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let order_id = parse_id(&id)?;
    let rb = state.rb;
    let mut summary = q1(
        rb,
        &format!("SELECT {ORDER_SUMMARY_COLS} FROM ecs_order_info WHERE order_id = ? AND user_id = ?"),
        vec![json!(order_id), json!(auth.user_id)],
    )
    .await?
    .map(|r| order_summary(&r))
    .ok_or_else(|| AppError::NotFound("order not found".to_string()))?;
    let goods = q(
        rb,
        "SELECT rec_id AS rec_id, goods_id AS goods_id, goods_name AS goods_name, goods_number AS goods_number, goods_price AS goods_price
         FROM ecs_order_goods WHERE order_id = ? ORDER BY rec_id",
        vec![json!(order_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "rec_id"),
            "goods_id": col_i64(&r, "goods_id"),
            "name": col_str(&r, "goods_name"),
            "quantity": col_i64(&r, "goods_number"),
            "price": cents_to_string(col_cents(&r, "goods_price")),
        })
    })
    .collect::<Vec<_>>();
    summary["goods"] = Value::Array(goods);
    let delivery = q1(
        rb,
        "SELECT email AS email, zipcode AS zipcode, telephone AS telephone, sign_building AS sign_building, best_time AS best_time
         FROM ecs_order_delivery_address WHERE order_id = ? AND user_id = ?",
        vec![json!(order_id), json!(auth.user_id)],
    )
    .await?
    .map(|r| {
        json!({
            "email": col_str(&r, "email"),
            "zipcode": col_str(&r, "zipcode"),
            "telephone": col_str(&r, "telephone"),
            "sign_building": col_str(&r, "sign_building"),
            "best_time": col_str(&r, "best_time"),
        })
    });
    summary["delivery_address"] = delivery.unwrap_or(Value::Null);
    Ok(Json(summary))
}

/// POST /api/v1/me/orders/{id}/cancel — pending_payment only; restores stock and refunds balance once.
pub async fn order_cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let order_id = parse_id(&id)?;
    let rb = state.rb;
    let tx = crate::infrastructure::db::begin(rb).await?;
    let updated = e(
        &tx,
        "UPDATE ecs_order_info SET order_status = 'cancelled' WHERE order_id = ? AND user_id = ? AND order_status = 'pending_payment'",
        vec![json!(order_id), json!(auth.user_id)],
    )
    .await?;
    if updated.rows_affected == 0 {
        return Err(AppError::Conflict("order cannot be cancelled".to_string()));
    }
    // Restore stock exactly once (guarded by the conditional status update above).
    let rows = q(
        &tx,
        "SELECT goods_id AS goods_id, goods_number AS goods_number FROM ecs_order_goods WHERE order_id = ?",
        vec![json!(order_id)],
    )
    .await?;
    for row in &rows {
        e(
            &tx,
            "UPDATE ecs_goods SET goods_number = goods_number + ? WHERE goods_id = ?",
            vec![json!(col_i64(row, "goods_number")), json!(col_i64(row, "goods_id"))],
        )
        .await?;
    }
    // Refund balance used for this order, if any.
    let paid = q1(
        &tx,
        "SELECT paid_cents AS paid_cents FROM ecs_order_balance_payment WHERE order_id = ? AND user_id = ?",
        vec![json!(order_id), json!(auth.user_id)],
    )
    .await?
    .map(|r| col_i64(&r, "paid_cents"))
    .unwrap_or(0);
    if paid > 0 {
        e(
            &tx,
            "UPDATE ecs_account_balance SET available_cents = available_cents + ? WHERE user_id = ?",
            vec![json!(paid), json!(auth.user_id)],
        )
        .await?;
        e(
            &tx,
            "INSERT INTO ecs_account_log (user_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at)
             VALUES (?, ?, 0, 'order_cancel_refund', 'order_info', ?, ?)",
            vec![json!(auth.user_id), json!(paid), json!(order_id), json!(unix_now())],
        )
        .await?;
    }
    e(
        &tx,
        "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?, ?, 'cancel', '', ?)",
        vec![json!(order_id), json!(auth.user_id), json!(unix_now())],
    )
    .await?;
    let summary = fetch_order_summary_tx(&tx, order_id, auth.user_id).await?;
    tx.commit().await?;
    Ok(Json(summary))
}

/// POST /api/v1/me/orders/{id}/received — paid -> received.
pub async fn order_received(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let order_id = parse_id(&id)?;
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let updated = e(
        &tx,
        "UPDATE ecs_order_info SET order_status = 'received' WHERE order_id = ? AND user_id = ? AND order_status = 'paid'",
        vec![json!(order_id), json!(auth.user_id)],
    )
    .await?;
    if updated.rows_affected == 0 {
        return Err(AppError::Conflict("order is not paid".to_string()));
    }
    e(
        &tx,
        "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?, ?, 'confirm_received', '', ?)",
        vec![json!(order_id), json!(auth.user_id), json!(unix_now())],
    )
    .await?;
    let summary = fetch_order_summary_tx(&tx, order_id, auth.user_id).await?;
    tx.commit().await?;
    Ok(Json(summary))
}

/// POST /api/v1/me/orders/{id}/cart — return sellable order goods to the cart.
pub async fn order_return_to_cart(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let order_id = parse_id(&id)?;
    let rb = state.rb;
    let tx = crate::infrastructure::db::begin(rb).await?;
    let owned = q1(
        &tx,
        "SELECT COUNT(*) AS cnt FROM ecs_order_info WHERE order_id = ? AND user_id = ?",
        vec![json!(order_id), json!(auth.user_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if owned == 0 {
        return Err(AppError::NotFound("order not found".to_string()));
    }
    let rows = q(
        &tx,
        "SELECT og.goods_id AS goods_id, og.goods_number AS goods_number, g.goods_number AS stock
         FROM ecs_order_goods og JOIN ecs_goods g ON g.goods_id = og.goods_id
         WHERE og.order_id = ? AND g.is_on_sale = 1 AND g.is_delete = 0",
        vec![json!(order_id)],
    )
    .await?;
    let mut added = 0i64;
    for row in &rows {
        let goods_id = col_i64(row, "goods_id");
        let addable = col_i64(row, "goods_number").min(col_i64(row, "stock"));
        if addable <= 0 {
            continue;
        }
        e(
            &tx,
            "INSERT INTO ecs_cart (user_id, goods_id, goods_number, version)
             VALUES (?, ?, ?, 1)
             ON CONFLICT(user_id, goods_id) DO UPDATE SET
                 goods_number = MIN(goods_number + ?, (SELECT goods_number FROM ecs_goods WHERE goods_id = ?)),
                 version = version + 1",
            vec![
                json!(auth.user_id),
                json!(goods_id),
                json!(addable),
                json!(addable),
                json!(goods_id),
            ],
        )
        .await?;
        added += addable;
    }
    if added == 0 {
        return Err(AppError::Conflict("no sellable goods to return to cart".to_string()));
    }
    tx.commit().await?;
    Ok(Json(json!({"returned_quantity": added})))
}

#[derive(Deserialize)]
pub struct OrderMergeRequest {
    from_order_id: i64,
    to_order_id: i64,
}

/// POST /api/v1/me/orders/merge — merges two pending_payment orders of the current user.
pub async fn order_merge(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<OrderMergeRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if body.from_order_id == body.to_order_id {
        return Err(AppError::Validation("from and to orders must differ".to_string()));
    }
    let rb = state.rb;
    let user_id = auth.user_id;
    let tx = crate::infrastructure::db::begin(rb).await?;
    async fn load_pending<E: rbatis::executor::Executor>(
        tx: &E,
        oid: i64,
        user_id: i64,
    ) -> Result<Value, AppError> {
        q1(
            tx,
            "SELECT order_sn AS order_sn, shipping_id AS shipping_id, pay_id AS pay_id, goods_amount AS goods_amount,
                    shipping_fee AS shipping_fee, payment_fee AS payment_fee, order_amount AS order_amount,
                    remark AS remark, consignee AS consignee, address AS address, mobile AS mobile
             FROM ecs_order_info WHERE order_id = ? AND user_id = ? AND order_status = 'pending_payment'",
            vec![json!(oid), json!(user_id)],
        )
        .await?
        .ok_or_else(|| AppError::NotFound("pending order not found".to_string()))
    }
    let from = load_pending(&tx, body.from_order_id, user_id).await?;
    let to = load_pending(&tx, body.to_order_id, user_id).await?;
    // Orders that already used account balance must not be merged (funds attribution).
    let balance_used = q1(
        &tx,
        "SELECT COUNT(*) AS cnt FROM ecs_order_balance_payment WHERE order_id IN (?, ?)",
        vec![json!(body.from_order_id), json!(body.to_order_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if balance_used > 0 {
        return Err(AppError::Conflict("orders using account balance cannot be merged".to_string()));
    }
    let goods_amount = col_cents(&from, "goods_amount") + col_cents(&to, "goods_amount");
    let shipping_fee = col_cents(&from, "shipping_fee") + col_cents(&to, "shipping_fee");
    let payment_fee = col_cents(&from, "payment_fee") + col_cents(&to, "payment_fee");
    let order_amount = goods_amount + shipping_fee + payment_fee;
    let order_sn = gen_order_sn();
    let result = e(
        &tx,
        "INSERT INTO ecs_order_info (order_sn, user_id, order_status, consignee, address, mobile, shipping_id, pay_id, goods_amount, shipping_fee, payment_fee, order_amount, idempotency_key, request_fingerprint, remark, created_at)
         VALUES (?, ?, 'pending_payment', ?, ?, ?, ?, ?, ?, ?, ?, ?, '', '', ?, ?)",
        vec![
            json!(order_sn),
            json!(user_id),
            json!(col_str(&from, "consignee")),
            json!(col_str(&from, "address")),
            json!(col_str(&from, "mobile")),
            json!(col_i64(&from, "shipping_id")),
            json!(col_i64(&from, "pay_id")),
            json!(goods_amount as f64 / 100.0),
            json!(shipping_fee as f64 / 100.0),
            json!(payment_fee as f64 / 100.0),
            json!(order_amount as f64 / 100.0),
            json!(col_str(&from, "remark")),
            json!(unix_now()),
        ],
    )
    .await?;
    let new_order_id = insert_id(&result);
    // Transfer from-order goods and copy to-order goods.
    e(
        &tx,
        "UPDATE ecs_order_goods SET order_id = ? WHERE order_id = ?",
        vec![json!(new_order_id), json!(body.from_order_id)],
    )
    .await?;
    e(
        &tx,
        "INSERT INTO ecs_order_goods (order_id, goods_id, goods_name, goods_number, goods_price)
         SELECT ?, goods_id, goods_name, goods_number, goods_price FROM ecs_order_goods WHERE order_id = ?",
        vec![json!(new_order_id), json!(body.to_order_id)],
    )
    .await?;
    e(
        &tx,
        "UPDATE OR IGNORE ecs_order_delivery_address SET order_id = ? WHERE order_id = ?",
        vec![json!(new_order_id), json!(body.from_order_id)],
    )
    .await?;
    e(
        &tx,
        "INSERT OR IGNORE INTO ecs_order_delivery_address (order_id, user_id, email, zipcode, telephone, sign_building, best_time)
         SELECT ?, user_id, email, zipcode, telephone, sign_building, best_time FROM ecs_order_delivery_address WHERE order_id = ?",
        vec![json!(new_order_id), json!(body.to_order_id)],
    )
    .await?;
    e(
        &tx,
        "UPDATE OR IGNORE ecs_order_promotion SET order_id = ? WHERE order_id = ?",
        vec![json!(new_order_id), json!(body.from_order_id)],
    )
    .await?;
    e(
        &tx,
        "INSERT OR IGNORE INTO ecs_order_promotion (order_id, promotion_id, promotion_type)
         SELECT ?, promotion_id, promotion_type FROM ecs_order_promotion WHERE order_id = ?",
        vec![json!(new_order_id), json!(body.to_order_id)],
    )
    .await?;
    e(
        &tx,
        "DELETE FROM ecs_order_info WHERE order_id = ?",
        vec![json!(body.from_order_id)],
    )
    .await?;
    e(
        &tx,
        "DELETE FROM ecs_order_info WHERE order_id = ?",
        vec![json!(body.to_order_id)],
    )
    .await?;
    e(
        &tx,
        "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?, ?, 'merge', ?, ?)",
        vec![
            json!(new_order_id),
            json!(user_id),
            json!(format!("merged from {}", col_str(&from, "order_sn"))),
            json!(unix_now()),
        ],
    )
    .await?;
    let summary = fetch_order_summary_tx(&tx, new_order_id, user_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(summary)))
}

/// GET /api/v1/me/orders/by-number/{order_sn}/status
pub async fn order_status_by_sn(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(order_sn): Path<String>,
) -> Result<Json<Value>, AppError> {
    let row = q1(
        state.rb,
        "SELECT order_sn AS order_sn, order_status AS order_status, order_amount AS order_amount
         FROM ecs_order_info WHERE order_sn = ? AND user_id = ?",
        vec![json!(order_sn), json!(auth.user_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("order not found".to_string()))?;
    Ok(Json(json!({
        "order_sn": col_str(&row, "order_sn"),
        "status": col_str(&row, "order_status"),
        "order_amount": cents_to_string(col_cents(&row, "order_amount")),
    })))
}

#[derive(Deserialize)]
pub struct SurplusPatchRequest {
    amount: String,
}

/// PATCH /api/v1/me/orders/{id}/surplus — integer-cent balance deduction with audit trail.
pub async fn order_surplus(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<SurplusPatchRequest>,
) -> Result<Json<Value>, AppError> {
    let order_id = parse_id(&id)?;
    let cents = parse_money_cents(&body.amount, "amount")?;
    if cents <= 0 {
        return Err(AppError::Validation("amount must be positive".to_string()));
    }
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let order = q1(
        &tx,
        "SELECT goods_amount AS goods_amount, shipping_fee AS shipping_fee
         FROM ecs_order_info WHERE order_id = ? AND user_id = ? AND order_status = 'pending_payment'",
        vec![json!(order_id), json!(auth.user_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("pending order not found".to_string()))?;
    let total_due = col_cents(&order, "goods_amount") + col_cents(&order, "shipping_fee");
    let already_paid = q1(
        &tx,
        "SELECT paid_cents AS paid_cents FROM ecs_order_balance_payment WHERE order_id = ? AND user_id = ?",
        vec![json!(order_id), json!(auth.user_id)],
    )
    .await?
    .map(|r| col_i64(&r, "paid_cents"))
    .unwrap_or(0);
    let remaining = total_due - already_paid;
    if remaining <= 0 {
        return Err(AppError::Conflict("order already fully paid by balance".to_string()));
    }
    let apply = cents.min(remaining);
    let deducted = e(
        &tx,
        "UPDATE ecs_account_balance SET available_cents = available_cents - ?
         WHERE user_id = ? AND available_cents >= ?",
        vec![json!(apply), json!(auth.user_id), json!(apply)],
    )
    .await?;
    if deducted.rows_affected == 0 {
        return Err(AppError::Conflict("insufficient balance".to_string()));
    }
    e(
        &tx,
        "INSERT INTO ecs_order_balance_payment (order_id, user_id, paid_cents, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(order_id) DO UPDATE SET paid_cents = paid_cents + ?, updated_at = ?",
        vec![
            json!(order_id),
            json!(auth.user_id),
            json!(apply),
            json!(now),
            json!(apply),
            json!(now),
        ],
    )
    .await?;
    e(
        &tx,
        "INSERT INTO ecs_account_log (user_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at)
         VALUES (?, ?, 0, 'order_surplus_payment', 'order_info', ?, ?)",
        vec![json!(auth.user_id), json!(-apply), json!(order_id), json!(now)],
    )
    .await?;
    e(
        &tx,
        "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?, ?, 'surplus_payment', ?, ?)",
        vec![json!(order_id), json!(auth.user_id), json!(cents_to_string(apply)), json!(now)],
    )
    .await?;
    let new_paid = already_paid + apply;
    if new_paid >= total_due {
        e(
            &tx,
            "UPDATE ecs_order_info SET order_status = 'paid' WHERE order_id = ? AND order_status = 'pending_payment'",
            vec![json!(order_id)],
        )
        .await?;
    }
    let mut summary = fetch_order_summary_tx(&tx, order_id, auth.user_id).await?;
    tx.commit().await?;
    summary["balance_applied"] = json!(cents_to_string(apply));
    Ok(Json(summary))
}

#[derive(Deserialize)]
pub struct OrderPaymentPatchRequest {
    payment_id: i64,
}

/// PATCH /api/v1/me/orders/{id}/payment — switch payment method with flat fee recalculation.
pub async fn order_payment_patch(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<OrderPaymentPatchRequest>,
) -> Result<Json<Value>, AppError> {
    let order_id = parse_id(&id)?;
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let order = q1(
        &tx,
        "SELECT pay_id AS pay_id, payment_fee AS payment_fee, order_amount AS order_amount
         FROM ecs_order_info WHERE order_id = ? AND user_id = ? AND order_status = 'pending_payment'",
        vec![json!(order_id), json!(auth.user_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("pending order not found".to_string()))?;
    let current_pay_id = col_i64(&order, "pay_id");
    if current_pay_id == body.payment_id {
        return Err(AppError::Conflict("payment method unchanged".to_string()));
    }
    let new_fee = q1(
        &tx,
        "SELECT pay_fee AS pay_fee FROM ecs_payment WHERE pay_id = ? AND enabled = 1",
        vec![json!(body.payment_id)],
    )
    .await?
    .map(|r| col_cents(&r, "pay_fee"))
    .ok_or_else(|| AppError::NotFound("payment not found or disabled".to_string()))?;
    let remaining = col_cents(&order, "order_amount") - col_cents(&order, "payment_fee");
    let new_amount = remaining + new_fee;
    e(
        &tx,
        "UPDATE ecs_order_info SET pay_id = ?, payment_fee = ?, order_amount = ? WHERE order_id = ?",
        vec![
            json!(body.payment_id),
            json!(new_fee as f64 / 100.0),
            json!(new_amount as f64 / 100.0),
            json!(order_id),
        ],
    )
    .await?;
    e(
        &tx,
        "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?, ?, 'change_payment', ?, ?)",
        vec![
            json!(order_id),
            json!(auth.user_id),
            json!(format!("payment {}", body.payment_id)),
            json!(now),
        ],
    )
    .await?;
    let summary = fetch_order_summary_tx(&tx, order_id, auth.user_id).await?;
    tx.commit().await?;
    Ok(Json(summary))
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct OrderAddressPatchRequest {
    consignee: String,
    email: String,
    address: String,
    #[serde(default)]
    zipcode: String,
    #[serde(default)]
    tel: String,
    #[serde(default)]
    mobile: String,
    #[serde(default)]
    sign_building: String,
    #[serde(default)]
    best_time: String,
}

/// PATCH /api/v1/me/orders/{id}/address — updates core consignee fields plus a full snapshot row.
pub async fn order_address_patch(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<OrderAddressPatchRequest>,
) -> Result<StatusCode, AppError> {
    let order_id = parse_id(&id)?;
    if body.consignee.is_empty() || body.address.is_empty() {
        return Err(AppError::Validation("consignee and address are required".to_string()));
    }
    if !body.email.contains('@') {
        return Err(AppError::Validation("email must be valid".to_string()));
    }
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let updated = e(
        &tx,
        "UPDATE ecs_order_info SET consignee = ?, address = ?, mobile = ?
         WHERE order_id = ? AND user_id = ? AND order_status = 'pending_payment'",
        vec![
            json!(body.consignee),
            json!(body.address),
            json!(body.mobile),
            json!(order_id),
            json!(auth.user_id),
        ],
    )
    .await?;
    if updated.rows_affected == 0 {
        return Err(AppError::NotFound("pending order not found".to_string()));
    }
    e(
        &tx,
        "INSERT INTO ecs_order_delivery_address (order_id, user_id, email, zipcode, telephone, sign_building, best_time)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(order_id) DO UPDATE SET email = ?, zipcode = ?, telephone = ?, sign_building = ?, best_time = ?",
        vec![
            json!(order_id),
            json!(auth.user_id),
            json!(body.email),
            json!(body.zipcode),
            json!(body.tel),
            json!(body.sign_building),
            json!(body.best_time),
            json!(body.email),
            json!(body.zipcode),
            json!(body.tel),
            json!(body.sign_building),
            json!(body.best_time),
        ],
    )
    .await?;
    e(
        &tx,
        "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?, ?, 'save_address', '', ?)",
        vec![json!(order_id), json!(auth.user_id), json!(now)],
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/me/group-buys and /api/v1/me/group-buys/{id}
pub async fn me_group_buys(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    group_buy_items(state.rb, auth.user_id, None)
        .await
        .map(Json)
}

pub async fn me_group_buy_detail(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let promotion_id = parse_id(&id)?;
    group_buy_items(state.rb, auth.user_id, Some(promotion_id))
        .await
        .map(Json)
}

async fn group_buy_items(
    rb: &RBatis,
    user_id: i64,
    only: Option<i64>,
) -> Result<Value, AppError> {
    let items = q(
        rb,
        "SELECT p.promotion_id AS promotion_id, p.order_id AS order_id, a.act_name AS act_name, a.goods_name AS goods_name,
                o.order_amount AS order_amount, o.order_status AS order_status
         FROM ecs_order_promotion p
         JOIN ecs_order_info o ON o.order_id = p.order_id AND o.user_id = ?
         JOIN ecs_goods_activity a ON a.act_id = p.promotion_id AND a.act_type = 1
         WHERE p.promotion_type = 'group_buy'
         ORDER BY p.order_id DESC",
        vec![json!(user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "promotion_id": col_i64(&r, "promotion_id"),
            "order_id": col_i64(&r, "order_id"),
            "activity_name": col_str(&r, "act_name"),
            "goods_name": col_str(&r, "goods_name"),
            "order_amount": cents_to_string(col_cents(&r, "order_amount")),
            "order_status": col_str(&r, "order_status"),
        })
    })
    .collect::<Vec<_>>();
    match only {
        Some(pid) => items
            .into_iter()
            .find(|i| i.get("promotion_id").and_then(|v| v.as_i64()) == Some(pid))
            .ok_or_else(|| AppError::NotFound("group buy not found".to_string())),
        None => Ok(json!({"items": items})),
    }
}
