//! Shared server-side pricing for quotes, carts and order snapshots.
//!
//! Rules (applied in this order, all server-side; client totals are never trusted):
//! base candidates = shop price discounted for the user's membership rank,
//! rank-specific member price, active promotion (time-limited) price and
//! quantity-tier volume price; the lowest candidate wins. Attribute surcharges
//! are added afterwards. Every arithmetic step is checked and reject on overflow.
use rust_ecshop_domain::error::AppError;
use rust_ecshop_domain::util::unix_now;
use rust_ecshop_infra::rbatis::executor::Executor;
use rust_ecshop_infra::rbutil::{col_cents, col_i64, col_str, q1};
use serde_json::{json, Value};

/// Resolve the membership rank of a user; rank 0 is the rankless normal member.
pub async fn user_rank<E: Executor>(tx: &E, user: i64) -> Result<i64, AppError> {
    Ok(q1(
        tx,
        "SELECT rank_id FROM ecs_user_membership WHERE user_id = ?",
        vec![json!(user)],
    )
    .await?
    .map(|r| col_i64(&r, "rank_id"))
    .unwrap_or(0))
}

/// Rank-level percent discount on the shop price ("90" stores 90% of the price).
async fn rank_discounted<E: Executor>(tx: &E, cents: i64, rank: i64) -> Result<i64, AppError> {
    if rank <= 0 {
        return Ok(cents);
    }
    let Some(row) = q1(
        tx,
        "SELECT discount_percent FROM ecs_user_rank WHERE rank_id = ?",
        vec![json!(rank)],
    )
    .await?
    else {
        return Ok(cents);
    };
    let percent = col_i64(&row, "discount_percent");
    if !(0..=100).contains(&percent) {
        return Err(AppError::InvalidState("invalid rank discount".into()));
    }
    cents
        .checked_mul(percent)
        .and_then(|v| v.checked_div(100))
        .ok_or_else(|| AppError::Validation("amount overflow".into()))
}

/// Rank-specific configured price for a goods; it always wins over the rank discount.
async fn member_price<E: Executor>(tx: &E, goods: i64, rank: i64) -> Result<Option<i64>, AppError> {
    if rank <= 0 {
        return Ok(None);
    }
    Ok(q1(
        tx,
        "SELECT user_price FROM ecs_member_price WHERE goods_id = ? AND rank_id = ?",
        vec![json!(goods), json!(rank)],
    )
    .await?
    .map(|r| col_cents(&r, "user_price")))
}

/// Lowest configured price whose quantity tier is reached.
async fn volume_price<E: Executor>(
    tx: &E,
    goods: i64,
    quantity: i64,
) -> Result<Option<i64>, AppError> {
    Ok(q1(
        tx,
        "SELECT volume_price FROM ecs_volume_price WHERE goods_id = ? AND volume_number <= ? ORDER BY volume_number DESC LIMIT 1",
        vec![json!(goods), json!(quantity)],
    )
    .await?
    .map(|r| col_cents(&r, "volume_price")))
}

/// Active promotion price of the goods window.
async fn promote_price<E: Executor>(tx: &E, goods: i64) -> Result<Option<i64>, AppError> {
    let row = q1(
        tx,
        "SELECT shop_price, promote_price, is_promote, promote_start_date, promote_end_date FROM ecs_goods WHERE goods_id = ?",
        vec![json!(goods)],
    )
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let now = unix_now();
    let promote = col_cents(&row, "promote_price");
    if col_i64(&row, "is_promote") == 1
        && promote > 0
        && col_i64(&row, "promote_start_date") <= now
        && col_i64(&row, "promote_end_date") > now
    {
        return Ok(Some(promote));
    }
    Ok(None)
}

/// Server-side unit price for one goods/quantity with attribute surcharges.
pub async fn unit<E: Executor>(
    tx: &E,
    user: i64,
    goods: i64,
    quantity: i64,
    product: i64,
    attrs: &[i64],
) -> Result<i64, AppError> {
    if !(1..=999).contains(&quantity) {
        return Err(AppError::Validation("quantity must be 1-999".into()));
    }
    let row = q1(
        tx,
        "SELECT * FROM ecs_goods WHERE goods_id=? AND is_on_sale=1 AND is_delete=0",
        vec![json!(goods)],
    )
    .await?
    .ok_or_else(|| AppError::Conflict("goods unavailable".into()))?;
    if col_i64(&row, "goods_number") < quantity {
        return Err(AppError::Conflict("insufficient stock".into()));
    }
    let shop = col_cents(&row, "shop_price");
    let rank = user_rank(tx, user).await?;
    let mut candidates = vec![rank_discounted(tx, shop, rank).await?];
    if let Some(price) = member_price(tx, goods, rank).await? {
        if price >= 0 {
            candidates.push(price);
        }
    }
    if let Some(price) = volume_price(tx, goods, quantity).await? {
        if price >= 0 {
            candidates.push(price);
        }
    }
    if let Some(price) = promote_price(tx, goods).await? {
        candidates.push(price);
    }
    let mut price = *candidates
        .iter()
        .min()
        .ok_or_else(|| AppError::Validation("amount overflow".into()))?;
    let mut unique = std::collections::BTreeSet::new();
    for id in attrs {
        if !unique.insert(*id) {
            return Err(AppError::Validation("duplicate attribute".into()));
        }
        let attr = q1(
            tx,
            "SELECT attr_price FROM ecs_goods_attr WHERE goods_attr_id=? AND goods_id=?",
            vec![json!(id), json!(goods)],
        )
        .await?
        .ok_or_else(|| AppError::Validation("attribute not associated with goods".into()))?;
        price = price
            .checked_add(col_cents(&attr, "attr_price"))
            .ok_or_else(|| AppError::Validation("amount overflow".into()))?;
    }
    if product > 0 {
        let sku = q1(
            tx,
            "SELECT product_number,goods_attr FROM ecs_products WHERE product_id=? AND goods_id=?",
            vec![json!(product), json!(goods)],
        )
        .await?
        .ok_or_else(|| AppError::Validation("SKU not associated with goods".into()))?;
        let configured: std::collections::BTreeSet<i64> = col_str(&sku, "goods_attr")
            .split('|')
            .filter_map(|v| v.parse().ok())
            .collect();
        if configured != unique {
            return Err(AppError::Validation("SKU attributes do not match".into()));
        }
        if col_i64(&sku, "product_number") < quantity {
            return Err(AppError::Conflict("insufficient SKU stock".into()));
        }
    }
    if price < 0 {
        return Err(AppError::InvalidState("negative goods price".into()));
    }
    Ok(price)
}

/// Apply the shared price calculator to cart rows in place.
pub async fn cart<E: Executor>(tx: &E, user: i64, rows: &mut [Value]) -> Result<(), AppError> {
    for row in rows {
        let goods = col_i64(row, "goods_id");
        let spec = q1(
            tx,
            "SELECT product_id,attributes FROM ecs_cart_spec WHERE user_id=? AND goods_id=?",
            vec![json!(user), json!(goods)],
        )
        .await?
        .unwrap_or(Value::Null);
        let product = col_i64(&spec, "product_id");
        let attrs: Vec<i64> =
            serde_json::from_str(&col_str(&spec, "attributes")).unwrap_or_default();
        let price = unit(
            tx,
            user,
            goods,
            col_i64(row, "goods_number"),
            product,
            &attrs,
        )
        .await?;
        row["shop_price"] = json!(rust_ecshop_domain::util::cents_to_string(price));
        row["product_id"] = json!(product);
        row["attributes"] = json!(attrs);
    }
    Ok(())
}
