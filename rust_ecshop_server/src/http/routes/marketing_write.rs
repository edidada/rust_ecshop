//! Persistent marketing writes: points redemption, auction/snatch bidding and group-buy ordering.
use crate::http::{auth::AuthUser, state::AppState};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use rust_ecshop_domain::error::AppError;
use rust_ecshop_domain::util::{cents_to_string, parse_money_cents, unix_now};
use rust_ecshop_infra::rbutil::{col_cents, col_i64, col_str, e, insert_id, q1};
use serde::Deserialize;
use serde_json::{json, Value};

fn key(headers: &HeaderMap) -> Result<String, AppError> {
    headers
        .get("Idempotency-Key")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty() && s.len() <= 100)
        .map(str::to_owned)
        .ok_or_else(|| AppError::Validation("Idempotency-Key required (1-100 bytes)".into()))
}
#[derive(Deserialize)]
pub struct Quantity {
    quantity: i64,
}

pub async fn redeem(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Json(body): Json<Quantity>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if !(1..=999).contains(&body.quantity) {
        return Err(AppError::Validation("quantity must be 1-999".into()));
    }
    let key = key(&headers)?;
    let tx = rust_ecshop_infra::db::begin(state.rb).await?;
    rust_ecshop_infra::db::lock_user(&tx, auth.user_id).await?;
    if let Some(old) = q1(
        &tx,
        "SELECT * FROM ecs_redemption WHERE user_id=? AND idempotency_key=?",
        vec![json!(auth.user_id), json!(key)],
    )
    .await?
    {
        if col_i64(&old, "goods_id") != id || col_i64(&old, "quantity") != body.quantity {
            return Err(AppError::Conflict("idempotency body mismatch".into()));
        }
        return Ok((
            StatusCode::OK,
            Json(json!({"replayed":true,"redemption":old})),
        ));
    }
    let goods=q1(&tx,"SELECT x.exchange_integral AS points FROM ecs_exchange_goods x JOIN ecs_goods g ON g.goods_id=x.goods_id WHERE x.goods_id=? AND x.is_exchange=1 AND g.is_on_sale=1 AND g.is_delete=0",vec![json!(id)]).await?.ok_or_else(||AppError::NotFound("exchange goods unavailable".into()))?;
    let points = col_i64(&goods, "points")
        .checked_mul(body.quantity)
        .filter(|p| *p > 0)
        .ok_or_else(|| AppError::Validation("invalid exchange points".into()))?;
    let user = e(
        &tx,
        "UPDATE ecs_users SET pay_points=pay_points-? WHERE user_id=? AND pay_points>=?",
        vec![json!(points), json!(auth.user_id), json!(points)],
    )
    .await?;
    let stock=e(&tx,"UPDATE ecs_goods SET goods_number=goods_number-? WHERE goods_id=? AND goods_number>=? AND is_on_sale=1 AND is_delete=0",vec![json!(body.quantity),json!(id),json!(body.quantity)]).await?;
    if user.rows_affected != 1 || stock.rows_affected != 1 {
        return Err(AppError::Conflict("insufficient points or stock".into()));
    }
    let result=e(&tx,"INSERT INTO ecs_redemption(user_id,goods_id,quantity,points,idempotency_key,created_at) VALUES(?,?,?,?,?,?)",vec![json!(auth.user_id),json!(id),json!(body.quantity),json!(points),json!(key),json!(unix_now())]).await?;
    rust_ecshop_infra::db::commit(&tx).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"id":insert_id(&result),"points":points,"quantity":body.quantity,"replayed":false}),
        ),
    ))
}

#[derive(Deserialize)]
pub struct Bid {
    price: String,
}

pub async fn bid(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<Bid>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let price = parse_money_cents(&body.price, "price")?;
    let tx = rust_ecshop_infra::db::begin(state.rb).await?;
    rust_ecshop_infra::db::lock_user(&tx, auth.user_id).await?;
    e(
        &tx,
        "INSERT OR IGNORE INTO ecs_promotion_version(promotion_id) VALUES(?)",
        vec![json!(id)],
    )
    .await?;
    // Serialize bid evaluation before reading the current winner.
    e(
        &tx,
        "UPDATE ecs_promotion_version SET version=version+1 WHERE promotion_id=?",
        vec![json!(id)],
    )
    .await?;
    let now = unix_now();
    let activity=q1(&tx,"SELECT * FROM ecs_goods_activity WHERE act_id=? AND act_type IN (0,2) AND is_finished=0 AND start_time<=? AND end_time>?",vec![json!(id),json!(now),json!(now)]).await?.ok_or_else(||AppError::NotFound("active auction or snatch not found".into()))?;
    let config: Value = serde_json::from_str(&col_str(&activity, "ext_info"))
        .map_err(|_| AppError::InvalidState("activity ext_info must be migrated to JSON".into()))?;
    let money = |field: &str| -> Result<i64, AppError> {
        match config.get(field) {
            None => Ok(0),
            Some(Value::String(s)) => parse_money_cents(s, field),
            Some(v) => parse_money_cents(&v.to_string(), field),
        }
    };
    let last = q1(
        &tx,
        "SELECT * FROM ecs_promotion_bid WHERE promotion_id=? ORDER BY bid_id DESC LIMIT 1",
        vec![json!(id)],
    )
    .await?;
    let is_auction = col_i64(&activity, "act_type") == 0;
    let minimum = if is_auction {
        last.as_ref()
            .map(|r| col_i64(r, "price_cents").saturating_add(money("amplitude").unwrap_or(0)))
            .unwrap_or(money("start_price")?)
    } else {
        money("min_price")?
    };
    let end = money(if is_auction { "end_price" } else { "max_price" })?;
    let minimum = if is_auction && end > 0 {
        minimum.min(end)
    } else {
        minimum
    };
    if price <= 0 || price < minimum || (end > 0 && price > end) {
        return Err(AppError::Validation("bid outside allowed range".into()));
    }
    if is_auction
        && last
            .as_ref()
            .is_some_and(|r| col_i64(r, "user_id") == auth.user_id)
    {
        return Err(AppError::Conflict("current winner cannot bid again".into()));
    }
    if !is_auction && q1(&tx,"SELECT bid_id FROM ecs_promotion_bid WHERE promotion_id=? AND user_id=? AND price_cents=?",vec![json!(id),json!(auth.user_id),json!(price)]).await?.is_some(){return Err(AppError::Conflict("duplicate snatch bid".into()));}
    if !is_auction {
        let points = config
            .get("cost_points")
            .and_then(|v| {
                v.as_i64()
                    .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
            })
            .unwrap_or(0);
        if points < 0 {
            return Err(AppError::InvalidState("negative snatch points".into()));
        }
        let r = e(
            &tx,
            "UPDATE ecs_users SET pay_points=pay_points-? WHERE user_id=? AND pay_points>=?",
            vec![json!(points), json!(auth.user_id), json!(points)],
        )
        .await?;
        if r.rows_affected != 1 {
            return Err(AppError::Conflict("insufficient snatch points".into()));
        }
    }
    let deposit = if is_auction { money("deposit")? } else { 0 };
    if deposit > 0 {
        let r=e(&tx,"UPDATE ecs_account_balance SET available_cents=available_cents-?,frozen_cents=frozen_cents+? WHERE user_id=? AND available_cents>=?",vec![json!(deposit),json!(deposit),json!(auth.user_id),json!(deposit)]).await?;
        if r.rows_affected != 1 {
            return Err(AppError::Conflict("insufficient auction deposit".into()));
        }
        e(&tx,"INSERT INTO ecs_account_log(user_id,available_delta_cents,frozen_delta_cents,reason,reference_type,reference_id) VALUES(?,?,?,'auction_deposit','promotion',?)",vec![json!(auth.user_id),json!(-deposit),json!(deposit),json!(id)]).await?;
    }
    if is_auction {
        if let Some(old) = last {
            let frozen = col_i64(&old, "deposit_cents");
            let user = col_i64(&old, "user_id");
            if frozen > 0 {
                e(&tx,"UPDATE ecs_account_balance SET available_cents=available_cents+?,frozen_cents=frozen_cents-? WHERE user_id=? AND frozen_cents>=?",vec![json!(frozen),json!(frozen),json!(user),json!(frozen)]).await?;
                e(&tx,"INSERT INTO ecs_account_log(user_id,available_delta_cents,frozen_delta_cents,reason,reference_type,reference_id) VALUES(?,?,?,'auction_outbid','promotion',?)",vec![json!(user),json!(frozen),json!(-frozen),json!(id)]).await?;
            }
        }
    }
    let result=e(&tx,"INSERT INTO ecs_promotion_bid(promotion_id,user_id,price_cents,deposit_cents,created_at) VALUES(?,?,?,?,?)",vec![json!(id),json!(auth.user_id),json!(price),json!(deposit),json!(now)]).await?;
    if is_auction && end > 0 && price == end {
        e(
            &tx,
            "UPDATE ecs_goods_activity SET is_finished=1 WHERE act_id=?",
            vec![json!(id)],
        )
        .await?;
    }
    rust_ecshop_infra::db::commit(&tx).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"id":insert_id(&result),"price":cents_to_string(price),"deposit":cents_to_string(deposit)}),
        ),
    ))
}

#[derive(Deserialize)]
pub struct GroupBuy {
    quantity: i64,
    address_id: i64,
    shipping_id: i64,
    payment_id: i64,
}

pub async fn group_buy(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Json(body): Json<GroupBuy>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if !(1..=999).contains(&body.quantity) {
        return Err(AppError::Validation("quantity must be 1-999".into()));
    }
    let key = key(&headers)?;
    let fingerprint = rust_ecshop_infra::crypto::sha256_hex(
        json!([
            id,
            body.quantity,
            body.address_id,
            body.shipping_id,
            body.payment_id
        ])
        .to_string()
        .as_bytes(),
    );
    let tx = rust_ecshop_infra::db::begin(state.rb).await?;
    rust_ecshop_infra::db::lock_user(&tx, auth.user_id).await?;
    if let Some(row)=q1(&tx,"SELECT order_id AS order_id,request_fingerprint AS fingerprint FROM ecs_order_info WHERE user_id=? AND idempotency_key=?",vec![json!(auth.user_id),json!(key)]).await? {
        if col_str(&row,"fingerprint")!=fingerprint{return Err(AppError::Conflict("idempotency body mismatch".into()));}
        return Ok((StatusCode::OK,Json(json!({"order_id":col_i64(&row,"order_id"),"replayed":true}))));
    }
    let now = unix_now();
    e(
        &tx,
        "INSERT OR IGNORE INTO ecs_promotion_version(promotion_id) VALUES(?)",
        vec![json!(id)],
    )
    .await?;
    e(
        &tx,
        "UPDATE ecs_promotion_version SET version=version+1 WHERE promotion_id=?",
        vec![json!(id)],
    )
    .await?;
    let activity=q1(&tx,"SELECT a.*,g.goods_name AS goods_name FROM ecs_goods_activity a JOIN ecs_goods g ON g.goods_id=a.goods_id WHERE a.act_id=? AND a.act_type IN(0,1,2) AND g.is_on_sale=1 AND g.is_delete=0",vec![json!(id)]).await?.ok_or_else(||AppError::NotFound("promotion not found".into()))?;
    let config: Value = serde_json::from_str(&col_str(&activity, "ext_info"))
        .map_err(|_| AppError::InvalidState("activity ext_info must be JSON".into()))?;
    let kind = col_i64(&activity, "act_type");
    let promotion_type = match kind {
        0 => "auction",
        1 => "group_buy",
        _ => "snatch",
    };
    let unit = if kind == 1 {
        if col_i64(&activity, "is_finished") != 0
            || col_i64(&activity, "start_time") > now
            || col_i64(&activity, "end_time") <= now
        {
            return Err(AppError::Conflict("group buy inactive".into()));
        }
        parse_money_cents(
            config
                .get("cur_price")
                .and_then(Value::as_str)
                .unwrap_or(""),
            "cur_price",
        )?
    } else {
        if body.quantity != 1
            || (col_i64(&activity, "is_finished") == 0 && col_i64(&activity, "end_time") > now)
        {
            return Err(AppError::Conflict(
                "promotion not finished or quantity not one".into(),
            ));
        }
        if q1(
            &tx,
            "SELECT order_id FROM ecs_promotion_claim WHERE promotion_id=?",
            vec![json!(id)],
        )
        .await?
        .is_some()
        {
            return Err(AppError::Conflict("promotion already purchased".into()));
        }
        let winner = if kind == 0 {
            q1(&tx,"SELECT user_id,price_cents,deposit_cents FROM ecs_promotion_bid WHERE promotion_id=? ORDER BY bid_id DESC LIMIT 1",vec![json!(id)]).await?
        } else {
            q1(&tx,"SELECT user_id,price_cents,deposit_cents FROM ecs_promotion_bid WHERE promotion_id=? AND price_cents IN(SELECT price_cents FROM ecs_promotion_bid WHERE promotion_id=? GROUP BY price_cents HAVING COUNT(*)=1) ORDER BY price_cents LIMIT 1",vec![json!(id),json!(id)]).await?
        };
        let winner = winner.ok_or_else(|| AppError::Conflict("no winning bid".into()))?;
        if col_i64(&winner, "user_id") != auth.user_id {
            return Err(AppError::Forbidden);
        }
        let deposit = col_i64(&winner, "deposit_cents");
        if deposit > 0 {
            let r=e(&tx,"UPDATE ecs_account_balance SET frozen_cents=frozen_cents-?,available_cents=available_cents+? WHERE user_id=? AND frozen_cents>=?",vec![json!(deposit),json!(deposit),json!(auth.user_id),json!(deposit)]).await?;
            if r.rows_affected != 1 {
                return Err(AppError::Conflict("auction deposit missing".into()));
            }
            e(&tx,"INSERT INTO ecs_account_log(user_id,available_delta_cents,frozen_delta_cents,reason,reference_type,reference_id) VALUES(?,?,?,'auction_purchase_release','promotion',?)",vec![json!(auth.user_id),json!(deposit),json!(-deposit),json!(id)]).await?;
        }
        col_i64(&winner, "price_cents")
    };
    let goods = unit
        .checked_mul(body.quantity)
        .ok_or_else(|| AppError::Validation("amount overflow".into()))?;
    let address = q1(
        &tx,
        "SELECT * FROM ecs_user_address WHERE address_id=? AND user_id=?",
        vec![json!(body.address_id), json!(auth.user_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("address not found".into()))?;
    let shipping = q1(
        &tx,
        "SELECT shipping_fee AS fee FROM ecs_shipping WHERE shipping_id=? AND enabled=1",
        vec![json!(body.shipping_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("shipping not found".into()))?;
    let payment = q1(
        &tx,
        "SELECT pay_fee AS fee FROM ecs_payment WHERE pay_id=? AND enabled=1",
        vec![json!(body.payment_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("payment not found".into()))?;
    let total = goods
        .checked_add(col_cents(&shipping, "fee"))
        .and_then(|v| v.checked_add(col_cents(&payment, "fee")))
        .ok_or_else(|| AppError::Validation("amount overflow".into()))?;
    let stock=e(&tx,"UPDATE ecs_goods SET goods_number=goods_number-? WHERE goods_id=? AND goods_number>=? AND is_on_sale=1 AND is_delete=0",vec![json!(body.quantity),json!(col_i64(&activity,"goods_id")),json!(body.quantity)]).await?;
    if stock.rows_affected != 1 {
        return Err(AppError::Conflict("insufficient stock".into()));
    }
    let serial = format!("GB{}", uuid::Uuid::new_v4().simple());
    let r=e(&tx,"INSERT INTO ecs_order_info(order_sn,user_id,consignee,address,mobile,shipping_id,pay_id,goods_amount,shipping_fee,payment_fee,order_amount,idempotency_key,request_fingerprint,created_at) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",vec![json!(serial),json!(auth.user_id),json!(col_str(&address,"consignee")),json!(col_str(&address,"address")),json!(col_str(&address,"mobile")),json!(body.shipping_id),json!(body.payment_id),json!(cents_to_string(goods)),json!(cents_to_string(col_cents(&shipping,"fee"))),json!(cents_to_string(col_cents(&payment,"fee"))),json!(cents_to_string(total)),json!(key),json!(fingerprint),json!(now)]).await?;
    let order = insert_id(&r);
    if kind != 1 {
        e(
            &tx,
            "INSERT INTO ecs_promotion_claim(promotion_id,order_id,user_id) VALUES(?,?,?)",
            vec![json!(id), json!(order), json!(auth.user_id)],
        )
        .await?;
    }
    e(&tx,"INSERT INTO ecs_order_goods(order_id,goods_id,goods_name,goods_number,goods_price) VALUES(?,?,?,?,?)",vec![json!(order),json!(col_i64(&activity,"goods_id")),json!(col_str(&activity,"goods_name")),json!(body.quantity),json!(cents_to_string(unit))]).await?;
    e(
        &tx,
        "INSERT INTO ecs_order_promotion(order_id,promotion_id,promotion_type) VALUES(?,?,?)",
        vec![json!(order), json!(id), json!(promotion_type)],
    )
    .await?;
    e(&tx,"INSERT INTO ecs_order_action(order_id,actor_user_id,action,created_at) VALUES(?,?,'group_buy',?)",vec![json!(order),json!(auth.user_id),json!(now)]).await?;
    rust_ecshop_infra::db::commit(&tx).await?;
    Ok((
        StatusCode::CREATED,
        Json(
            json!({"order_id":order,"order_sn":serial,"order_amount":cents_to_string(total),"replayed":false}),
        ),
    ))
}
