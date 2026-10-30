//! Shared server-side pricing for quotes, carts and order snapshots.
use crate::shared::{
    error::AppError,
    rbutil::{col_cents, col_i64, col_str, q1},
    util::unix_now,
};
use rbatis::executor::Executor;
use serde_json::{json, Value};

pub async fn unit<E: Executor>(
    tx: &E,
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
    let now = unix_now();
    let mut price = col_cents(&row, "shop_price");
    let promote = col_cents(&row, "promote_price");
    if col_i64(&row, "is_promote") == 1
        && promote > 0
        && col_i64(&row, "promote_start_date") <= now
        && col_i64(&row, "promote_end_date") > now
    {
        price = price.min(promote);
    }
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
        let price = unit(tx, goods, col_i64(row, "goods_number"), product, &attrs).await?;
        row["shop_price"] = json!(crate::shared::util::cents_to_string(price));
        row["product_id"] = json!(product);
        row["attributes"] = json!(attrs);
    }
    Ok(())
}
