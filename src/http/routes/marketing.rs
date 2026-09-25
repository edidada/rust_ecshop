//! Marketing context: activities, packages, promotions, topics, votes, exchange goods.
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::rbutil::{col_i64, col_str, col_cents, col_f64, e, q, q1};
use crate::shared::util::{cents_to_string, parse_i64_param, unix_now, PageParams};

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation("id must be a positive integer".to_string())),
    }
}

/// GET /api/v1/activities
pub async fn activities(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let now = unix_now();
    let items = q(
        state.rb,
        "SELECT act_id AS act_id, act_name AS act_name, start_time AS start_time, end_time AS end_time,
                user_rank AS user_rank, act_range AS act_range, act_range_ext AS act_range_ext,
                min_amount AS min_amount, max_amount AS max_amount, act_type AS act_type, act_type_ext AS act_type_ext, gift AS gift
         FROM ecs_favourable_activity
         WHERE end_time > ? ORDER BY sort_order, end_time",
        vec![json!(now)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "act_id"),
            "name": col_str(&r, "act_name"),
            "start_time": col_i64(&r, "start_time"),
            "end_time": col_i64(&r, "end_time"),
            "user_rank": col_str(&r, "user_rank"),
            "range": col_i64(&r, "act_range"),
            "range_ext": col_str(&r, "act_range_ext"),
            "min_amount": cents_to_string(col_cents(&r, "min_amount")),
            "max_amount": cents_to_string(col_cents(&r, "max_amount")),
            "type": col_i64(&r, "act_type"),
            "type_ext": cents_to_string(col_cents(&r, "act_type_ext")),
            "gift": col_str(&r, "gift"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

/// GET /api/v1/packages
pub async fn packages(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let now = unix_now();
    let rb = state.rb;
    let rows = q(
        rb,
        "SELECT act_id AS act_id, act_name AS act_name, act_desc AS act_desc, start_time AS start_time,
                end_time AS end_time, ext_info AS ext_info
         FROM ecs_goods_activity
         WHERE act_type = 4 AND is_finished = 0 AND start_time <= ? AND end_time > ?
         ORDER BY act_id",
        vec![json!(now), json!(now)],
    )
    .await?;
    let mut items = Vec::new();
    for row in rows {
        let act_id = col_i64(&row, "act_id");
        // ext_info keeps "package_price" as a JSON string; parse it leniently.
        let package_price_cents = serde_json::from_str::<Value>(&col_str(&row, "ext_info"))
            .ok()
            .and_then(|v| v.get("package_price").and_then(|p| p.as_str()).map(|s| s.to_string()))
            .and_then(|s| s.parse::<f64>().ok())
            .map(|f| (f * 100.0).round() as i64)
            .unwrap_or(0);
        let detail = q(
            rb,
            "SELECT pg.goods_id AS goods_id, g.goods_name AS goods_name, pg.goods_number AS goods_number, g.shop_price AS shop_price
             FROM ecs_package_goods pg JOIN ecs_goods g ON g.goods_id = pg.goods_id
             WHERE pg.package_id = ? AND g.is_on_sale = 1 AND g.is_delete = 0",
            vec![json!(act_id)],
        )
        .await?;
        let mut subtotal_cents = 0i64;
        let mut package_items = Vec::new();
        for d in detail {
            let price_cents = col_cents(&d, "shop_price");
            let number = col_i64(&d, "goods_number");
            subtotal_cents = subtotal_cents.saturating_add(price_cents.saturating_mul(number));
            package_items.push(json!({
                "goods_id": col_i64(&d, "goods_id"),
                "name": col_str(&d, "goods_name"),
                "quantity": number,
                "price": cents_to_string(price_cents),
            }));
        }
        let saving = (subtotal_cents - package_price_cents).max(0);
        items.push(json!({
            "id": act_id,
            "name": col_str(&row, "act_name"),
            "desc": col_str(&row, "act_desc"),
            "start_time": col_i64(&row, "start_time"),
            "end_time": col_i64(&row, "end_time"),
            "package_price": cents_to_string(package_price_cents),
            "subtotal": cents_to_string(subtotal_cents),
            "saving": cents_to_string(saving),
            "items": package_items,
        }));
    }
    Ok(Json(json!({"items": items})))
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct PromotionQuery {
    #[serde(rename = "type")]
    act_type: Option<String>,
}

/// GET /api/v1/promotions?type=0..4
pub async fn promotions(
    State(state): State<AppState>,
    Query(qq): Query<PromotionQuery>,
) -> Result<Json<Value>, AppError> {
    let act_type = parse_i64_param(qq.act_type.as_deref(), "type")?;
    if let Some(t) = act_type {
        if !(0..=4).contains(&t) {
            return Err(AppError::Validation("type must be between 0 and 4".to_string()));
        }
    }
    let now = unix_now();
    let (sql, args): (String, Vec<Value>) = match act_type {
        Some(t) => (
            "SELECT a.act_id AS act_id, a.act_name AS act_name, a.act_desc AS act_desc, a.act_type AS act_type,
                    a.goods_id AS goods_id, a.goods_name AS goods_name, a.start_time AS start_time, a.end_time AS end_time, a.ext_info AS ext_info
             FROM ecs_goods_activity a LEFT JOIN ecs_goods g ON g.goods_id = a.goods_id
             WHERE a.act_type = ? AND a.is_finished = 0 AND a.start_time <= ? AND a.end_time > ?
               AND COALESCE(g.is_on_sale, 1) = 1 AND COALESCE(g.is_delete, 0) = 0
             ORDER BY a.act_id"
                .to_string(),
            vec![json!(t), json!(now), json!(now)],
        ),
        None => (
            "SELECT a.act_id AS act_id, a.act_name AS act_name, a.act_desc AS act_desc, a.act_type AS act_type,
                    a.goods_id AS goods_id, a.goods_name AS goods_name, a.start_time AS start_time, a.end_time AS end_time, a.ext_info AS ext_info
             FROM ecs_goods_activity a LEFT JOIN ecs_goods g ON g.goods_id = a.goods_id
             WHERE a.is_finished = 0 AND a.start_time <= ? AND a.end_time > ?
               AND COALESCE(g.is_on_sale, 1) = 1 AND COALESCE(g.is_delete, 0) = 0
             ORDER BY a.act_id"
                .to_string(),
            vec![json!(now), json!(now)],
        ),
    };
    let items = q(state.rb, &sql, args)
        .await?
        .into_iter()
        .map(|r| {
            json!({
                "id": col_i64(&r, "act_id"),
                "name": col_str(&r, "act_name"),
                "desc": col_str(&r, "act_desc"),
                "type": col_i64(&r, "act_type"),
                "goods_id": col_i64(&r, "goods_id"),
                "goods_name": col_str(&r, "goods_name"),
                "start_time": col_i64(&r, "start_time"),
                "end_time": col_i64(&r, "end_time"),
                "ext_info": col_str(&r, "ext_info"),
            })
        })
        .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

/// GET /api/v1/promotions/{id}
pub async fn promotion_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let act_id = parse_id(&id)?;
    let now = unix_now();
    let row = q1(
        state.rb,
        "SELECT a.act_id AS act_id, a.act_name AS act_name, a.act_desc AS act_desc, a.act_type AS act_type,
                a.goods_id AS goods_id, a.goods_name AS goods_name, a.start_time AS start_time, a.end_time AS end_time,
                a.ext_info AS ext_info, COALESCE(g.is_on_sale,1) AS is_on_sale, COALESCE(g.is_delete,0) AS is_delete
         FROM ecs_goods_activity a LEFT JOIN ecs_goods g ON g.goods_id = a.goods_id
         WHERE a.act_id = ? AND a.is_finished = 0 AND a.start_time <= ? AND a.end_time > ?",
        vec![json!(act_id), json!(now), json!(now)],
    )
    .await?
    .filter(|r| col_i64(r, "is_on_sale") == 1 && col_i64(r, "is_delete") == 0)
    .ok_or_else(|| AppError::NotFound("promotion not found".to_string()))?;
    Ok(Json(json!({
        "id": col_i64(&row, "act_id"),
        "name": col_str(&row, "act_name"),
        "desc": col_str(&row, "act_desc"),
        "type": col_i64(&row, "act_type"),
        "goods_id": col_i64(&row, "goods_id"),
        "goods_name": col_str(&row, "goods_name"),
        "start_time": col_i64(&row, "start_time"),
        "end_time": col_i64(&row, "end_time"),
        "ext_info": col_str(&row, "ext_info"),
    })))
}

/// GET /api/v1/topics/{id}
pub async fn topic_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let topic_id = parse_id(&id)?;
    let now = unix_now();
    let rb = state.rb;
    let row = q1(
        rb,
        "SELECT topic_id AS topic_id, title AS title, intro AS intro, start_time AS start_time, end_time AS end_time,
                data AS data, topic_img AS topic_img, title_pic AS title_pic, keywords AS keywords, description AS description
         FROM ecs_topic WHERE topic_id = ? AND start_time <= ? AND end_time > ?",
        vec![json!(topic_id), json!(now), json!(now)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("topic not found".to_string()))?;
    let data_json: Value =
        serde_json::from_str(&col_str(&row, "data")).unwrap_or_else(|_| json!({}));
    let mut groups = Vec::new();
    if let Some(map) = data_json.as_object() {
        for (group_name, goods_ids) in map {
            let ids: Vec<i64> = goods_ids
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
                .unwrap_or_default();
            let mut visible_ids = Vec::new();
            for gid in ids {
                let placeholders = json!(gid);
                let ok = q1(
                    rb,
                    "SELECT COUNT(*) AS cnt FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
                    vec![placeholders],
                )
                .await?
                .map(|r| col_i64(&r, "cnt"))
                .unwrap_or(0);
                if ok > 0 {
                    visible_ids.push(gid);
                }
            }
            groups.push(json!({"name": group_name, "goods_ids": visible_ids}));
        }
    }
    Ok(Json(json!({
        "id": col_i64(&row, "topic_id"),
        "title": col_str(&row, "title"),
        "intro": col_str(&row, "intro"),
        "start_time": col_i64(&row, "start_time"),
        "end_time": col_i64(&row, "end_time"),
        "topic_img": col_str(&row, "topic_img"),
        "title_pic": col_str(&row, "title_pic"),
        "keywords": col_str(&row, "keywords"),
        "description": col_str(&row, "description"),
        "groups": groups,
    })))
}

/// GET /api/v1/votes/current
pub async fn votes_current(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let now = unix_now();
    let rb = state.rb;
    let vote = q1(
        rb,
        "SELECT vote_id AS vote_id, vote_name AS vote_name, start_time AS start_time, end_time AS end_time,
                can_multi AS can_multi, vote_count AS vote_count
         FROM ecs_vote WHERE start_time <= ? AND end_time > ? ORDER BY vote_id DESC LIMIT 1",
        vec![json!(now), json!(now)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("no active vote".to_string()))?;
    let vote_id = col_i64(&vote, "vote_id");
    let options = q(
        rb,
        "SELECT option_id AS option_id, option_name AS option_name, option_count AS option_count
         FROM ecs_vote_option WHERE vote_id = ? ORDER BY option_order, option_id",
        vec![json!(vote_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "option_id"),
            "name": col_str(&r, "option_name"),
            "count": col_i64(&r, "option_count"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({
        "id": vote_id,
        "name": col_str(&vote, "vote_name"),
        "start_time": col_i64(&vote, "start_time"),
        "end_time": col_i64(&vote, "end_time"),
        "can_multi": col_i64(&vote, "can_multi") != 0,
        "total_count": col_i64(&vote, "vote_count"),
        "options": options,
    })))
}

#[derive(Deserialize)]
pub struct VoteResponseRequest {
    option_ids: Vec<i64>,
}

/// POST /api/v1/votes/{id}/responses — client IP from socket, unique (vote, ip), single transaction.
pub async fn vote_respond(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ConnectInfo(addr): ConnectInfo<std::net::SocketAddr>,
    Json(body): Json<VoteResponseRequest>,
) -> Result<Json<Value>, AppError> {
    let vote_id = parse_id(&id)?;
    let now = unix_now();
    if body.option_ids.is_empty() {
        return Err(AppError::Validation("option_ids must not be empty".to_string()));
    }
    let unique: std::collections::BTreeSet<i64> = body.option_ids.iter().copied().collect();
    if unique.len() != body.option_ids.len() {
        return Err(AppError::Validation("option_ids must be unique".to_string()));
    }
    if unique.len() > 20 {
        return Err(AppError::Validation("option_ids must have at most 20 items".to_string()));
    }
    let rb = state.rb;
    let ip = addr.ip().to_string();
    let option_ids: Vec<i64> = unique.into_iter().collect();
    let tx = crate::infrastructure::db::begin(rb).await?;
    let vote = q1(
        &tx,
        "SELECT can_multi AS can_multi, (start_time <= ? AND end_time > ?) AS in_window
         FROM ecs_vote WHERE vote_id = ?",
        vec![json!(now), json!(now), json!(vote_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("vote not found".to_string()))?;
    if col_i64(&vote, "in_window") == 0 {
        return Err(AppError::NotFound("vote not active".to_string()));
    }
    if col_i64(&vote, "can_multi") == 0 && option_ids.len() > 1 {
        return Err(AppError::Validation("this vote accepts a single option".to_string()));
    }
    for oid in &option_ids {
        let ok = q1(
            &tx,
            "SELECT COUNT(*) AS cnt FROM ecs_vote_option WHERE option_id = ? AND vote_id = ?",
            vec![json!(oid), json!(vote_id)],
        )
        .await?
        .map(|r| col_i64(&r, "cnt"))
        .unwrap_or(0);
        if ok == 0 {
            return Err(AppError::Validation(format!("option {oid} does not belong to vote {vote_id}")));
        }
    }
    let dup = q1(
        &tx,
        "SELECT COUNT(*) AS cnt FROM ecs_vote_log WHERE vote_id = ? AND ip_address = ?",
        vec![json!(vote_id), json!(ip)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if dup > 0 {
        return Err(AppError::Conflict("this client already voted".to_string()));
    }
    e(
        &tx,
        "INSERT INTO ecs_vote_log (vote_id, ip_address, vote_time) VALUES (?, ?, ?)",
        vec![json!(vote_id), json!(ip), json!(now)],
    )
    .await?;
    for oid in &option_ids {
        e(
            &tx,
            "UPDATE ecs_vote_option SET option_count = option_count + 1 WHERE option_id = ?",
            vec![json!(oid)],
        )
        .await?;
    }
    e(
        &tx,
        "UPDATE ecs_vote SET vote_count = vote_count + 1 WHERE vote_id = ?",
        vec![json!(vote_id)],
    )
    .await?;
    tx.commit().await?;
    Ok(Json(json!({"status": "accepted"})))
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct ExchangeQuery {
    category_id: Option<i64>,
    integral_min: Option<i64>,
    integral_max: Option<i64>,
    page: Option<i64>,
    page_size: Option<i64>,
    sort: Option<String>,
    order: Option<String>,
}

/// GET /api/v1/exchange-goods
pub async fn exchange_goods(
    State(state): State<AppState>,
    Query(qq): Query<ExchangeQuery>,
) -> Result<Json<Value>, AppError> {
    let page = PageParams { page: qq.page, page_size: qq.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let order_sql = match (qq.sort.as_deref(), qq.order.as_deref()) {
        (Some("exchange_integral"), Some("asc")) => "ORDER BY e.exchange_integral ASC",
        (Some("exchange_integral"), Some("desc")) => "ORDER BY e.exchange_integral DESC",
        (Some("goods_id"), Some("asc")) => "ORDER BY g.goods_id ASC",
        (Some("goods_id"), Some("desc")) => "ORDER BY g.goods_id DESC",
        (Some("last_update"), Some("asc")) => "ORDER BY g.last_update ASC",
        (Some("last_update"), Some("desc")) => "ORDER BY g.last_update DESC",
        (Some(_), _) => return Err(AppError::Validation("invalid sort field".to_string())),
        _ => "ORDER BY e.exchange_integral ASC",
    };
    let rb = state.rb;
    let mut where_clause = "WHERE e.is_exchange = 1 AND g.is_on_sale = 1 AND g.is_delete = 0".to_string();
    let mut args: Vec<Value> = Vec::new();
    if let Some(cid) = qq.category_id {
        where_clause.push_str(" AND g.cat_id = ?");
        args.push(json!(cid));
    }
    if let Some(min) = qq.integral_min {
        where_clause.push_str(" AND e.exchange_integral >= ?");
        args.push(json!(min));
    }
    if let Some(max) = qq.integral_max {
        where_clause.push_str(" AND e.exchange_integral <= ?");
        args.push(json!(max));
    }
    let total = q1(
        rb,
        &format!(
            "SELECT COUNT(*) AS cnt FROM ecs_exchange_goods e JOIN ecs_goods g ON g.goods_id = e.goods_id {where_clause}"
        ),
        args.clone(),
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    args.push(json!(page_size));
    args.push(json!(offset));
    let items = q(
        rb,
        &format!(
            "SELECT g.goods_id AS goods_id, g.goods_name AS goods_name, g.shop_price AS shop_price, g.goods_img AS goods_img,
                    e.exchange_integral AS exchange_integral, e.is_hot AS is_hot
             FROM ecs_exchange_goods e JOIN ecs_goods g ON g.goods_id = e.goods_id
             {where_clause} {order_sql} LIMIT ? OFFSET ?"
        ),
        args,
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "goods_id": col_i64(&r, "goods_id"),
            "name": col_str(&r, "goods_name"),
            "price": cents_to_string(col_cents(&r, "shop_price")),
            "img": col_str(&r, "goods_img"),
            "exchange_integral": col_i64(&r, "exchange_integral"),
            "is_hot": col_i64(&r, "is_hot") != 0,
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"page": page_no, "page_size": page_size, "total": total, "items": items})))
}

/// GET /api/v1/exchange-goods/{id}
pub async fn exchange_goods_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let goods_id = parse_id(&id)?;
    let row = q1(
        state.rb,
        "SELECT g.goods_id AS goods_id, g.goods_name AS goods_name, g.shop_price AS shop_price, g.goods_img AS goods_img,
                g.goods_brief AS goods_brief, e.exchange_integral AS exchange_integral, e.is_hot AS is_hot
         FROM ecs_exchange_goods e JOIN ecs_goods g ON g.goods_id = e.goods_id
         WHERE e.goods_id = ? AND e.is_exchange = 1 AND g.is_on_sale = 1 AND g.is_delete = 0",
        vec![json!(goods_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("exchange goods not found".to_string()))?;
    Ok(Json(json!({
        "goods_id": col_i64(&row, "goods_id"),
        "name": col_str(&row, "goods_name"),
        "price": cents_to_string(col_cents(&row, "shop_price")),
        "img": col_str(&row, "goods_img"),
        "brief": col_str(&row, "goods_brief"),
        "exchange_integral": col_i64(&row, "exchange_integral"),
        "is_hot": col_i64(&row, "is_hot") != 0,
    })))
}

#[allow(dead_code)]
fn _keep(row: &Value) -> f64 {
    col_f64(row, "x")
}
