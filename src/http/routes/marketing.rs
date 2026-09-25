//! Marketing context: activities, packages, promotions, topics, votes, exchange goods.
use axum::extract::{ConnectInfo, Path, Query, State};
use axum::Json;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::util::{cents_to_string, parse_i64_param, unix_now, PageParams};

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation("id must be a positive integer".to_string())),
    }
}

/// GET /api/v1/activities
pub async fn activities(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT act_id, act_name, start_time, end_time, user_rank, act_range, act_range_ext,
                        min_amount, max_amount, act_type, act_type_ext, gift
                 FROM ecs_favourable_activity
                 WHERE end_time > ?1 ORDER BY sort_order, end_time",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([now], |r| {
                let min_amount: f64 = r.get(7)?;
                let max_amount: f64 = r.get(8)?;
                let act_type_ext: f64 = r.get(10)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "start_time": r.get::<_, i64>(2)?,
                    "end_time": r.get::<_, i64>(3)?,
                    "user_rank": r.get::<_, String>(4)?,
                    "range": r.get::<_, i64>(5)?,
                    "range_ext": r.get::<_, String>(6)?,
                    "min_amount": cents_to_string((min_amount * 100.0).round() as i64),
                    "max_amount": cents_to_string((max_amount * 100.0).round() as i64),
                    "type": r.get::<_, i64>(9)?,
                    "type_ext": cents_to_string((act_type_ext * 100.0).round() as i64),
                    "gift": r.get::<_, String>(11)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/packages
pub async fn packages(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT act_id, act_name, act_desc, goods_id, goods_name, start_time, end_time, ext_info
                 FROM ecs_goods_activity
                 WHERE act_type = 4 AND is_finished = 0 AND start_time <= ?1 AND end_time > ?2
                 ORDER BY act_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([now, now], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, String>(7)?,
                ))
            })
            .map_err(db_err)?;
        let rows: Vec<(i64, String, String, i64, String, i64, i64, String)> =
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        let mut items = Vec::new();
        for (act_id, name, desc, _goods_id, _goods_name, start, end, ext_info) in rows {
            let package_price: f64 = serde_json::from_str::<Value>(&ext_info)
                .ok()
                .and_then(|v| v.get("package_price").and_then(|p| p.as_str()).map(|s| s.to_string()))
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0);
            let package_price_cents = (package_price * 100.0).round() as i64;
            // Items with current goods prices and quantities.
            let detail: Vec<(i64, String, i64, f64)> = {
                let mut pstmt = conn
                    .prepare(
                        "SELECT pg.goods_id, g.goods_name, pg.goods_number, g.shop_price
                         FROM ecs_package_goods pg JOIN ecs_goods g ON g.goods_id = pg.goods_id
                         WHERE pg.package_id = ?1 AND g.is_on_sale = 1 AND g.is_delete = 0",
                    )
                    .map_err(db_err)?;
                let prows = pstmt
                    .query_map([act_id], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, f64>(3)?,
                        ))
                    })
                    .map_err(db_err)?;
                prows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
            };
            let mut subtotal_cents = 0i64;
            let mut package_items = Vec::new();
            for (goods_id, goods_name, number, shop_price) in detail {
                let price_cents = (shop_price * 100.0).round() as i64;
                subtotal_cents = subtotal_cents.saturating_add(price_cents.saturating_mul(number));
                package_items.push(json!({
                    "goods_id": goods_id,
                    "name": goods_name,
                    "quantity": number,
                    "price": cents_to_string(price_cents),
                }));
            }
            let saving = (subtotal_cents - package_price_cents).max(0);
            items.push(json!({
                "id": act_id,
                "name": name,
                "desc": desc,
                "start_time": start,
                "end_time": end,
                "package_price": cents_to_string(package_price_cents),
                "subtotal": cents_to_string(subtotal_cents),
                "saving": cents_to_string(saving),
                "items": package_items,
            }));
        }
        Ok(json!({"items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct PromotionQuery {
    #[serde(rename = "type")]
    act_type: Option<String>,
}

/// GET /api/v1/promotions?type=0..4 and /api/v1/promotions/{id}
pub async fn promotions(
    State(state): State<AppState>,
    Query(q): Query<PromotionQuery>,
) -> Result<Json<Value>, AppError> {
    let act_type = parse_i64_param(q.act_type.as_deref(), "type")?;
    if let Some(t) = act_type {
        if !(0..=4).contains(&t) {
            return Err(AppError::Validation("type must be between 0 and 4".to_string()));
        }
    }
    let db = state.db.clone();
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let (sql, params): (String, Vec<Box<dyn rusqlite::ToSql>>) = match act_type {
            Some(t) => (
                "SELECT a.act_id, a.act_name, a.act_desc, a.act_type, a.goods_id, a.goods_name, a.start_time, a.end_time, a.ext_info, g.is_on_sale, g.is_delete
                 FROM ecs_goods_activity a LEFT JOIN ecs_goods g ON g.goods_id = a.goods_id
                 WHERE a.act_type = ?1 AND a.is_finished = 0 AND a.start_time <= ?2 AND a.end_time > ?3
                   AND COALESCE(g.is_on_sale, 1) = 1 AND COALESCE(g.is_delete, 0) = 0
                 ORDER BY a.act_id"
                    .to_string(),
                vec![Box::new(t), Box::new(now), Box::new(now)],
            ),
            None => (
                "SELECT a.act_id, a.act_name, a.act_desc, a.act_type, a.goods_id, a.goods_name, a.start_time, a.end_time, a.ext_info, g.is_on_sale, g.is_delete
                 FROM ecs_goods_activity a LEFT JOIN ecs_goods g ON g.goods_id = a.goods_id
                 WHERE a.is_finished = 0 AND a.start_time <= ?1 AND a.end_time > ?2
                   AND COALESCE(g.is_on_sale, 1) = 1 AND COALESCE(g.is_delete, 0) = 0
                 ORDER BY a.act_id"
                    .to_string(),
                vec![Box::new(now), Box::new(now)],
            ),
        };
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "desc": r.get::<_, String>(2)?,
                    "type": r.get::<_, i64>(3)?,
                    "goods_id": r.get::<_, i64>(4)?,
                    "goods_name": r.get::<_, String>(5)?,
                    "start_time": r.get::<_, i64>(6)?,
                    "end_time": r.get::<_, i64>(7)?,
                    "ext_info": r.get::<_, String>(8)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

pub async fn promotion_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let act_id = parse_id(&id)?;
    let db = state.db.clone();
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let row = conn
            .query_row(
                "SELECT a.act_id, a.act_name, a.act_desc, a.act_type, a.goods_id, a.goods_name, a.start_time, a.end_time, a.ext_info, COALESCE(g.is_on_sale,1), COALESCE(g.is_delete,0)
                 FROM ecs_goods_activity a LEFT JOIN ecs_goods g ON g.goods_id = a.goods_id
                 WHERE a.act_id = ?1 AND a.is_finished = 0 AND a.start_time <= ?2 AND a.end_time > ?3",
                [act_id, now, now],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "desc": r.get::<_, String>(2)?,
                        "type": r.get::<_, i64>(3)?,
                        "goods_id": r.get::<_, i64>(4)?,
                        "goods_name": r.get::<_, String>(5)?,
                        "start_time": r.get::<_, i64>(6)?,
                        "end_time": r.get::<_, i64>(7)?,
                        "ext_info": r.get::<_, String>(8)?,
                    }))
                },
            )
            .optional()
            .map_err(db_err)?;
        // Visibility filter on the joined goods.
        let visible: i64 = conn
            .query_row(
                "SELECT COALESCE(g.is_on_sale,1) * (1 - COALESCE(g.is_delete,0))
                 FROM ecs_goods_activity a LEFT JOIN ecs_goods g ON g.goods_id = a.goods_id WHERE a.act_id = ?1",
                [act_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?
            .unwrap_or(0);
        match (row, visible) {
            (Some(v), 1) => Ok(v),
            _ => Err(AppError::NotFound("promotion not found".to_string())),
        }
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/topics/{id}
pub async fn topic_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let topic_id = parse_id(&id)?;
    let db = state.db.clone();
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let row = conn
            .query_row(
                "SELECT topic_id, title, intro, start_time, end_time, data, topic_img, title_pic, keywords, description
                 FROM ecs_topic WHERE topic_id = ?1 AND start_time <= ?2 AND end_time > ?3",
                [topic_id, now, now],
                |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, i64>(3)?,
                        r.get::<_, i64>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, Option<String>>(9)?,
                    ))
                },
            )
            .optional()
            .map_err(db_err)?;
        let Some((tid, title, intro, start, end, data, topic_img, title_pic, keywords, description)) = row
        else {
            return Err(AppError::NotFound("topic not found".to_string()));
        };
        // Groups follow the JSON object order of topic.data, filtered to sellable goods.
        let data_json: Value =
            serde_json::from_str(&data).unwrap_or_else(|_| json!({}));
        let mut groups = Vec::new();
        if let Some(map) = data_json.as_object() {
            for (group_name, goods_ids) in map {
                let ids: Vec<i64> = goods_ids
                    .as_array()
                    .map(|a| a.iter().filter_map(|v| v.as_i64()).collect())
                    .unwrap_or_default();
                let mut visible_ids = Vec::new();
                for gid in ids {
                    let ok: i64 = conn
                        .query_row(
                            "SELECT COUNT(*) FROM ecs_goods WHERE goods_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                            [gid],
                            |r| r.get(0),
                        )
                        .map_err(db_err)?;
                    if ok > 0 {
                        visible_ids.push(gid);
                    }
                }
                groups.push(json!({"name": group_name, "goods_ids": visible_ids}));
            }
        }
        Ok(json!({
            "id": tid,
            "title": title,
            "intro": intro,
            "start_time": start,
            "end_time": end,
            "topic_img": topic_img,
            "title_pic": title_pic,
            "keywords": keywords,
            "description": description,
            "groups": groups,
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/votes/current
pub async fn votes_current(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let row = conn
            .query_row(
                "SELECT vote_id, vote_name, start_time, end_time, can_multi, vote_count
                 FROM ecs_vote WHERE start_time <= ?1 AND end_time > ?2 ORDER BY vote_id DESC LIMIT 1",
                [now, now],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "start_time": r.get::<_, i64>(2)?,
                        "end_time": r.get::<_, i64>(3)?,
                        "can_multi": r.get::<_, i64>(4)? != 0,
                        "total_count": r.get::<_, i64>(5)?,
                    }))
                },
            )
            .optional()
            .map_err(db_err)?;
        let Some(mut vote) = row else {
            return Err(AppError::NotFound("no active vote".to_string()));
        };
        let vote_id = vote["id"].as_i64().unwrap_or(0);
        let mut stmt = conn
            .prepare("SELECT option_id, option_name, option_count FROM ecs_vote_option WHERE vote_id = ?1 ORDER BY option_order, option_id")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([vote_id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "count": r.get::<_, i64>(2)?,
                }))
            })
            .map_err(db_err)?;
        let options: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        vote["options"] = Value::Array(options);
        Ok(vote)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct VoteResponseRequest {
    option_ids: Vec<i64>,
}

/// POST /api/v1/votes/{id}/responses — client IP from socket, unique (vote, ip).
pub async fn vote_respond(
    State(state): State<AppState>,
    Path(id): Path<String>,
    ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
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
    let db = state.db.clone();
    let ip = addr.ip().to_string();
    let option_ids: Vec<i64> = unique.into_iter().collect();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let (can_multi, in_window): (i64, i64) = tx
            .query_row(
                "SELECT can_multi, (start_time <= ?2 AND end_time > ?3) FROM ecs_vote WHERE vote_id = ?1",
                [vote_id, now, now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| AppError::NotFound("vote not found".to_string()))?;
        if in_window == 0 {
            return Err(AppError::NotFound("vote not active".to_string()));
        }
        if can_multi == 0 && option_ids.len() > 1 {
            return Err(AppError::Validation("this vote accepts a single option".to_string()));
        }
        // Validate options belong to the vote.
        for oid in &option_ids {
            let ok: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM ecs_vote_option WHERE option_id = ?1 AND vote_id = ?2",
                    [oid, &vote_id],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            if ok == 0 {
                return Err(AppError::Validation(format!("option {oid} does not belong to vote {vote_id}")));
            }
        }
        // Duplicate IP check inside the transaction.
        let exists: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM ecs_vote_log WHERE vote_id = ?1 AND ip_address = ?2",
                rusqlite::params![vote_id, ip],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if exists > 0 {
            return Err(AppError::Conflict("this client already voted".to_string()));
        }
        tx.execute(
            "INSERT INTO ecs_vote_log (vote_id, ip_address, vote_time) VALUES (?1, ?2, ?3)",
            rusqlite::params![vote_id, ip, now],
        )
        .map_err(db_err)?;
        for oid in &option_ids {
            tx.execute(
                "UPDATE ecs_vote_option SET option_count = option_count + 1 WHERE option_id = ?1",
                [oid],
            )
            .map_err(db_err)?;
        }
        tx.execute(
            "UPDATE ecs_vote SET vote_count = vote_count + 1 WHERE vote_id = ?1",
            [vote_id],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(json!({"status": "accepted"}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
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
    Query(q): Query<ExchangeQuery>,
) -> Result<Json<Value>, AppError> {
    let page = PageParams { page: q.page, page_size: q.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let sort = q.sort.clone().unwrap_or_default();
    let order = q.order.clone().unwrap_or_default();
    let order_sql = match (sort.as_str(), order.as_str()) {
        ("exchange_integral", "asc") => "ORDER BY e.exchange_integral ASC",
        ("exchange_integral", "desc") => "ORDER BY e.exchange_integral DESC",
        ("goods_id", "asc") => "ORDER BY g.goods_id ASC",
        ("goods_id", "desc") => "ORDER BY g.goods_id DESC",
        ("last_update", "asc") => "ORDER BY g.last_update ASC",
        ("last_update", "desc") => "ORDER BY g.last_update DESC",
        (_, "asc") | (_, "desc") => {
            return Err(AppError::Validation("invalid sort field".to_string()))
        }
        _ => "ORDER BY e.exchange_integral ASC",
    };
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut where_clause =
            "WHERE e.is_exchange = 1 AND g.is_on_sale = 1 AND g.is_delete = 0".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(cid) = q.category_id {
            where_clause.push_str(" AND g.cat_id = ?");
            params.push(Box::new(cid));
        }
        if let Some(min) = q.integral_min {
            where_clause.push_str(" AND e.exchange_integral >= ?");
            params.push(Box::new(min));
        }
        if let Some(max) = q.integral_max {
            where_clause.push_str(" AND e.exchange_integral <= ?");
            params.push(Box::new(max));
        }
        let total: i64 = {
            let sql = format!(
                "SELECT COUNT(*) FROM ecs_exchange_goods e JOIN ecs_goods g ON g.goods_id = e.goods_id {where_clause}"
            );
            let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
            conn.query_row(&sql, params_ref.as_slice(), |r| r.get(0))
                .map_err(db_err)?
        };
        let sql = format!(
            "SELECT g.goods_id, g.goods_name, g.shop_price, g.goods_img, e.exchange_integral, e.is_hot
             FROM ecs_exchange_goods e JOIN ecs_goods g ON g.goods_id = e.goods_id
             {where_clause} {order_sql} LIMIT ? OFFSET ?"
        );
        params.push(Box::new(page_size));
        params.push(Box::new(offset));
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                let price: f64 = r.get(2)?;
                Ok(json!({
                    "goods_id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "price": cents_to_string((price * 100.0).round() as i64),
                    "img": r.get::<_, String>(3)?,
                    "exchange_integral": r.get::<_, i64>(4)?,
                    "is_hot": r.get::<_, i64>(5)? != 0,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"page": page_no, "page_size": page_size, "total": total, "items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/exchange-goods/{id}
pub async fn exchange_goods_detail(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    let goods_id = parse_id(&id)?;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let row = conn
            .query_row(
                "SELECT g.goods_id, g.goods_name, g.shop_price, g.goods_img, g.goods_brief, e.exchange_integral, e.is_hot
                 FROM ecs_exchange_goods e JOIN ecs_goods g ON g.goods_id = e.goods_id
                 WHERE e.goods_id = ?1 AND e.is_exchange = 1 AND g.is_on_sale = 1 AND g.is_delete = 0",
                [goods_id],
                |r| {
                    let price: f64 = r.get(2)?;
                    Ok(json!({
                        "goods_id": r.get::<_, i64>(0)?,
                        "name": r.get::<_, String>(1)?,
                        "price": cents_to_string((price * 100.0).round() as i64),
                        "img": r.get::<_, String>(3)?,
                        "brief": r.get::<_, String>(4)?,
                        "exchange_integral": r.get::<_, i64>(5)?,
                        "is_hot": r.get::<_, i64>(6)? != 0,
                    }))
                },
            )
            .optional()
            .map_err(db_err)?;
        row.ok_or_else(|| AppError::NotFound("exchange goods not found".to_string()))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}
