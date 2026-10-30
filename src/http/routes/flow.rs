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
    crate::infrastructure::db::lock_user(&tx, auth.user_id).await?;
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
    e(
        &tx,
        "DELETE FROM ecs_cart_gift WHERE user_id=?",
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
    crate::infrastructure::db::lock_user(&tx, auth.user_id).await?;
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
    crate::infrastructure::db::lock_user(&tx, auth.user_id).await?;
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
    pub offer_id: i64,
    pub snapshot: Value,
}

pub async fn adjustments<E: Executor>(
    tx: &E,
    user: i64,
    goods: i64,
    rows: &[Value],
) -> Result<Adjustment, AppError> {
    let offer = offer_discount(tx, user, goods, rows).await?;
    let Some(row) = q1(
        tx,
        "SELECT * FROM ecs_checkout_selection WHERE user_id = ?",
        vec![json!(user)],
    )
    .await?
    else {
        return Ok(Adjustment {
            discount: offer.discount,
            offer_id: offer.act_id,
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
    let bonus = bonus(tx, user, bonus_id, goods).await?;
    // Bonus, points and offer discount stack in this order, capped by the
    // applicable goods amount so the payable amount can never go negative.
    let combined = bonus
        .checked_add(offer.discount)
        .ok_or_else(|| AppError::Validation("amount overflow".into()))?;
    if points > goods - combined {
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
    let sum = combined
        .checked_add(points)
        .ok_or_else(|| AppError::Validation("amount overflow".into()))?;
    // min cannot overflow; it only bounds the stack by the goods amount.
    let discount = sum.min(goods);
    Ok(Adjustment {
        extra,
        discount,
        balance: col_i64(&row, "surplus_cents"),
        points,
        bonus_id,
        offer_id: offer.act_id,
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
        "DELETE FROM ecs_cart_gift WHERE user_id=?",
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
    crate::infrastructure::db::lock_user(&tx, auth.user_id).await?;
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
        "DELETE FROM ecs_cart_gift WHERE user_id=?",
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
    crate::infrastructure::db::lock_user(&tx, auth.user_id).await?;
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
        "DELETE FROM ecs_cart_gift WHERE user_id=?",
        vec![json!(auth.user_id)],
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

pub async fn favourable(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    crate::infrastructure::db::lock_user(&tx, auth.user_id).await?;
    let offer = q1(
        &tx,
        "SELECT * FROM ecs_favourable_activity WHERE act_id=? AND start_time<=? AND end_time>?",
        vec![json!(id), json!(now), json!(now)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("active offer not found".into()))?;
    let act_type = col_i64(&offer, "act_type");
    if !(0..=2).contains(&act_type) {
        return Err(AppError::Validation("unsupported offer type".into()));
    }
    // The offer's user_rank is an explicit list (0 = normal member).
    require_rank(&offer, super::pricing::user_rank(&tx, auth.user_id).await?)?;
    let gifts: Vec<GiftSelection> = body
        .get("gifts")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| AppError::Validation("gifts must be a list".into()))?
        .unwrap_or_default();
    if act_type == 0 {
        sync_gifts(&tx, auth.user_id, id, &gifts).await?;
    } else if !gifts.is_empty() {
        return Err(AppError::Validation(
            "gifts apply only to gift offers".into(),
        ));
    } else {
        e(
            &tx,
            "DELETE FROM ecs_cart_gift WHERE user_id=?",
            vec![json!(auth.user_id)],
        )
        .await?;
    }
    e(
        &tx,
        "INSERT INTO ecs_cart_offer(user_id,favourable_id) VALUES(?,?) ON CONFLICT(user_id) DO UPDATE SET favourable_id=excluded.favourable_id,package_id=0",
        vec![json!(auth.user_id), json!(id)],
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"favourable_id": id})))
}

/// The offer's user_rank is an explicit rank list; 0 denotes the normal member.
/// Users outside the list cannot select or benefit from the offer.
fn require_rank(offer: &Value, rank: i64) -> Result<(), AppError> {
    let allowed: Vec<i64> = crate::shared::rbutil::col_str(offer, "user_rank")
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect();
    if allowed.contains(&rank) {
        Ok(())
    } else {
        Err(AppError::Forbidden)
    }
}

#[derive(Deserialize, Debug)]
pub struct GiftSelection {
    goods_id: i64,
    quantity: i64,
}

/// Validate gift selections against the offer's gift allow-list and stock, then
/// persist them for the cart. Zero-amount gifts never accept client prices.
async fn sync_gifts<E: Executor>(
    tx: &E,
    user: i64,
    offer_id: i64,
    gifts: &[GiftSelection],
) -> Result<(), AppError> {
    let Some(offer) = q1(
        tx,
        "SELECT gift,act_type_ext FROM ecs_favourable_activity WHERE act_id=?",
        vec![json!(offer_id)],
    )
    .await?
    else {
        return Err(AppError::NotFound("active offer not found".into()));
    };
    let config: Value = serde_json::from_str(&crate::shared::rbutil::col_str(&offer, "gift"))
        .map_err(|_| AppError::InvalidState("offer gift config must be JSON".into()))?;
    let allowed: Vec<i64> = config
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|g| g.get("id").and_then(Value::as_i64))
                .collect()
        })
        .unwrap_or_default();
    let limit = col_i64(&offer, "act_type_ext");
    if limit <= 0 {
        return Err(AppError::Validation("offer allows no gifts".into()));
    }
    let mut total = 0i64;
    let mut seen = std::collections::BTreeSet::new();
    for gift in gifts {
        if !seen.insert(gift.goods_id) {
            return Err(AppError::Validation("duplicate gift goods".into()));
        }
        if !allowed.contains(&gift.goods_id) {
            return Err(AppError::Validation("gift goods not in allow-list".into()));
        }
        if !(1..=999).contains(&gift.quantity) {
            return Err(AppError::Validation("gift quantity must be 1-999".into()));
        }
        total += gift.quantity;
    }
    if total > limit {
        return Err(AppError::Validation(
            "gift quantity exceeds the offer limit".into(),
        ));
    }
    for gift in gifts {
        if q1(
            tx,
            "SELECT goods_id AS ok FROM ecs_goods WHERE goods_id=? AND is_on_sale=1 AND is_delete=0 AND goods_number>=?",
            vec![json!(gift.goods_id), json!(gift.quantity)],
        )
        .await?
        .is_none()
        {
            return Err(AppError::Conflict("gift goods unavailable".into()));
        }
    }
    e(
        tx,
        "DELETE FROM ecs_cart_gift WHERE user_id=?",
        vec![json!(user)],
    )
    .await?;
    for gift in gifts {
        e(
            tx,
            "INSERT INTO ecs_cart_gift(user_id,goods_id,quantity) VALUES(?,?,?)",
            vec![json!(user), json!(gift.goods_id), json!(gift.quantity)],
        )
        .await?;
    }
    Ok(())
}

fn parse_id_list(text: &str) -> Vec<i64> {
    text.split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect()
}

/// PHP FAR_* range constants: 0 all, 1 category, 2 brand, 3 goods.
const FAR_ALL: i64 = 0;
const FAR_CATEGORY: i64 = 1;
const FAR_BRAND: i64 = 2;
const FAR_GOODS: i64 = 3;

fn good_matches_range(
    range: i64,
    ext_ids: &[i64],
    allowed: &[i64],
    goods_id: i64,
    cat_id: i64,
    brand_id: i64,
) -> bool {
    match range {
        FAR_ALL => true,
        FAR_CATEGORY => allowed.contains(&cat_id),
        FAR_BRAND => allowed.contains(&brand_id),
        FAR_GOODS => ext_ids.contains(&goods_id),
        _ => false,
    }
}

/// Category matching includes descendants of the seeded categories (depth-limited).
async fn expand_categories<E: Executor>(tx: &E, seeds: &[i64]) -> Result<Vec<i64>, AppError> {
    let rows = q(
        tx,
        "SELECT cat_id AS cat_id, parent_id AS parent_id FROM ecs_category",
        vec![],
    )
    .await?;
    let mut children: std::collections::HashMap<i64, Vec<i64>> = std::collections::HashMap::new();
    for row in &rows {
        children
            .entry(col_i64(row, "parent_id"))
            .or_default()
            .push(col_i64(row, "cat_id"));
    }
    let mut queue: std::collections::VecDeque<(i64, i64)> =
        seeds.iter().map(|id| (*id, 0i64)).collect();
    let mut seen: std::collections::BTreeSet<i64> = seeds.iter().copied().collect();
    while let Some((cat, depth)) = queue.pop_front() {
        if depth >= 10 {
            continue;
        }
        if let Some(kids) = children.get(&cat) {
            for kid in kids {
                if seen.insert(*kid) {
                    queue.push_back((*kid, depth + 1));
                }
            }
        }
    }
    Ok(seen.into_iter().collect())
}

/// Discount produced by the selected offer for the current cart contents.
pub struct OfferDiscount {
    pub discount: i64,
    pub act_id: i64,
}

async fn offer_discount<E: Executor>(
    tx: &E,
    user: i64,
    goods: i64,
    rows: &[Value],
) -> Result<OfferDiscount, AppError> {
    let Some(offer) = q1(
        tx,
        "SELECT * FROM ecs_cart_offer WHERE user_id=?",
        vec![json!(user)],
    )
    .await?
    else {
        return Ok(OfferDiscount {
            discount: 0,
            act_id: 0,
        });
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
        // Negative discount represents bundle markup.
        return Ok(OfferDiscount {
            discount: goods - price,
            act_id: 0,
        });
    }
    let id = col_i64(&offer, "favourable_id");
    if id == 0 {
        return Ok(OfferDiscount {
            discount: 0,
            act_id: 0,
        });
    }
    let a = q1(
        tx,
        "SELECT * FROM ecs_favourable_activity WHERE act_id=? AND start_time<=? AND end_time>?",
        vec![json!(id), json!(now), json!(now)],
    )
    .await?
    .ok_or_else(|| AppError::Conflict("offer expired".into()))?;
    require_rank(&a, super::pricing::user_rank(tx, user).await?)?;
    let range = col_i64(&a, "act_range");
    let ext_ids = parse_id_list(&crate::shared::rbutil::col_str(&a, "act_range_ext"));
    let allowed = if range == FAR_CATEGORY {
        expand_categories(tx, &ext_ids).await?
    } else {
        ext_ids.clone()
    };
    let mut applicable = 0i64;
    for row in rows {
        let matched = good_matches_range(
            range,
            &ext_ids,
            &allowed,
            col_i64(row, "goods_id"),
            col_i64(row, "cat_id"),
            col_i64(row, "brand_id"),
        );
        if !matched {
            continue;
        }
        let price = col_cents(row, "shop_price");
        let quantity = col_i64(row, "goods_number");
        applicable = applicable
            .checked_add(
                price
                    .checked_mul(quantity)
                    .ok_or_else(|| AppError::Validation("amount overflow".into()))?,
            )
            .ok_or_else(|| AppError::Validation("amount overflow".into()))?;
    }
    let min_amount = col_cents(&a, "min_amount");
    let max_amount = col_cents(&a, "max_amount");
    if applicable < min_amount || (max_amount > 0 && applicable > max_amount) {
        return Err(AppError::Conflict("offer amount outside limits".into()));
    }
    match col_i64(&a, "act_type") {
        1 => Ok(OfferDiscount {
            discount: col_cents(&a, "act_type_ext").min(applicable),
            act_id: id,
        }),
        2 => {
            let percent = col_cents(&a, "act_type_ext");
            if !(0..=10000).contains(&percent) {
                return Err(AppError::InvalidState("invalid discount percentage".into()));
            }
            let pay = applicable
                .checked_mul(percent)
                .ok_or_else(|| AppError::Validation("amount overflow".into()))?
                / 10000;
            Ok(OfferDiscount {
                discount: (applicable - pay).min(applicable),
                act_id: id,
            })
        }
        // FAT_GOODS: gifts are granted free of charge and re-validated at order time.
        0 => Ok(OfferDiscount {
            discount: 0,
            act_id: id,
        }),
        _ => Err(AppError::Validation("unsupported offer type".into())),
    }
}
