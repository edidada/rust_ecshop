//! Cart and checkout context.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::state::AppState;
use rust_ecshop_domain::error::AppError;
use rust_ecshop_domain::util::cents_to_string;
use rust_ecshop_infra::rbutil::{col_cents, col_i64, col_str, e, q, q1};

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation(
            "id must be a positive integer".to_string(),
        )),
    }
}

#[derive(Deserialize)]
pub struct CartAddRequest {
    goods_id: i64,
    quantity: i64,
    #[serde(default)]
    product_id: i64,
    #[serde(default)]
    attribute_ids: Vec<i64>,
}

/// POST /api/v1/me/cart — same goods accumulates quantity atomically and bumps version.
pub async fn cart_add(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<CartAddRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if !(1..=999).contains(&body.quantity) {
        return Err(AppError::Validation(
            "quantity must be between 1 and 999".to_string(),
        ));
    }
    let rb = state.rb;
    let tx = rust_ecshop_infra::db::begin(rb).await?;
    rust_ecshop_infra::db::lock_user(&tx, auth.user_id).await?;
    let goods = q1(
        &tx,
        "SELECT goods_name AS goods_name, shop_price AS shop_price, goods_number AS goods_number
         FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
        vec![json!(body.goods_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("goods not found".to_string()))?;
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
    let price_cents = super::pricing::unit(
        &tx,
        auth.user_id,
        body.goods_id,
        new_qty,
        body.product_id,
        &body.attribute_ids,
    )
    .await?;
    let old_spec = q1(
        &tx,
        "SELECT product_id,attributes FROM ecs_cart_spec WHERE user_id=? AND goods_id=?",
        vec![json!(auth.user_id), json!(body.goods_id)],
    )
    .await?;
    if existing.is_some() {
        let old_product = old_spec
            .as_ref()
            .map(|r| col_i64(r, "product_id"))
            .unwrap_or(0);
        let old_attrs: Vec<i64> = old_spec
            .as_ref()
            .and_then(|r| serde_json::from_str(&col_str(r, "attributes")).ok())
            .unwrap_or_default();
        if old_product != body.product_id || old_attrs != body.attribute_ids {
            return Err(AppError::Conflict(
                "remove existing variant before adding another SKU".into(),
            ));
        }
    }
    e(&tx,"INSERT INTO ecs_cart_spec(user_id,goods_id,product_id,attributes) VALUES(?,?,?,?) ON CONFLICT(user_id,goods_id) DO UPDATE SET product_id=excluded.product_id,attributes=excluded.attributes",vec![json!(auth.user_id),json!(body.goods_id),json!(body.product_id),json!(serde_json::to_string(&body.attribute_ids).unwrap_or_default())]).await?;
    match existing {
        Some(_) => {
            e(
                &tx,
                "UPDATE ecs_cart SET goods_number = goods_number + ?, version = version + 1
                 WHERE user_id = ? AND goods_id = ?",
                vec![
                    json!(body.quantity),
                    json!(auth.user_id),
                    json!(body.goods_id),
                ],
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
    rust_ecshop_infra::db::commit(&tx).await?;
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
    let mut rows = q(
        state.rb,
        "SELECT c.rec_id AS rec_id, c.goods_id AS goods_id, g.goods_name AS goods_name, g.shop_price AS shop_price,
                c.goods_number AS goods_number, c.version AS version
         FROM ecs_cart c JOIN ecs_goods g ON g.goods_id = c.goods_id
         WHERE c.user_id = ? ORDER BY c.rec_id",
        vec![json!(auth.user_id)],
    )
    .await?;
    super::pricing::cart(state.rb, auth.user_id, &mut rows).await?;
    let items = rows
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
    Ok(Json(
        json!({"items": items, "total_quantity": total_quantity}),
    ))
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
        return Err(AppError::Validation(
            "quantity must be between 1 and 999".to_string(),
        ));
    }
    let tx = rust_ecshop_infra::db::begin(state.rb).await?;
    rust_ecshop_infra::db::lock_user(&tx, auth.user_id).await?;
    // Optimistic lock: WHERE matches id, user and version.
    let updated = e(
        &tx,
        "UPDATE ecs_cart SET goods_number = ?, version = version + 1
         WHERE rec_id = ? AND user_id = ? AND version = ?",
        vec![
            json!(body.quantity),
            json!(rec_id),
            json!(auth.user_id),
            json!(body.version),
        ],
    )
    .await?;
    if updated.rows_affected == 0 {
        return Err(AppError::Conflict("cart version is stale".to_string()));
    }
    let mut selected = q(
        &tx,
        "SELECT goods_id,goods_number FROM ecs_cart WHERE rec_id=? AND user_id=?",
        vec![json!(rec_id), json!(auth.user_id)],
    )
    .await?;
    super::pricing::cart(&tx, auth.user_id, &mut selected).await?;
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
    rust_ecshop_infra::db::commit(&tx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/me/cart/{id}
pub async fn cart_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    let tx = rust_ecshop_infra::db::begin(state.rb).await?;
    rust_ecshop_infra::db::lock_user(&tx, auth.user_id).await?;
    e(&tx,"DELETE FROM ecs_cart_spec WHERE user_id=? AND goods_id IN(SELECT goods_id FROM ecs_cart WHERE rec_id=? AND user_id=?)",vec![json!(auth.user_id),json!(rec_id),json!(auth.user_id)]).await?;
    let deleted = e(
        &tx,
        "DELETE FROM ecs_cart WHERE rec_id=? AND user_id=?",
        vec![json!(rec_id), json!(auth.user_id)],
    )
    .await?;
    if deleted.rows_affected == 0 {
        return Err(AppError::NotFound("cart item not found".into()));
    }
    rust_ecshop_infra::db::commit(&tx).await?;
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
    #[serde(default)]
    shipping_id: i64,
    #[serde(default)]
    payment_id: i64,
}

/// POST /api/v1/checkout/quote — server-side price only; client totals are never trusted.
pub async fn checkout_quote(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(mut body): Json<CheckoutQuoteRequest>,
) -> Result<Json<Value>, AppError> {
    if body.shipping_id == 0 || body.payment_id == 0 {
        if let Some(saved) = q1(
            state.rb,
            "SELECT shipping_id,payment_id FROM ecs_checkout_selection WHERE user_id=?",
            vec![json!(auth.user_id)],
        )
        .await?
        {
            if body.shipping_id == 0 {
                body.shipping_id = col_i64(&saved, "shipping_id");
            }
            if body.payment_id == 0 {
                body.payment_id = col_i64(&saved, "payment_id");
            }
        }
    }

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
    let mut rows = q(
        rb,
        "SELECT c.rec_id AS rec_id, c.goods_id AS goods_id, g.goods_name AS goods_name, g.shop_price AS shop_price, g.cat_id AS cat_id, g.brand_id AS brand_id, c.goods_number AS goods_number
         FROM ecs_cart c JOIN ecs_goods g ON g.goods_id = c.goods_id
         WHERE c.user_id = ? ORDER BY c.rec_id",
        vec![json!(auth.user_id)],
    )
    .await?;
    if rows.is_empty() {
        return Err(AppError::Validation("cart is empty".to_string()));
    }
    super::pricing::cart(rb, auth.user_id, &mut rows).await?;
    let mut goods_amount = 0i64;
    let mut items = Vec::new();
    for row in &rows {
        let price = col_cents(row, "shop_price");
        let quantity = col_i64(row, "goods_number");
        goods_amount = goods_amount
            .checked_add(
                price
                    .checked_mul(quantity)
                    .ok_or_else(|| AppError::Validation("amount overflow".to_string()))?,
            )
            .ok_or_else(|| AppError::Validation("amount overflow".to_string()))?;
        items.push(json!({
            "cart_id": col_i64(row, "rec_id"),
            "goods_id": col_i64(row, "goods_id"),
            "name": col_str(row, "goods_name"),
            "price": cents_to_string(price),
            "quantity": quantity,
        }));
    }
    let adjustment = super::flow::adjustments(rb, auth.user_id, goods_amount, &rows).await?;
    let order_amount = goods_amount
        .checked_add(shipping_fee)
        .and_then(|v| v.checked_add(payment_fee))
        .and_then(|v| v.checked_add(adjustment.extra))
        .and_then(|v| v.checked_sub(adjustment.discount))
        .ok_or_else(|| AppError::Validation("amount overflow".into()))?;
    let gifts: Vec<Value> = q(
        rb,
        "SELECT gf.goods_id AS goods_id, gf.quantity AS quantity, g.goods_name AS goods_name
         FROM ecs_cart_gift gf JOIN ecs_goods g ON g.goods_id = gf.goods_id
         WHERE gf.user_id = ? ORDER BY gf.goods_id",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|g| {
        json!({
            "goods_id": col_i64(&g, "goods_id"),
            "name": col_str(&g, "goods_name"),
            "quantity": col_i64(&g, "quantity"),
            "price": cents_to_string(0),
        })
    })
    .collect();
    Ok(Json(json!({
        "address_id": body.address_id,
        "consignee": col_str(&address, "consignee"),
        "address": col_str(&address, "address"),
        "shipping_id": body.shipping_id,
        "payment_id": body.payment_id,
        "items": items,
        "gifts": gifts,
        "goods_amount": cents_to_string(goods_amount),
        "shipping_fee": cents_to_string(shipping_fee),
        "payment_fee": cents_to_string(payment_fee),
        "order_amount": cents_to_string(order_amount),
        "extra_fee": cents_to_string(adjustment.extra),
        "discount": cents_to_string(adjustment.discount),
        "balance_requested": cents_to_string(adjustment.balance),
    })))
}
