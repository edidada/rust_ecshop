//! Cart and checkout context.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::util::{cents_to_string, parse_money_cents};

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation("id must be a positive integer".to_string())),
    }
}

#[derive(Deserialize)]
pub struct CartAddRequest {
    goods_id: i64,
    quantity: i64,
}

/// POST /api/v1/me/cart — same goods accumulates quantity atomically and bumps version.
pub async fn cart_add(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CartAddRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if body.quantity < 1 || body.quantity > 999 {
        return Err(AppError::Validation("quantity must be between 1 and 999".to_string()));
    }
    let db = state.db.clone();
    let user_id = auth.user_id;
    let goods_id = body.goods_id;
    let quantity = body.quantity;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let (name, price_cents, stock): (String, i64, i64) = tx
            .query_row(
                "SELECT goods_name, shop_price, goods_number FROM ecs_goods
                 WHERE goods_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                [goods_id],
                |r| {
                    let price: f64 = r.get(1)?;
                    Ok((
                        r.get::<_, String>(0)?,
                        (price * 100.0).round() as i64,
                        r.get(2)?,
                    ))
                },
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| AppError::NotFound("goods not found".to_string()))?;
        let existing: Option<i64> = tx
            .query_row(
                "SELECT goods_number FROM ecs_cart WHERE user_id = ?1 AND goods_id = ?2",
                [user_id, goods_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let new_qty = existing.unwrap_or(0) + quantity;
        if new_qty > stock {
            return Err(AppError::Conflict("insufficient stock".to_string()));
        }
        match existing {
            Some(_) => {
                tx.execute(
                    "UPDATE ecs_cart SET goods_number = goods_number + ?1, version = version + 1
                     WHERE user_id = ?2 AND goods_id = ?3",
                    rusqlite::params![quantity, user_id, goods_id],
                )
                .map_err(db_err)?;
            }
            None => {
                tx.execute(
                    "INSERT INTO ecs_cart (user_id, goods_id, goods_number, version) VALUES (?1, ?2, ?3, 1)",
                    rusqlite::params![user_id, goods_id, quantity],
                )
                .map_err(db_err)?;
            }
        }
        let rec_id: i64 = tx
            .query_row(
                "SELECT rec_id FROM ecs_cart WHERE user_id = ?1 AND goods_id = ?2",
                [user_id, goods_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        let (version, quantity): (i64, i64) = tx
            .query_row(
                "SELECT version, goods_number FROM ecs_cart WHERE rec_id = ?1",
                [rec_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(json!({
            "id": rec_id,
            "goods_id": goods_id,
            "name": name,
            "price": cents_to_string(price_cents),
            "quantity": quantity,
            "version": version,
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

/// GET /api/v1/me/cart
pub async fn cart_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT c.rec_id, c.goods_id, g.goods_name, g.shop_price, c.goods_number, c.version
                 FROM ecs_cart c JOIN ecs_goods g ON g.goods_id = c.goods_id
                 WHERE c.user_id = ?1 ORDER BY c.rec_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                let price: f64 = r.get(3)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "goods_id": r.get::<_, i64>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "price": cents_to_string((price * 100.0).round() as i64),
                    "quantity": r.get::<_, i64>(4)?,
                    "version": r.get::<_, i64>(5)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        let total_quantity: i64 = items
            .iter()
            .filter_map(|i| i.get("quantity").and_then(|q| q.as_i64()))
            .sum();
        Ok(json!({"items": items, "total_quantity": total_quantity}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct CartPatchRequest {
    quantity: i64,
    version: i64,
}

/// PATCH /api/v1/me/cart/{id} — optimistic lock on version; stale updates return 409.
pub async fn cart_update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<CartPatchRequest>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    if body.quantity < 1 || body.quantity > 999 {
        return Err(AppError::Validation("quantity must be between 1 and 999".to_string()));
    }
    let db = state.db.clone();
    let user_id = auth.user_id;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        // Optimistic lock: WHERE matches id, user and version.
        let updated = tx
            .execute(
                "UPDATE ecs_cart SET goods_number = ?1, version = version + 1
                 WHERE rec_id = ?2 AND user_id = ?3 AND version = ?4",
                rusqlite::params![body.quantity, rec_id, user_id, body.version],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(AppError::Conflict("cart version is stale".to_string()));
        }
        // Stock guard after quantity change.
        let over: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM ecs_cart c JOIN ecs_goods g ON g.goods_id = c.goods_id
                 WHERE c.rec_id = ?1 AND c.goods_number > g.goods_number",
                [rec_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if over > 0 {
            return Err(AppError::Conflict("insufficient stock".to_string()));
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/me/cart/{id}
pub async fn cart_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    let db = state.db.clone();
    let user_id = auth.user_id;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        let deleted = conn
            .execute(
                "DELETE FROM ecs_cart WHERE rec_id = ?1 AND user_id = ?2",
                [rec_id, user_id],
            )
            .map_err(db_err)?;
        if deleted == 0 {
            return Err(AppError::NotFound("cart item not found".to_string()));
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/checkout/options
pub async fn checkout_options(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let shipping: Vec<Value> = {
            let mut stmt = conn
                .prepare("SELECT shipping_id, shipping_name, shipping_fee FROM ecs_shipping WHERE enabled = 1 ORDER BY shipping_id")
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| {
                    let fee: f64 = r.get(2)?;
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "fee": cents_to_string((fee * 100.0).round() as i64),
                    }))
                })
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        let payments: Vec<Value> = {
            let mut stmt = conn
                .prepare("SELECT pay_id, pay_name, pay_fee FROM ecs_payment WHERE enabled = 1 ORDER BY pay_id")
                .map_err(db_err)?;
            let rows = stmt
                .query_map([], |r| {
                    let fee: f64 = r.get(2)?;
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "fee": cents_to_string((fee * 100.0).round() as i64),
                    }))
                })
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        Ok(json!({"shipping": shipping, "payment": payments}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct CheckoutQuoteRequest {
    address_id: i64,
    shipping_id: i64,
    payment_id: i64,
}

/// POST /api/v1/checkout/quote — server-side price only; client totals are never trusted.
pub async fn checkout_quote(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CheckoutQuoteRequest>,
) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let user_id = auth.user_id;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let address: Option<(String, String)> = conn
            .query_row(
                "SELECT consignee, address FROM ecs_user_address WHERE address_id = ?1 AND user_id = ?2",
                [body.address_id, user_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let Some((consignee, address)) = address else {
            return Err(AppError::NotFound("address not found".to_string()));
        };
        let shipping_fee: i64 = conn
            .query_row(
                "SELECT shipping_fee FROM ecs_shipping WHERE shipping_id = ?1 AND enabled = 1",
                [body.shipping_id],
                |r| r.get::<_, f64>(0).map(|f| (f * 100.0).round() as i64),
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| AppError::NotFound("shipping not found or disabled".to_string()))?;
        let payment_fee: i64 = conn
            .query_row(
                "SELECT pay_fee FROM ecs_payment WHERE pay_id = ?1 AND enabled = 1",
                [body.payment_id],
                |r| r.get::<_, f64>(0).map(|f| (f * 100.0).round() as i64),
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| AppError::NotFound("payment not found or disabled".to_string()))?;
        let mut stmt = conn
            .prepare(
                "SELECT c.rec_id, c.goods_id, g.goods_name, g.shop_price, c.goods_number
                 FROM ecs_cart c JOIN ecs_goods g ON g.goods_id = c.goods_id
                 WHERE c.user_id = ?1 AND g.is_on_sale = 1 AND g.is_delete = 0 ORDER BY c.rec_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                let price: f64 = r.get(3)?;
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    (price * 100.0).round() as i64,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(db_err)?;
        let rows: Vec<(i64, i64, String, i64, i64)> =
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        if rows.is_empty() {
            return Err(AppError::Validation("cart is empty".to_string()));
        }
        let mut goods_amount = 0i64;
        let mut items = Vec::new();
        for (rec_id, goods_id, name, price, quantity) in rows {
            goods_amount = goods_amount
                .checked_add(price.checked_mul(quantity).ok_or_else(|| {
                    AppError::Validation("amount overflow".to_string())
                })?)
                .ok_or_else(|| AppError::Validation("amount overflow".to_string()))?;
            items.push(json!({
                "cart_id": rec_id,
                "goods_id": goods_id,
                "name": name,
                "price": cents_to_string(price),
                "quantity": quantity,
            }));
        }
        let order_amount = goods_amount + shipping_fee + payment_fee;
        Ok(json!({
            "address_id": body.address_id,
            "consignee": consignee,
            "address": address,
            "shipping_id": body.shipping_id,
            "payment_id": body.payment_id,
            "items": items,
            "goods_amount": cents_to_string(goods_amount),
            "shipping_fee": cents_to_string(shipping_fee),
            "payment_fee": cents_to_string(payment_fee),
            "order_amount": cents_to_string(order_amount),
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// Keep parse_money_cents in scope for future checkout extensions.
#[allow(dead_code)]
fn _parse_money_kept() -> Result<i64, AppError> {
    parse_money_cents("0.00", "amount")
}
