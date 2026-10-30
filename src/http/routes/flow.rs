//! Additional flow.php actions using authenticated, persistent checkout selections.
use crate::{
    http::{auth::AuthUser, state::AppState},
    shared::{
        error::AppError,
        rbutil::{col_cents, col_i64, e, q, q1},
        util::{cents_to_string, parse_money_cents, unix_now},
    },
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use rbatis::executor::Executor;
use serde::Deserialize;
use serde_json::{json, Value};

pub async fn clear(State(state): State<AppState>, auth: AuthUser) -> Result<StatusCode, AppError> {
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    e(
        &tx,
        "DELETE FROM ecs_cart WHERE user_id = ?",
        vec![json!(auth.user_id)],
    )
    .await?;
    e(
        &tx,
        "DELETE FROM ecs_cart_offer WHERE user_id=?",
        vec![json!(auth.user_id)],
    )
    .await?;
    e(&tx,"DELETE FROM ecs_cart_spec WHERE user_id=? AND goods_id NOT IN(SELECT goods_id FROM ecs_cart WHERE user_id=?)",vec![json!(auth.user_id),json!(auth.user_id)]).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn collect(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<StatusCode, AppError> {
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let row = q1(
        &tx,
        "SELECT goods_id AS goods_id FROM ecs_cart WHERE rec_id = ? AND user_id = ?",
        vec![json!(id), json!(auth.user_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("cart item not found".into()))?;
    e(
        &tx,
        "INSERT OR IGNORE INTO ecs_collect_goods(user_id,goods_id,add_time) VALUES(?,?,?)",
        vec![
            json!(auth.user_id),
            json!(col_i64(&row, "goods_id")),
            json!(unix_now()),
        ],
    )
    .await?;
    e(
        &tx,
        "DELETE FROM ecs_cart WHERE rec_id = ? AND user_id = ?",
        vec![json!(id), json!(auth.user_id)],
    )
    .await?;
    e(&tx,"DELETE FROM ecs_cart_spec WHERE user_id=? AND goods_id NOT IN(SELECT goods_id FROM ecs_cart WHERE user_id=?)",vec![json!(auth.user_id),json!(auth.user_id)]).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn selection(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let selected = q1(
        state.rb,
        "SELECT * FROM ecs_checkout_selection WHERE user_id = ?",
        vec![json!(auth.user_id)],
    )
    .await?;
    let packs = q(state.rb, "SELECT * FROM ecs_pack ORDER BY pack_id", vec![]).await?;
    let cards = q(state.rb, "SELECT * FROM ecs_card ORDER BY card_id", vec![]).await?;
    Ok(Json(
        json!({"selection":selected,"packs":packs,"cards":cards,"points_per_yuan":100}),
    ))
}

#[derive(Deserialize)]
pub struct SelectionPatch {
    pub value: Value,
}

/// One allow-listed endpoint per old select/change step; all values revalidated at order creation.
pub async fn select(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(field): Path<String>,
    Json(body): Json<SelectionPatch>,
) -> Result<Json<Value>, AppError> {
    let column = match field.as_str() {
        "shipping" => "shipping_id",
        "payment" => "payment_id",
        "pack" => "pack_id",
        "card" => "card_id",
        "surplus" => "surplus_cents",
        "integral" => "integral",
        "bonus" => "bonus_id",
        "insure" => "insure",
        "invoice-title" => "invoice_title",
        "invoice-content" => "invoice_content",
        "invoice" => "need_invoice",
        "oos" => "oos_policy",
        _ => return Err(AppError::Validation("unknown checkout selection".into())),
    };
    let val = if column == "surplus_cents" {
        json!(parse_money_cents(
            body.value.as_str().unwrap_or(""),
            "surplus"
        )?)
    } else if matches!(column, "invoice_title" | "invoice_content" | "oos_policy") {
        let text = body
            .value
            .as_str()
            .ok_or_else(|| AppError::Validation("string value required".into()))?;
        if text.len() > 255
            || (column == "oos_policy" && !["cancel", "wait", "partial"].contains(&text))
        {
            return Err(AppError::Validation("invalid selection".into()));
        }
        json!(text)
    } else {
        let number = body
            .value
            .as_i64()
            .ok_or_else(|| AppError::Validation("integer value required".into()))?;
        if number < 0 || (matches!(column, "insure" | "need_invoice") && number > 1) {
            return Err(AppError::Validation("invalid selection".into()));
        }
        json!(number)
    };
    // Insurance needs an actual carrier quote; mock adapter supplies zero premium explicitly.
    if column == "insure" && val.as_i64() == Some(1) {
        super::mock::enabled()?;
    }
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    e(
        &tx,
        "INSERT OR IGNORE INTO ecs_checkout_selection(user_id) VALUES(?)",
        vec![json!(auth.user_id)],
    )
    .await?;
    e(
        &tx,
        &format!("UPDATE ecs_checkout_selection SET {column} = ? WHERE user_id = ?"),
        vec![val, json!(auth.user_id)],
    )
    .await?;
    tx.commit().await?;
    selection(State(state), auth).await
}

pub async fn validate_funds(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let row=q1(state.rb,"SELECT u.pay_points AS points, COALESCE(b.available_cents,0) AS available_cents FROM ecs_users u LEFT JOIN ecs_account_balance b ON b.user_id=u.user_id WHERE u.user_id = ?",vec![json!(auth.user_id)]).await?.unwrap_or_default();
    Ok(Json(
        json!({"integral":col_i64(&row,"points"),"surplus":cents_to_string(col_i64(&row,"available_cents"))}),
    ))
}

pub async fn bonus<E: Executor>(tx: &E, user: i64, id: i64, goods: i64) -> Result<i64, AppError> {
    if id == 0 {
        return Ok(0);
    }
    let now = unix_now();
    let row=q1(tx,"SELECT t.type_money AS type_money,t.min_goods_amount AS min_goods_amount FROM ecs_user_bonus b JOIN ecs_bonus_type t ON t.type_id=b.bonus_type_id WHERE b.bonus_id = ? AND b.user_id = ? AND b.used_time = 0 AND b.order_id = 0 AND t.use_start_date <= ? AND t.use_end_date > ?",vec![json!(id),json!(user),json!(now),json!(now)]).await?.ok_or_else(||AppError::Conflict("bonus unavailable".into()))?;
    if goods < col_cents(&row, "min_goods_amount") {
        return Err(AppError::Conflict("bonus minimum not reached".into()));
    }
    Ok(col_cents(&row, "type_money").min(goods))
}

#[derive(Default)]
pub struct Adjustment {
    pub extra: i64,
    pub discount: i64,
    pub balance: i64,
    pub points: i64,
    pub bonus_id: i64,
    pub snapshot: Value,
}

pub async fn adjustments<E: Executor>(
    tx: &E,
    user: i64,
    goods: i64,
) -> Result<Adjustment, AppError> {
    let offer_discount = offer_discount(tx, user, goods).await?;
    let Some(row) = q1(
        tx,
        "SELECT * FROM ecs_checkout_selection WHERE user_id = ?",
        vec![json!(user)],
    )
    .await?
    else {
        return Ok(Adjustment {
            discount: offer_discount,
            ..Adjustment::default()
        });
    };
    let mut extra = 0i64;
    for (field, table, key, fee) in [
        ("pack_id", "ecs_pack", "pack_id", "pack_fee"),
        ("card_id", "ecs_card", "card_id", "card_fee"),
    ] {
        let id = col_i64(&row, field);
        if id != 0 {
            let item = q1(
                tx,
                &format!("SELECT {fee} AS fee FROM {table} WHERE {key} = ?"),
                vec![json!(id)],
            )
            .await?
            .ok_or_else(|| AppError::NotFound(format!("{field} not found")))?;
            extra = extra
                .checked_add(col_cents(&item, "fee"))
                .ok_or_else(|| AppError::Validation("amount overflow".into()))?;
        }
    }
    let points = col_i64(&row, "integral");
    let bonus_id = col_i64(&row, "bonus_id");
    let discount = bonus(tx, user, bonus_id, goods).await?;
    if points > goods - discount {
        return Err(AppError::Validation("points exceed goods amount".into()));
    }
    let available = q1(
        tx,
        "SELECT pay_points AS points FROM ecs_users WHERE user_id = ?",
        vec![json!(user)],
    )
    .await?
    .map(|r| col_i64(&r, "points"))
    .unwrap_or(0);
    if points > available {
        return Err(AppError::Conflict("insufficient points".into()));
    }
    if col_i64(&row, "insure") == 1 {
        super::mock::enabled()?;
    }
    Ok(Adjustment {
        extra,
        discount: discount + points + offer_discount,
        balance: col_i64(&row, "surplus_cents"),
        points,
        bonus_id,
        snapshot: row,
    })
}

pub async fn apply<E: Executor>(
    tx: &E,
    user: i64,
    order: i64,
    total: i64,
    a: &Adjustment,
) -> Result<(), AppError> {
    if a.points > 0 {
        let r=e(tx,"UPDATE ecs_users SET pay_points = pay_points - ? WHERE user_id = ? AND pay_points >= ?",vec![json!(a.points),json!(user),json!(a.points)]).await?;
        if r.rows_affected != 1 {
            return Err(AppError::Conflict("insufficient points".into()));
        }
    }
    if a.bonus_id > 0 {
        let r=e(tx,"UPDATE ecs_user_bonus SET used_time = ?, order_id = ? WHERE bonus_id = ? AND user_id = ? AND used_time = 0 AND order_id = 0",vec![json!(unix_now()),json!(order),json!(a.bonus_id),json!(user)]).await?;
        if r.rows_affected != 1 {
            return Err(AppError::Conflict("bonus already used".into()));
        }
    }
    let paid = a.balance.min(total);
    if paid > 0 {
        let r=e(tx,"UPDATE ecs_account_balance SET available_cents=available_cents-? WHERE user_id=? AND available_cents>=?",vec![json!(paid),json!(user),json!(paid)]).await?;
        if r.rows_affected != 1 {
            return Err(AppError::Conflict("insufficient balance".into()));
        }
        e(tx,"INSERT INTO ecs_order_balance_payment(order_id,user_id,paid_cents,updated_at) VALUES(?,?,?,?)",vec![json!(order),json!(user),json!(paid),json!(unix_now())]).await?;
        e(tx,"INSERT INTO ecs_account_log(user_id,available_delta_cents,reason,reference_type,reference_id) VALUES(?,?,'checkout','order_info',?)",vec![json!(user),json!(-paid),json!(order)]).await?;
    }
    if paid == total {
        e(
            tx,
            "UPDATE ecs_order_info SET order_status='paid' WHERE order_id=?",
            vec![json!(order)],
        )
        .await?;
    }
    e(
        tx,
        "INSERT INTO ecs_checkout_audit(order_id,selection,discount_cents) VALUES(?,?,?)",
        vec![
            json!(order),
            json!(a.snapshot.to_string()),
            json!(a.discount),
        ],
    )
    .await?;
    e(
        tx,
        "DELETE FROM ecs_cart_offer WHERE user_id=?",
        vec![json!(user)],
    )
    .await?;
    e(
        tx,
        "DELETE FROM ecs_checkout_selection WHERE user_id=?",
        vec![json!(user)],
    )
    .await?;
    Ok(())
}

pub async fn validate_bonus(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let goods=q1(state.rb,"SELECT COALESCE(SUM(c.goods_number*g.shop_price),0) AS total FROM ecs_cart c JOIN ecs_goods g ON g.goods_id=c.goods_id WHERE c.user_id=?",vec![json!(auth.user_id)]).await?.unwrap_or_default();
    let amount = bonus(state.rb, auth.user_id, id, col_cents(&goods, "total")).await?;
    Ok(Json(
        json!({"bonus_id":id,"discount":cents_to_string(amount)}),
    ))
}

#[derive(Deserialize)]
pub struct LinkedBuy {
    goods_id: i64,
    quantity: i64,
}
/// link_buy: replace the cart and add the requested product atomically.
pub async fn linked_buy(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<LinkedBuy>,
) -> Result<Json<Value>, AppError> {
    if !(1..=999).contains(&body.quantity) {
        return Err(AppError::Validation("quantity must be 1-999".into()));
    }
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let goods=q1(&tx,"SELECT goods_id FROM ecs_goods WHERE goods_id=? AND goods_number>=? AND is_on_sale=1 AND is_delete=0",vec![json!(body.goods_id),json!(body.quantity)]).await?;
    if goods.is_none() {
        return Err(AppError::Conflict("goods unavailable".into()));
    }
    e(
        &tx,
        "DELETE FROM ecs_cart WHERE user_id=?",
        vec![json!(auth.user_id)],
    )
    .await?;
    e(
        &tx,
        "DELETE FROM ecs_cart_offer WHERE user_id=?",
        vec![json!(auth.user_id)],
    )
    .await?;
    e(
        &tx,
        "INSERT INTO ecs_cart(user_id,goods_id,goods_number,version) VALUES(?,?,?,1)",
        vec![
            json!(auth.user_id),
            json!(body.goods_id),
            json!(body.quantity),
        ],
    )
    .await?;
    e(
        &tx,
        "DELETE FROM ecs_cart_spec WHERE user_id=?",
        vec![json!(auth.user_id)],
    )
    .await?;
    tx.commit().await?;
    super::cart::cart_list(State(state), auth).await
}

/// Package purchase replaces the cart, keeping the bundle price bound to exactly its members.
pub async fn package(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let now = unix_now();
    if q1(&tx,"SELECT act_id FROM ecs_goods_activity WHERE act_id=? AND act_type=4 AND is_finished=0 AND start_time<=? AND end_time>?",vec![json!(id),json!(now),json!(now)]).await?.is_none(){return Err(AppError::NotFound("active package not found".into()));}
    let rows=q(&tx,"SELECT p.goods_id AS goods_id,p.goods_number AS quantity,p.product_id AS product_id,g.goods_number AS stock,g.is_on_sale AS sale,g.is_delete AS deleted FROM ecs_package_goods p JOIN ecs_goods g ON g.goods_id=p.goods_id WHERE p.package_id=?",vec![json!(id)]).await?;
    if rows.is_empty() {
        return Err(AppError::Conflict("empty package".into()));
    }
    e(
        &tx,
        "DELETE FROM ecs_cart WHERE user_id=?",
        vec![json!(auth.user_id)],
    )
    .await?;
    for row in rows {
        let qty = col_i64(&row, "quantity");
        if qty <= 0
            || qty > col_i64(&row, "stock")
            || col_i64(&row, "sale") != 1
            || col_i64(&row, "deleted") != 0
            || col_i64(&row, "product_id") != 0
        {
            return Err(AppError::Conflict(
                "package item unavailable or requires SKU".into(),
            ));
        }
        e(
            &tx,
            "INSERT INTO ecs_cart(user_id,goods_id,goods_number,version) VALUES(?,?,?,1)",
            vec![
                json!(auth.user_id),
                json!(col_i64(&row, "goods_id")),
                json!(qty),
            ],
        )
        .await?;
    }
    e(&tx,"INSERT INTO ecs_cart_offer(user_id,package_id) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET package_id=excluded.package_id,favourable_id=0",vec![json!(auth.user_id),json!(id)]).await?;
    e(
        &tx,
        "DELETE FROM ecs_cart_spec WHERE user_id=?",
        vec![json!(auth.user_id)],
    )
    .await?;
    tx.commit().await?;
    super::cart::cart_list(State(state), auth).await
}

pub async fn favourable(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    let now = unix_now();
    let offer = q1(
        state.rb,
        "SELECT * FROM ecs_favourable_activity WHERE act_id=? AND start_time<=? AND end_time>?",
        vec![json!(id), json!(now), json!(now)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("active offer not found".into()))?;
    // Only global offers available to all ranks; scoped/rank offers require migrated membership rules.
    if col_i64(&offer, "act_range") != 0
        || !crate::shared::rbutil::col_str(&offer, "user_rank")
            .split(',')
            .any(|r| r == "0")
        || col_i64(&offer, "act_type") == 0
    {
        return Err(AppError::Validation(
            "offer requires unsupported gift/rank/range rules".into(),
        ));
    }
    e(state.rb,"INSERT INTO ecs_cart_offer(user_id,favourable_id) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET favourable_id=excluded.favourable_id,package_id=0",vec![json!(auth.user_id),json!(id)]).await?;
    Ok(Json(json!({"favourable_id":id})))
}

async fn offer_discount<E: Executor>(tx: &E, user: i64, goods: i64) -> Result<i64, AppError> {
    let Some(offer) = q1(
        tx,
        "SELECT * FROM ecs_cart_offer WHERE user_id=?",
        vec![json!(user)],
    )
    .await?
    else {
        return Ok(0);
    };
    let now = unix_now();
    let package = col_i64(&offer, "package_id");
    if package > 0 {
        let activity=q1(tx,"SELECT ext_info FROM ecs_goods_activity WHERE act_id=? AND act_type=4 AND is_finished=0 AND start_time<=? AND end_time>?",vec![json!(package),json!(now),json!(now)]).await?.ok_or_else(||AppError::Conflict("package expired".into()))?;
        let cart = q(
            tx,
            "SELECT goods_id,goods_number FROM ecs_cart WHERE user_id=? ORDER BY goods_id",
            vec![json!(user)],
        )
        .await?;
        let members=q(tx,"SELECT goods_id,goods_number FROM ecs_package_goods WHERE package_id=? ORDER BY goods_id",vec![json!(package)]).await?;
        if cart != members {
            return Err(AppError::Conflict(
                "package contents changed; re-add package".into(),
            ));
        }
        let config: Value =
            serde_json::from_str(&crate::shared::rbutil::col_str(&activity, "ext_info"))
                .map_err(|_| AppError::InvalidState("package ext_info must be JSON".into()))?;
        let price = parse_money_cents(
            config
                .get("package_price")
                .and_then(Value::as_str)
                .unwrap_or(""),
            "package_price",
        )?;
        return Ok(goods - price); // Negative discount represents bundle markup.
    }
    let id = col_i64(&offer, "favourable_id");
    if id == 0 {
        return Ok(0);
    }
    let a = q1(
        tx,
        "SELECT * FROM ecs_favourable_activity WHERE act_id=? AND start_time<=? AND end_time>?",
        vec![json!(id), json!(now), json!(now)],
    )
    .await?
    .ok_or_else(|| AppError::Conflict("offer expired".into()))?;
    if goods < col_cents(&a, "min_amount")
        || (col_cents(&a, "max_amount") > 0 && goods > col_cents(&a, "max_amount"))
    {
        return Err(AppError::Conflict("offer amount outside limits".into()));
    }
    match col_i64(&a, "act_type") {
        1 => Ok(col_cents(&a, "act_type_ext").min(goods)),
        2 => {
            let percent = col_cents(&a, "act_type_ext");
            if !(0..=10000).contains(&percent) {
                return Err(AppError::InvalidState("invalid discount percentage".into()));
            }
            Ok(goods
                - goods
                    .checked_mul(percent)
                    .ok_or_else(|| AppError::Validation("amount overflow".into()))?
                    / 10000)
        }
        _ => Err(AppError::Validation(
            "gift offer requires gift selection".into(),
        )),
    }
}
