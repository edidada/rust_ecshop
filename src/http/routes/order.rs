//! Order context: create (idempotent), list, detail, lifecycle actions, merge, surplus, payment/address changes, group buys.
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};

/// Row tuple loaded by order_merge for one pending order.
type MergeOrderRow = (
    String, // order_sn
    i64,    // shipping_id
    i64,    // pay_id
    f64,    // goods_amount
    f64,    // shipping_fee
    f64,    // payment_fee
    f64,    // order_amount
    i64,    // idempotency_key not null flag
    i64,    // placeholder 0
    String, // remark
    String, // consignee
    String, // address
    String, // mobile
);

use crate::http::auth::AuthUser;
use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::util::{cents_to_string, parse_money_cents, unix_now};

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation("id must be a positive integer".to_string())),
    }
}

fn order_summary(row: &rusqlite::Row) -> rusqlite::Result<Value> {
    let goods_amount: f64 = row.get(3)?;
    let shipping_fee: f64 = row.get(4)?;
    let payment_fee: f64 = row.get(5)?;
    let order_amount: f64 = row.get(6)?;
    Ok(json!({
        "id": row.get::<_, i64>(0)?,
        "order_sn": row.get::<_, String>(1)?,
        "status": row.get::<_, String>(2)?,
        "goods_amount": cents_to_string((goods_amount * 100.0).round() as i64),
        "shipping_fee": cents_to_string((shipping_fee * 100.0).round() as i64),
        "payment_fee": cents_to_string((payment_fee * 100.0).round() as i64),
        "order_amount": cents_to_string((order_amount * 100.0).round() as i64),
        "created_at": row.get::<_, i64>(7)?,
    }))
}

const ORDER_SUMMARY_COLS: &str =
    "order_id, order_sn, order_status, goods_amount, shipping_fee, payment_fee, order_amount, created_at";

fn fetch_order_summary(
    tx: &Connection,
    order_id: i64,
    user_id: i64,
) -> Result<Value, AppError> {
    tx.query_row(
        &format!("SELECT {ORDER_SUMMARY_COLS} FROM ecs_order_info WHERE order_id = ?1 AND user_id = ?2"),
        [order_id, user_id],
        order_summary,
    )
    .optional()
    .map_err(db_err)?
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let (status, result) = tokio::task::spawn_blocking(move || -> Result<(StatusCode, Value), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        // Idempotency replay handling.
        let existing: Option<(i64, String)> = tx
            .query_row(
                "SELECT order_id, request_fingerprint FROM ecs_order_info WHERE user_id = ?1 AND idempotency_key = ?2",
                rusqlite::params![user_id, idempotency_key],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        if let Some((order_id, existing_fp)) = existing {
            if existing_fp == fingerprint {
                let summary = fetch_order_summary(&tx, order_id, user_id)?;
                let mut v = summary;
                v["replayed"] = json!(true);
                return Ok((StatusCode::OK, v));
            }
            return Err(AppError::Conflict("idempotency key reused with different body".to_string()));
        }
        // Address must belong to the user.
        let address: Option<(String, String, String)> = tx
            .query_row(
                "SELECT consignee, address, mobile FROM ecs_user_address WHERE address_id = ?1 AND user_id = ?2",
                [body.address_id, user_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db_err)?;
        let Some((consignee, address, mobile)) = address else {
            return Err(AppError::NotFound("address not found".to_string()));
        };
        let shipping_fee: i64 = tx
            .query_row(
                "SELECT shipping_fee FROM ecs_shipping WHERE shipping_id = ?1 AND enabled = 1",
                [body.shipping_id],
                |r| r.get::<_, f64>(0).map(|f| (f * 100.0).round() as i64),
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| AppError::NotFound("shipping not found or disabled".to_string()))?;
        let payment_fee: i64 = tx
            .query_row(
                "SELECT pay_fee FROM ecs_payment WHERE pay_id = ?1 AND enabled = 1",
                [body.payment_id],
                |r| r.get::<_, f64>(0).map(|f| (f * 100.0).round() as i64),
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| AppError::NotFound("payment not found or disabled".to_string()))?;
        // Re-read cart inside the transaction.
        let mut stmt = tx
            .prepare(
                "SELECT c.goods_id, c.goods_number, g.goods_name, g.shop_price
                 FROM ecs_cart c JOIN ecs_goods g ON g.goods_id = c.goods_id
                 WHERE c.user_id = ?1 ORDER BY c.goods_id",
            )
            .map_err(db_err)?;
        let cart_rows: Vec<(i64, i64, String, i64)> = stmt
            .query_map([user_id], |r| {
                let price: f64 = r.get(3)?;
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    (price * 100.0).round() as i64,
                ))
            })
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        drop(stmt);
        if cart_rows.is_empty() {
            return Err(AppError::Validation("cart is empty".to_string()));
        }
        // Conditional stock deduction: UPDATE ... WHERE stock >= quantity.
        for (goods_id, quantity, _name, _price) in &cart_rows {
            let updated = tx
                .execute(
                    "UPDATE ecs_goods SET goods_number = goods_number - ?1 WHERE goods_id = ?2 AND goods_number >= ?1",
                    rusqlite::params![quantity, goods_id],
                )
                .map_err(db_err)?;
            if updated != 1 {
                return Err(AppError::Conflict("insufficient stock or unavailable goods".to_string()));
            }
        }
        let goods_amount: i64 = cart_rows
            .iter()
            .try_fold(0i64, |acc, (_, qty, _, price)| {
                acc.checked_mul(1)
                    .and_then(|_| acc.checked_add(price.checked_mul(*qty)?))
            })
            .ok_or_else(|| AppError::Validation("amount overflow".to_string()))?;
        let order_amount = goods_amount + shipping_fee + payment_fee;
        let order_sn = gen_order_sn();
        tx.execute(
            "INSERT INTO ecs_order_info (order_sn, user_id, order_status, consignee, address, mobile, shipping_id, pay_id, goods_amount, shipping_fee, payment_fee, order_amount, idempotency_key, request_fingerprint, remark, created_at)
             VALUES (?1, ?2, 'pending_payment', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
            rusqlite::params![
                order_sn, user_id, consignee, address, mobile, body.shipping_id, body.payment_id,
                goods_amount as f64 / 100.0,
                shipping_fee as f64 / 100.0,
                payment_fee as f64 / 100.0,
                order_amount as f64 / 100.0,
                idempotency_key, fingerprint, body.remark, unix_now()
            ],
        )
        .map_err(db_err)?;
        let order_id = tx.last_insert_rowid();
        for (goods_id, quantity, name, price) in &cart_rows {
            tx.execute(
                "INSERT INTO ecs_order_goods (order_id, goods_id, goods_name, goods_number, goods_price)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![order_id, goods_id, name, quantity, *price as f64 / 100.0],
            )
            .map_err(db_err)?;
        }
        tx.execute("DELETE FROM ecs_cart WHERE user_id = ?1", [user_id])
            .map_err(db_err)?;
        tx.execute(
            "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?1, ?2, 'create', '', ?3)",
            [order_id, user_id, unix_now()],
        )
        .map_err(db_err)?;
        let summary = fetch_order_summary(&tx, order_id, user_id)?;
        tx.commit().map_err(db_err)?;
        let mut v = summary;
        v["replayed"] = json!(false);
        Ok((StatusCode::CREATED, v))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((status, Json(result)).into_response())
}

/// GET /api/v1/me/orders
pub async fn order_list(
    State(state): State<AppState>,
    auth: AuthUser,
    axum::extract::Query(page): axum::extract::Query<crate::shared::util::PageParams>,
) -> Result<Json<Value>, AppError> {
    let (page_no, page_size, offset) = page.resolve();
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ecs_order_info WHERE user_id = ?1",
                [user_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        let sql = format!(
            "SELECT {ORDER_SUMMARY_COLS} FROM ecs_order_info WHERE user_id = ?1 ORDER BY order_id DESC LIMIT ?2 OFFSET ?3"
        );
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(rusqlite::params![user_id, page_size, offset], order_summary)
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"page": page_no, "page_size": page_size, "total": total, "items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/me/orders/{id}
pub async fn order_detail(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let order_id = parse_id(&id)?;
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut summary = conn
            .query_row(
                &format!("SELECT {ORDER_SUMMARY_COLS} FROM ecs_order_info WHERE order_id = ?1 AND user_id = ?2"),
                [order_id, user_id],
                order_summary,
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| AppError::NotFound("order not found".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT rec_id, goods_id, goods_name, goods_number, goods_price FROM ecs_order_goods WHERE order_id = ?1 ORDER BY rec_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([order_id], |r| {
                let price: f64 = r.get(4)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "goods_id": r.get::<_, i64>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "quantity": r.get::<_, i64>(3)?,
                    "price": cents_to_string((price * 100.0).round() as i64),
                }))
            })
            .map_err(db_err)?;
        let goods: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        summary["goods"] = Value::Array(goods);
        let delivery: Option<Value> = conn
            .query_row(
                "SELECT email, zipcode, telephone, sign_building, best_time FROM ecs_order_delivery_address WHERE order_id = ?1 AND user_id = ?2",
                [order_id, user_id],
                |r| {
                    Ok(json!({
                        "email": r.get::<_, String>(0)?,
                        "zipcode": r.get::<_, String>(1)?,
                        "telephone": r.get::<_, String>(2)?,
                        "sign_building": r.get::<_, String>(3)?,
                        "best_time": r.get::<_, String>(4)?,
                    }))
                },
            )
            .optional()
            .map_err(db_err)?;
        summary["delivery_address"] = delivery.unwrap_or(Value::Null);
        Ok(summary)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// POST /api/v1/me/orders/{id}/cancel — pending_payment only; restores stock and refunds balance once.
pub async fn order_cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let order_id = parse_id(&id)?;
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let updated = tx
            .execute(
                "UPDATE ecs_order_info SET order_status = 'cancelled' WHERE order_id = ?1 AND user_id = ?2 AND order_status = 'pending_payment'",
                [order_id, user_id],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(AppError::Conflict("order cannot be cancelled".to_string()));
        }
        // Restore stock exactly once (guarded by the conditional status update above).
        let mut stmt = tx
            .prepare("SELECT goods_id, goods_number FROM ecs_order_goods WHERE order_id = ?1")
            .map_err(db_err)?;
        let rows: Vec<(i64, i64)> = stmt
            .query_map([order_id], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        drop(stmt);
        for (goods_id, quantity) in &rows {
            tx.execute(
                "UPDATE ecs_goods SET goods_number = goods_number + ?1 WHERE goods_id = ?2",
                rusqlite::params![quantity, goods_id],
            )
            .map_err(db_err)?;
        }
        // Refund balance used for this order, if any.
        let paid: Option<i64> = tx
            .query_row(
                "SELECT paid_cents FROM ecs_order_balance_payment WHERE order_id = ?1 AND user_id = ?2",
                [order_id, user_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        if let Some(paid_cents) = paid {
            if paid_cents > 0 {
                tx.execute(
                    "UPDATE ecs_account_balance SET available_cents = available_cents + ?1 WHERE user_id = ?2",
                    rusqlite::params![paid_cents, user_id],
                )
                .map_err(db_err)?;
                tx.execute(
                    "INSERT INTO ecs_account_log (user_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at)
                     VALUES (?1, ?2, 0, 'order_cancel_refund', 'order_info', ?3, ?4)",
                    rusqlite::params![user_id, paid_cents, order_id, unix_now()],
                )
                .map_err(db_err)?;
            }
        }
        tx.execute(
            "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?1, ?2, 'cancel', '', ?3)",
            [order_id, user_id, unix_now()],
        )
        .map_err(db_err)?;
        let summary = fetch_order_summary(&tx, order_id, user_id)?;
        tx.commit().map_err(db_err)?;
        Ok(summary)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// POST /api/v1/me/orders/{id}/received — paid -> received.
pub async fn order_received(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let order_id = parse_id(&id)?;
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let updated = tx
            .execute(
                "UPDATE ecs_order_info SET order_status = 'received' WHERE order_id = ?1 AND user_id = ?2 AND order_status = 'paid'",
                [order_id, user_id],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(AppError::Conflict("order is not paid".to_string()));
        }
        tx.execute(
            "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?1, ?2, 'confirm_received', '', ?3)",
            [order_id, user_id, unix_now()],
        )
        .map_err(db_err)?;
        let summary = fetch_order_summary(&tx, order_id, user_id)?;
        tx.commit().map_err(db_err)?;
        Ok(summary)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// POST /api/v1/me/orders/{id}/cart — return sellable order goods to the cart.
pub async fn order_return_to_cart(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let order_id = parse_id(&id)?;
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let owned: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM ecs_order_info WHERE order_id = ?1 AND user_id = ?2",
                [order_id, user_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if owned == 0 {
            return Err(AppError::NotFound("order not found".to_string()));
        }
        let mut stmt = tx
            .prepare(
                "SELECT og.goods_id, og.goods_number, g.goods_number
                 FROM ecs_order_goods og JOIN ecs_goods g ON g.goods_id = og.goods_id
                 WHERE og.order_id = ?1 AND g.is_on_sale = 1 AND g.is_delete = 0",
            )
            .map_err(db_err)?;
        let rows: Vec<(i64, i64, i64)> = stmt
            .query_map([order_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .map_err(db_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(db_err)?;
        drop(stmt);
        let mut added = 0i64;
        for (goods_id, order_qty, stock) in rows {
            let addable = order_qty.min(stock);
            if addable <= 0 {
                continue;
            }
            tx.execute(
                "INSERT INTO ecs_cart (user_id, goods_id, goods_number, version)
                 VALUES (?1, ?2, ?3, 1)
                 ON CONFLICT(user_id, goods_id) DO UPDATE SET
                     goods_number = MIN(goods_number + ?3, (SELECT goods_number FROM ecs_goods WHERE goods_id = ?2)),
                     version = version + 1",
                rusqlite::params![user_id, goods_id, addable],
            )
            .map_err(db_err)?;
            added += addable;
        }
        if added == 0 {
            return Err(AppError::Conflict("no sellable goods to return to cart".to_string()));
        }
        tx.commit().map_err(db_err)?;
        Ok(json!({"returned_quantity": added}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let load = |oid: i64| -> Result<MergeOrderRow, AppError> {
            tx.query_row(
                "SELECT order_sn, shipping_id, pay_id, goods_amount, shipping_fee, payment_fee, order_amount, idempotency_key IS NOT NULL, 0, remark, consignee, address, mobile FROM ecs_order_info WHERE order_id = ?1 AND user_id = ?2 AND order_status = 'pending_payment'",
                rusqlite::params![oid, user_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, f64>(3)?,
                        r.get::<_, f64>(4)?,
                        r.get::<_, f64>(5)?,
                        r.get::<_, f64>(6)?,
                        r.get::<_, i64>(7)?,
                        r.get::<_, i64>(8)?,
                        r.get::<_, String>(9)?,
                        r.get::<_, String>(10)?,
                        r.get::<_, String>(11)?,
                        r.get::<_, String>(12)?,
                    ))
                },
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| AppError::NotFound("pending order not found".to_string()))
        };
        let from = load(body.from_order_id)?;
        let to = load(body.to_order_id)?;
        // Orders that already used account balance must not be merged (funds attribution).
        let balance_used: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM ecs_order_balance_payment WHERE order_id IN (?1, ?2)",
                rusqlite::params![body.from_order_id, body.to_order_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if balance_used > 0 {
            return Err(AppError::Conflict("orders using account balance cannot be merged".to_string()));
        }
        let goods_amount = from.3 + to.3;
        let shipping_fee = from.4 + to.4;
        let payment_fee = from.5 + to.5;
        let order_amount = goods_amount + shipping_fee + payment_fee;
        let order_sn = gen_order_sn();
        tx.execute(
            "INSERT INTO ecs_order_info (order_sn, user_id, order_status, consignee, address, mobile, shipping_id, pay_id, goods_amount, shipping_fee, payment_fee, order_amount, idempotency_key, request_fingerprint, remark, created_at)
             VALUES (?1, ?2, 'pending_payment', ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, '', '', ?12, ?13)",
            rusqlite::params![
                order_sn, user_id, from.9, from.10, from.11, from.1, from.2,
                goods_amount, shipping_fee, payment_fee, order_amount, from.8, unix_now()
            ],
        )
        .map_err(db_err)?;
        let new_order_id = tx.last_insert_rowid();
        tx.execute(
            "UPDATE ecs_order_goods SET order_id = ?1 WHERE order_id = ?2",
            [new_order_id, body.from_order_id],
        )
        .map_err(db_err)?;
        // to-order goods rows: re-point them too.
        tx.execute(
            "INSERT INTO ecs_order_goods (order_id, goods_id, goods_name, goods_number, goods_price)
             SELECT ?1, goods_id, goods_name, goods_number, goods_price FROM ecs_order_goods WHERE order_id = ?2",
            [new_order_id, body.to_order_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "UPDATE OR IGNORE ecs_order_delivery_address SET order_id = ?1 WHERE order_id = ?2",
            [new_order_id, body.from_order_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT OR IGNORE INTO ecs_order_delivery_address (order_id, user_id, email, zipcode, telephone, sign_building, best_time)
             SELECT ?1, user_id, email, zipcode, telephone, sign_building, best_time FROM ecs_order_delivery_address WHERE order_id = ?2",
            [new_order_id, body.to_order_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "UPDATE OR IGNORE ecs_order_promotion SET order_id = ?1 WHERE order_id = ?2",
            [new_order_id, body.from_order_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT OR IGNORE INTO ecs_order_promotion (order_id, promotion_id, promotion_type)
             SELECT ?1, promotion_id, promotion_type FROM ecs_order_promotion WHERE order_id = ?2",
            [new_order_id, body.to_order_id],
        )
        .map_err(db_err)?;
        tx.execute("DELETE FROM ecs_order_info WHERE order_id = ?1", [body.from_order_id])
            .map_err(db_err)?;
        tx.execute("DELETE FROM ecs_order_info WHERE order_id = ?1", [body.to_order_id])
            .map_err(db_err)?;
        tx.execute(
            "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?1, ?2, 'merge', ?3, ?4)",
            rusqlite::params![new_order_id, user_id, format!("merged from {}", from.0), unix_now()],
        )
        .map_err(db_err)?;
        let summary = fetch_order_summary(&tx, new_order_id, user_id)?;
        tx.commit().map_err(db_err)?;
        Ok(summary)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

/// GET /api/v1/me/orders/by-number/{order_sn}/status
pub async fn order_status_by_sn(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(order_sn): Path<String>,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let row = conn
            .query_row(
                "SELECT order_sn, order_status, order_amount FROM ecs_order_info WHERE order_sn = ?1 AND user_id = ?2",
                rusqlite::params![order_sn, user_id],
                |r| {
                    let amount: f64 = r.get(2)?;
                    Ok(json!({
                        "order_sn": r.get::<_, String>(0)?,
                        "status": r.get::<_, String>(1)?,
                        "order_amount": cents_to_string((amount * 100.0).round() as i64),
                    }))
                },
            )
            .optional()
            .map_err(db_err)?;
        row.ok_or_else(|| AppError::NotFound("order not found".to_string()))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let order: Option<(f64, f64, f64)> = tx
            .query_row(
                "SELECT goods_amount, shipping_fee, order_amount FROM ecs_order_info
                 WHERE order_id = ?1 AND user_id = ?2 AND order_status = 'pending_payment'",
                [order_id, user_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db_err)?;
        let Some((goods_amount, shipping_fee, _order_amount)) = order else {
            return Err(AppError::NotFound("pending order not found".to_string()));
        };
        let total_due = ((goods_amount + shipping_fee) * 100.0).round() as i64;
        let already_paid: i64 = tx
            .query_row(
                "SELECT COALESCE(paid_cents, 0) FROM ecs_order_balance_payment WHERE order_id = ?1 AND user_id = ?2",
                [order_id, user_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?
            .unwrap_or(0);
        let remaining = total_due - already_paid;
        if remaining <= 0 {
            return Err(AppError::Conflict("order already fully paid by balance".to_string()));
        }
        let apply = cents.min(remaining);
        let deducted = tx
            .execute(
                "UPDATE ecs_account_balance SET available_cents = available_cents - ?1
                 WHERE user_id = ?2 AND available_cents >= ?1",
                rusqlite::params![apply, user_id],
            )
            .map_err(db_err)?;
        if deducted == 0 {
            return Err(AppError::Conflict("insufficient balance".to_string()));
        }
        tx.execute(
            "INSERT INTO ecs_order_balance_payment (order_id, user_id, paid_cents, updated_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(order_id) DO UPDATE SET paid_cents = paid_cents + ?3, updated_at = ?4",
            rusqlite::params![order_id, user_id, apply, now],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT INTO ecs_account_log (user_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at)
             VALUES (?1, ?2, 0, 'order_surplus_payment', 'order_info', ?3, ?4)",
            rusqlite::params![user_id, -apply, order_id, now],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?1, ?2, 'surplus_payment', ?3, ?4)",
            rusqlite::params![order_id, user_id, cents_to_string(apply), now],
        )
        .map_err(db_err)?;
        let new_paid = already_paid + apply;
        if new_paid >= total_due {
            tx.execute(
                "UPDATE ecs_order_info SET order_status = 'paid' WHERE order_id = ?1 AND order_status = 'pending_payment'",
                [order_id],
            )
            .map_err(db_err)?;
        }
        let summary = fetch_order_summary(&tx, order_id, user_id)?;
        tx.commit().map_err(db_err)?;
        let mut v = summary;
        v["balance_applied"] = json!(cents_to_string(apply));
        Ok(v)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let row: Option<(i64, f64, f64)> = tx
            .query_row(
                "SELECT pay_id, payment_fee, order_amount FROM ecs_order_info
                 WHERE order_id = ?1 AND user_id = ?2 AND order_status = 'pending_payment'",
                [order_id, user_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()
            .map_err(db_err)?;
        let Some((current_pay_id, current_fee, order_amount)) = row else {
            return Err(AppError::NotFound("pending order not found".to_string()));
        };
        if current_pay_id == body.payment_id {
            return Err(AppError::Conflict("payment method unchanged".to_string()));
        }
        let new_fee: i64 = tx
            .query_row(
                "SELECT pay_fee FROM ecs_payment WHERE pay_id = ?1 AND enabled = 1",
                [body.payment_id],
                |r| r.get::<_, f64>(0).map(|f| (f * 100.0).round() as i64),
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| AppError::NotFound("payment not found or disabled".to_string()))?;
        let remaining = ((order_amount * 100.0).round() as i64) - ((current_fee * 100.0).round() as i64);
        let new_amount = remaining + new_fee;
        tx.execute(
            "UPDATE ecs_order_info SET pay_id = ?1, payment_fee = ?2, order_amount = ?3 WHERE order_id = ?4",
            rusqlite::params![body.payment_id, new_fee as f64 / 100.0, new_amount as f64 / 100.0, order_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?1, ?2, 'change_payment', ?3, ?4)",
            rusqlite::params![order_id, user_id, format!("payment {}", body.payment_id), now],
        )
        .map_err(db_err)?;
        let summary = fetch_order_summary(&tx, order_id, user_id)?;
        tx.commit().map_err(db_err)?;
        Ok(summary)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let now = unix_now();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let updated = tx
            .execute(
                "UPDATE ecs_order_info SET consignee = ?1, address = ?2, mobile = ?3
                 WHERE order_id = ?4 AND user_id = ?5 AND order_status = 'pending_payment'",
                rusqlite::params![body.consignee, body.address, body.mobile, order_id, user_id],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(AppError::NotFound("pending order not found".to_string()));
        }
        tx.execute(
            "INSERT INTO ecs_order_delivery_address (order_id, user_id, email, zipcode, telephone, sign_building, best_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(order_id) DO UPDATE SET email = ?3, zipcode = ?4, telephone = ?5, sign_building = ?6, best_time = ?7",
            rusqlite::params![order_id, user_id, body.email, body.zipcode, body.tel, body.sign_building, body.best_time],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT INTO ecs_order_action (order_id, actor_user_id, action, note, created_at) VALUES (?1, ?2, 'save_address', '', ?3)",
            [order_id, user_id, now],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/me/group-buys and /api/v1/me/group-buys/{id}
pub async fn me_group_buys(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    me_group_buys_impl(state, auth, None).await
}

pub async fn me_group_buy_detail(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let promotion_id = parse_id(&id)?;
    me_group_buys_impl(state, auth, Some(promotion_id)).await
}

async fn me_group_buys_impl(
    state: AppState,
    auth: AuthUser,
    promotion_id: Option<i64>,
) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let user_id = auth.user_id;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT p.promotion_id, p.order_id, a.act_name, a.goods_name, o.order_amount, o.order_status
                 FROM ecs_order_promotion p
                 JOIN ecs_order_info o ON o.order_id = p.order_id AND o.user_id = ?1
                 JOIN ecs_goods_activity a ON a.act_id = p.promotion_id AND a.act_type = 1
                 WHERE p.promotion_type = 'group_buy'
                 ORDER BY p.order_id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                let amount: f64 = r.get(4)?;
                Ok(json!({
                    "promotion_id": r.get::<_, i64>(0)?,
                    "order_id": r.get::<_, i64>(1)?,
                    "activity_name": r.get::<_, String>(2)?,
                    "goods_name": r.get::<_, String>(3)?,
                    "order_amount": cents_to_string((amount * 100.0).round() as i64),
                    "order_status": r.get::<_, String>(5)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        match promotion_id {
            Some(pid) => {
                let found = items
                    .into_iter()
                    .find(|i| i.get("promotion_id").and_then(|v| v.as_i64()) == Some(pid))
                    .ok_or_else(|| AppError::NotFound("group buy not found".to_string()))?;
                Ok(found)
            }
            None => Ok(json!({"items": items})),
        }
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}
