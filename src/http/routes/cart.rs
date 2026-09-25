//! Cart and checkout context.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::rbutil::{col_i64, col_str, col_cents, e, q, q1};
use crate::shared::util::cents_to_string;

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
    if !(1..=999).contains(&body.quantity) {
        return Err(AppError::Validation("quantity must be between 1 and 999".to_string()));
    }
    let rb = state.rb;
    let tx = crate::infrastructure::db::begin(rb).await?;
    let goods = q1(
        &tx,
        "SELECT goods_name AS goods_name, shop_price AS shop_price, goods_number AS goods_number
         FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
        vec![json!(body.goods_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("goods not found".to_string()))?;
    let price_cents = col_cents(&goods, "shop_price");
    let stock = col_i64(&goods, "goods_number");
    let existing = q1(
        &tx,
        "SELECT goods_number AS goods_number FROM ecs_cart WHERE user_id = ? AND goods_id = ?",
        vec![json!(auth.user_id), json!(body.goods_id)],
    )
    .await?
    .map(|r| col_i64(&r, "goods_number"));
    let new_qty = existing.unwrap_or(0) + body.quantity;
    if new_qty > stock {
        return Err(AppError::Conflict("insufficient stock".to_string()));
    }
    match existing {
        Some(_) => {
            e(
                &tx,
                "UPDATE ecs_cart SET goods_number = goods_number + ?, version = version + 1
                 WHERE user_id = ? AND goods_id = ?",
                vec![json!(body.quantity), json!(auth.user_id), json!(body.goods_id)],
            )
            .await?;
        }
        None => {
            e(
                &tx,
                "INSERT INTO ecs_cart (user_id, goods_id, goods_number, version) VALUES (?, ?, ?, 1)",
                vec![json!(auth.user_id), json!(body.goods_id), json!(body.quantity)],
            )
            .await?;
        }
    }
    let cart_row = q1(
        &tx,
        "SELECT rec_id AS rec_id, version AS version, goods_number AS goods_number
         FROM ecs_cart WHERE user_id = ? AND goods_id = ?",
        vec![json!(auth.user_id), json!(body.goods_id)],
    )
    .await?
    .unwrap_or_default();
    let rec_id = col_i64(&cart_row, "rec_id");
    let version = col_i64(&cart_row, "version");
    let quantity = col_i64(&cart_row, "goods_number");
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": if rec_id > 0 { rec_id } else { insert_id_dummy() },
            "goods_id": body.goods_id,
            "name": col_str(&goods, "goods_name"),
            "price": cents_to_string(price_cents),
            "quantity": quantity,
            "version": version,
        })),
    ))
}

fn insert_id_dummy() -> i64 {
    0
}

/// GET /api/v1/me/cart
pub async fn cart_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let items = q(
        state.rb,
        "SELECT c.rec_id AS rec_id, c.goods_id AS goods_id, g.goods_name AS goods_name, g.shop_price AS shop_price,
                c.goods_number AS goods_number, c.version AS version
         FROM ecs_cart c JOIN ecs_goods g ON g.goods_id = c.goods_id
         WHERE c.user_id = ? ORDER BY c.rec_id",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "rec_id"),
            "goods_id": col_i64(&r, "goods_id"),
            "name": col_str(&r, "goods_name"),
            "price": cents_to_string(col_cents(&r, "shop_price")),
            "quantity": col_i64(&r, "goods_number"),
            "version": col_i64(&r, "version"),
        })
    })
    .collect::<Vec<_>>();
    let total_quantity: i64 = items
        .iter()
        .filter_map(|i| i.get("quantity").and_then(|v| v.as_i64()))
        .sum();
    Ok(Json(json!({"items": items, "total_quantity": total_quantity})))
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
    if !(1..=999).contains(&body.quantity) {
        return Err(AppError::Validation("quantity must be between 1 and 999".to_string()));
    }
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    // Optimistic lock: WHERE matches id, user and version.
    let updated = e(
        &tx,
        "UPDATE ecs_cart SET goods_number = ?, version = version + 1
         WHERE rec_id = ? AND user_id = ? AND version = ?",
        vec![json!(body.quantity), json!(rec_id), json!(auth.user_id), json!(body.version)],
    )
    .await?;
    if updated.rows_affected == 0 {
        return Err(AppError::Conflict("cart version is stale".to_string()));
    }
    // Stock guard after quantity change.
    let over = q1(
        &tx,
        "SELECT COUNT(*) AS cnt FROM ecs_cart c JOIN ecs_goods g ON g.goods_id = c.goods_id
         WHERE c.rec_id = ? AND c.goods_number > g.goods_number",
        vec![json!(rec_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if over > 0 {
        return Err(AppError::Conflict("insufficient stock".to_string()));
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/me/cart/{id}
pub async fn cart_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    let deleted = e(
        state.rb,
        "DELETE FROM ecs_cart WHERE rec_id = ? AND user_id = ?",
        vec![json!(rec_id), json!(auth.user_id)],
    )
    .await?;
    if deleted.rows_affected == 0 {
        return Err(AppError::NotFound("cart item not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/checkout/options
pub async fn checkout_options(
    State(state): State<AppState>,
    _auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let rb = state.rb;
    let shipping = q(
        rb,
        "SELECT shipping_id AS shipping_id, shipping_name AS shipping_name, shipping_fee AS shipping_fee
         FROM ecs_shipping WHERE enabled = 1 ORDER BY shipping_id",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "shipping_id"),
            "name": col_str(&r, "shipping_name"),
            "fee": cents_to_string(col_cents(&r, "shipping_fee")),
        })
    })
    .collect::<Vec<_>>();
    let payments = q(
        rb,
        "SELECT pay_id AS pay_id, pay_name AS pay_name, pay_fee AS pay_fee
         FROM ecs_payment WHERE enabled = 1 ORDER BY pay_id",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "pay_id"),
            "name": col_str(&r, "pay_name"),
            "fee": cents_to_string(col_cents(&r, "pay_fee")),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"shipping": shipping, "payment": payments})))
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
    let rb = state.rb;
    let address = q1(
        rb,
        "SELECT consignee AS consignee, address AS address FROM ecs_user_address WHERE address_id = ? AND user_id = ?",
        vec![json!(body.address_id), json!(auth.user_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("address not found".to_string()))?;
    let shipping_fee = q1(
        rb,
        "SELECT shipping_fee AS shipping_fee FROM ecs_shipping WHERE shipping_id = ? AND enabled = 1",
        vec![json!(body.shipping_id)],
    )
    .await?
    .map(|r| col_cents(&r, "shipping_fee"))
    .ok_or_else(|| AppError::NotFound("shipping not found or disabled".to_string()))?;
    let payment_fee = q1(
        rb,
        "SELECT pay_fee AS pay_fee FROM ecs_payment WHERE pay_id = ? AND enabled = 1",
        vec![json!(body.payment_id)],
    )
    .await?
    .map(|r| col_cents(&r, "pay_fee"))
    .ok_or_else(|| AppError::NotFound("payment not found or disabled".to_string()))?;
    let rows = q(
        rb,
        "SELECT c.rec_id AS rec_id, c.goods_id AS goods_id, g.goods_name AS goods_name, g.shop_price AS shop_price, c.goods_number AS goods_number
         FROM ecs_cart c JOIN ecs_goods g ON g.goods_id = c.goods_id
         WHERE c.user_id = ? AND g.is_on_sale = 1 AND g.is_delete = 0 ORDER BY c.rec_id",
        vec![json!(auth.user_id)],
    )
    .await?;
    if rows.is_empty() {
        return Err(AppError::Validation("cart is empty".to_string()));
    }
    let mut goods_amount = 0i64;
    let mut items = Vec::new();
    for row in &rows {
        let price = col_cents(row, "shop_price");
        let quantity = col_i64(row, "goods_number");
        goods_amount = goods_amount
            .checked_add(price.checked_mul(quantity).ok_or_else(|| {
                AppError::Validation("amount overflow".to_string())
            })?)
            .ok_or_else(|| AppError::Validation("amount overflow".to_string()))?;
        items.push(json!({
            "cart_id": col_i64(row, "rec_id"),
            "goods_id": col_i64(row, "goods_id"),
            "name": col_str(row, "goods_name"),
            "price": cents_to_string(price),
            "quantity": quantity,
        }));
    }
    let order_amount = goods_amount + shipping_fee + payment_fee;
    Ok(Json(json!({
        "address_id": body.address_id,
        "consignee": col_str(&address, "consignee"),
        "address": col_str(&address, "address"),
        "shipping_id": body.shipping_id,
        "payment_id": body.payment_id,
        "items": items,
        "goods_amount": cents_to_string(goods_amount),
        "shipping_fee": cents_to_string(shipping_fee),
        "payment_fee": cents_to_string(payment_fee),
        "order_amount": cents_to_string(order_amount),
    })))
}
