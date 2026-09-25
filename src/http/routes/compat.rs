//! Compatibility and supplementary URLs: affiliate, shipments, points, browsing history,
//! personal messages (pm), pick-out, wholesale, captcha, api.php compat, certi.
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::state::AppState;
use crate::infrastructure::crypto;
use crate::shared::error::AppError;
use crate::shared::util::{cents_to_string, parse_money_cents, unix_now, PageParams};

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation("id must be a positive integer".to_string())),
    }
}

fn mask_order_sn(sn: &str) -> String {
    let chars: Vec<char> = sn.chars().collect();
    if chars.len() <= 4 {
        return "*".repeat(chars.len());
    }
    format!("{}***{}", &sn[..2], &sn[sn.len() - 2..])
}

// ---------------------------------------------------------------------------
// GET /api/v1/me/affiliate
// ---------------------------------------------------------------------------

/// GET /api/v1/me/affiliate?page=&page_size= — referral levels, downline orders, commission logs.
pub async fn affiliate(
    State(state): State<AppState>,
    auth: AuthUser,
    Query(page): Query<PageParams>,
) -> Result<Json<Value>, AppError> {
    let (page_no, page_size, offset) = page.resolve();
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        // Recursive CTE over ecs_user_referral, max 10 levels.
        let levels: Vec<Value> = {
            let mut stmt = conn
                .prepare(
                    "WITH RECURSIVE downline(user_id, level) AS (
                        SELECT ?1, 0
                        UNION ALL
                        SELECT ur.user_id, d.level + 1
                        FROM ecs_user_referral ur JOIN downline d ON ur.referrer_user_id = d.user_id
                        WHERE d.level < 10
                     )
                     SELECT level - 1 AS depth, COUNT(*) AS cnt FROM downline WHERE level > 0 GROUP BY depth ORDER BY depth",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([user_id], |r| {
                    Ok(json!({
                        "level": r.get::<_, i64>(0)?,
                        "count": r.get::<_, i64>(1)?,
                    }))
                })
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        // Downline orders masked; separated flag from affiliate_log existence.
        let orders: Vec<Value> = {
            let mut stmt = conn
                .prepare(
                    "WITH RECURSIVE downline(user_id, level) AS (
                        SELECT ?1, 0
                        UNION ALL
                        SELECT ur.user_id, d.level + 1
                        FROM ecs_user_referral ur JOIN downline d ON ur.referrer_user_id = d.user_id
                        WHERE d.level < 10
                     )
                     SELECT o.order_sn, u.user_name, o.order_amount, o.order_id, o.created_at,
                            (SELECT COUNT(*) FROM ecs_affiliate_log al WHERE al.order_id = o.order_id AND al.user_id = ?1) AS separated
                     FROM ecs_order_info o
                     JOIN ecs_users u ON u.user_id = o.user_id
                     WHERE o.user_id IN (SELECT user_id FROM downline WHERE user_id <> ?1)
                     ORDER BY o.order_id DESC LIMIT ?2 OFFSET ?3",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map(rusqlite::params![user_id, page_size, offset], |r| {
                    let amount: f64 = r.get(2)?;
                    Ok(json!({
                        "order_sn": mask_order_sn(&r.get::<_, String>(0)?),
                        "user_name": r.get::<_, String>(1)?,
                        "order_amount": cents_to_string((amount * 100.0).round() as i64),
                        "order_id": r.get::<_, i64>(3)?,
                        "created_at": r.get::<_, i64>(4)?,
                        "separated": r.get::<_, i64>(5)? > 0,
                    }))
                })
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        let own_logs: Vec<Value> = {
            let mut stmt = conn
                .prepare(
                    "SELECT log_id, order_id, money, point, separate_type, created_at
                     FROM ecs_affiliate_log WHERE user_id = ?1 ORDER BY log_id DESC",
                )
                .map_err(db_err)?;
            let rows = stmt
                .query_map([user_id], |r| {
                    let money: f64 = r.get(2)?;
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "order_id": r.get::<_, i64>(1)?,
                        "commission": cents_to_string((money * 100.0).round() as i64),
                        "points": r.get::<_, i64>(3)?,
                        "separate_type": r.get::<_, i64>(4)?,
                        "created_at": r.get::<_, i64>(5)?,
                    }))
                })
                .map_err(db_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?
        };
        Ok(json!({
            "page": page_no,
            "page_size": page_size,
            "levels": levels,
            "orders": orders,
            "logs": own_logs,
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// GET /api/v1/me/shipments (user.php?act=track_packages)
// ---------------------------------------------------------------------------

/// GET /api/v1/me/shipments — paid/shipped/received orders with shipping info.
pub async fn shipments(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT o.order_id, o.order_sn, o.order_status, s.shipping_name, o.created_at
                 FROM ecs_order_info o JOIN ecs_shipping s ON s.shipping_id = o.shipping_id
                 WHERE o.user_id = ?1 AND o.order_status IN ('paid', 'shipped', 'received')
                 ORDER BY o.order_id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                Ok(json!({
                    "order_id": r.get::<_, i64>(0)?,
                    "order_sn": r.get::<_, String>(1)?,
                    "status": r.get::<_, String>(2)?,
                    "shipping_name": r.get::<_, String>(3)?,
                    "created_at": r.get::<_, i64>(4)?,
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

// ---------------------------------------------------------------------------
// Points conversion (user.php?act=transform_points)
// ---------------------------------------------------------------------------

/// GET /api/v1/me/points/conversion-options
pub async fn points_options(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let points: i64 = conn
            .query_row(
                "SELECT COALESCE(pay_points, 0) FROM ecs_users WHERE user_id = ?1",
                [user_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?
            .unwrap_or(0);
        // Fixed development rate: 100 points = 1.00 CNY.
        Ok(json!({
            "pay_points": points,
            "points_per_yuan": 100,
            "min_points": 100,
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct PointsConversionRequest {
    points: i64,
}

/// POST /api/v1/me/points/conversions
pub async fn points_convert(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<PointsConversionRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if body.points < 100 {
        return Err(AppError::Validation("points must be at least 100".to_string()));
    }
    let db = state.db.clone();
    let user_id = auth.user_id;
    let now = unix_now();
    let points = body.points;
    let cents = points; // 100 points = 1.00 CNY => 1 point = 1 cent.
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let updated = tx
            .execute(
                "UPDATE ecs_users SET pay_points = pay_points - ?1 WHERE user_id = ?2 AND pay_points >= ?1",
                rusqlite::params![points, user_id],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(AppError::Conflict("insufficient points".to_string()));
        }
        tx.execute(
            "INSERT INTO ecs_account_balance (user_id, available_cents, frozen_cents) VALUES (?1, ?2, 0)
             ON CONFLICT(user_id) DO UPDATE SET available_cents = available_cents + ?2",
            rusqlite::params![user_id, cents],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT INTO ecs_account_log (user_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at)
             VALUES (?1, ?2, 0, 'points_conversion', 'users', ?1, ?3)",
            rusqlite::params![user_id, cents, now],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(json!({
            "converted_points": points,
            "balance": cents_to_string(cents),
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/me/browsing-history
// ---------------------------------------------------------------------------

/// DELETE /api/v1/me/browsing-history
pub async fn browsing_history_clear(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<StatusCode, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        conn.execute("DELETE FROM ecs_browsing_history WHERE user_id = ?1", [user_id])
            .map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Personal messages (pm.php)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PmCreateRequest {
    to_user_id: i64,
    title: String,
    content: String,
}

/// POST /api/v1/me/pms
pub async fn pm_create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<PmCreateRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if body.title.is_empty() || body.content.is_empty() {
        return Err(AppError::Validation("title and content are required".to_string()));
    }
    let db = state.db.clone();
    let from_user_id = auth.user_id;
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ecs_users WHERE user_id = ?1",
                [body.to_user_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if exists == 0 {
            return Err(AppError::NotFound("recipient not found".to_string()));
        }
        conn.execute(
            "INSERT INTO ecs_pm (user_id, from_user_id, title, content, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![body.to_user_id, from_user_id, body.title, body.content, now],
        )
        .map_err(db_err)?;
        Ok(json!({"id": conn.last_insert_rowid()}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

/// GET /api/v1/me/pms
pub async fn pm_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT pm_id, from_user_id, u.user_name, title, content, is_read, created_at
                 FROM ecs_pm p JOIN ecs_users u ON u.user_id = p.from_user_id
                 WHERE p.user_id = ?1 ORDER BY p.pm_id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "from_user_id": r.get::<_, i64>(1)?,
                    "from_user_name": r.get::<_, String>(2)?,
                    "title": r.get::<_, String>(3)?,
                    "content": r.get::<_, String>(4)?,
                    "is_read": r.get::<_, i64>(5)? != 0,
                    "created_at": r.get::<_, i64>(6)?,
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

/// DELETE /api/v1/me/pms/{id}
pub async fn pm_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let pm_id = parse_id(&id)?;
    let user_id = auth.user_id;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        let deleted = conn
            .execute(
                "DELETE FROM ecs_pm WHERE pm_id = ?1 AND user_id = ?2",
                [pm_id, user_id],
            )
            .map_err(db_err)?;
        if deleted == 0 {
            return Err(AppError::NotFound("message not found".to_string()));
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Pick-out (pick_out.php)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct PickOutQuery {
    cat_ids: Option<String>,
    min_price: Option<String>,
    max_price: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

/// GET /api/v1/pick-out?cat_ids=1,2&min_price=&max_price=
pub async fn pick_out(
    State(state): State<AppState>,
    Query(q): Query<PickOutQuery>,
) -> Result<Json<Value>, AppError> {
    let page = PageParams { page: q.page, page_size: q.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let cat_ids: Vec<i64> = match &q.cat_ids {
        Some(s) if !s.is_empty() => s
            .split(',')
            .map(|x| x.trim().parse::<i64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| AppError::Validation("cat_ids must be integers".to_string()))?,
        _ => Vec::new(),
    };
    let min_cents = match &q.min_price {
        Some(s) if !s.is_empty() => Some(parse_money_cents(s, "min_price")?),
        _ => None,
    };
    let max_cents = match &q.max_price {
        Some(s) if !s.is_empty() => Some(parse_money_cents(s, "max_price")?),
        _ => None,
    };
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut where_clause = "WHERE is_on_sale = 1 AND is_delete = 0".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if !cat_ids.is_empty() {
            let placeholders = cat_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            where_clause.push_str(&format!(" AND cat_id IN ({placeholders})"));
            for c in &cat_ids {
                params.push(Box::new(*c));
            }
        }
        if let Some(min) = min_cents {
            where_clause.push_str(" AND shop_price >= ?");
            params.push(Box::new(min as f64 / 100.0));
        }
        if let Some(max) = max_cents {
            where_clause.push_str(" AND shop_price <= ?");
            params.push(Box::new(max as f64 / 100.0));
        }
        let total: i64 = {
            let sql = format!("SELECT COUNT(*) FROM ecs_goods {where_clause}");
            let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
            conn.query_row(&sql, params_ref.as_slice(), |r| r.get(0))
                .map_err(db_err)?
        };
        let sql = format!(
            "SELECT goods_id, goods_name, shop_price FROM ecs_goods {where_clause} ORDER BY goods_id DESC LIMIT ? OFFSET ?"
        );
        params.push(Box::new(page_size));
        params.push(Box::new(offset));
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                let price: f64 = r.get(2)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "price": cents_to_string((price * 100.0).round() as i64),
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

// ---------------------------------------------------------------------------
// Wholesale (wholesale.php)
// ---------------------------------------------------------------------------

/// GET /api/v1/wholesale-goods
pub async fn wholesale_goods(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT g.goods_id, g.goods_name, MIN(w.price_cents)
                 FROM ecs_wholesale w JOIN ecs_goods g ON g.goods_id = w.goods_id
                 WHERE g.is_on_sale = 1 AND g.is_delete = 0
                 GROUP BY g.goods_id ORDER BY g.goods_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(json!({
                    "goods_id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "min_price": cents_to_string(r.get::<_, i64>(2)?),
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

#[derive(Deserialize)]
pub struct WholesaleQuoteRequest {
    goods_id: i64,
    quantity: i64,
}

/// POST /api/v1/wholesale/quote
pub async fn wholesale_quote(
    State(state): State<AppState>,
    Json(body): Json<WholesaleQuoteRequest>,
) -> Result<Json<Value>, AppError> {
    if body.quantity < 1 {
        return Err(AppError::Validation("quantity must be positive".to_string()));
    }
    let db = state.db.clone();
    let quantity = body.quantity;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let row: Option<(i64, i64)> = conn
            .query_row(
                "SELECT price_cents, min_quantity FROM ecs_wholesale
                 WHERE goods_id = ?1 AND min_quantity <= ?2
                 ORDER BY min_quantity DESC LIMIT 1",
                rusqlite::params![body.goods_id, quantity],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let Some((price_cents, min_quantity)) = row else {
            return Err(AppError::NotFound("no wholesale tier for this goods/quantity".to_string()));
        };
        let total = price_cents
            .checked_mul(quantity)
            .ok_or_else(|| AppError::Validation("total is out of range".to_string()))?;
        Ok(json!({
            "goods_id": body.goods_id,
            "quantity": quantity,
            "min_quantity": min_quantity,
            "unit_price": cents_to_string(price_cents),
            "total": cents_to_string(total),
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// Captcha (captcha.php) — stateless math challenge with HMAC binding
// ---------------------------------------------------------------------------

/// GET /api/v1/captcha
pub async fn captcha_issue(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let a = rand::random::<u32>() % 19 + 1;
    let b = rand::random::<u32>() % 19 + 1;
    let secret = state.config.payment_callback_secret.clone();
    let sig = crypto::hmac_sha256_hex(&secret, format!("{a}+{b}").as_bytes());
    let challenge = format!("{a}.{b}.{sig}");
    Ok(Json(json!({
        "challenge": challenge,
        "question": format!("{a} + {b} = ?"),
    })))
}

#[derive(Deserialize)]
pub struct CaptchaVerifyRequest {
    challenge: String,
    answer: u32,
}

/// POST /api/v1/captcha/verify
pub async fn captcha_verify(
    State(state): State<AppState>,
    Json(body): Json<CaptchaVerifyRequest>,
) -> Result<Json<Value>, AppError> {
    let parts: Vec<&str> = body.challenge.split('.').collect();
    if parts.len() != 3 {
        return Err(AppError::Validation("invalid challenge".to_string()));
    }
    let a: u32 = parts[0]
        .parse()
        .map_err(|_| AppError::Validation("invalid challenge".to_string()))?;
    let b: u32 = parts[1]
        .parse()
        .map_err(|_| AppError::Validation("invalid challenge".to_string()))?;
    let sig = parts[2];
    let secret = state.config.payment_callback_secret.clone();
    let expected = crypto::hmac_sha256_hex(&secret, format!("{a}+{b}").as_bytes());
    if sig != expected {
        return Err(AppError::Validation("invalid challenge".to_string()));
    }
    let ok = a + b == body.answer;
    Ok(Json(json!({"valid": ok})))
}

// ---------------------------------------------------------------------------
// api.php compat — limited REST read of goods search
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[allow(dead_code)]
pub struct ApiCompatQuery {
    keyword: Option<String>,
    page: Option<i64>,
    page_size: Option<i64>,
}

/// GET /api.php?keyword=&page=&page_size=
pub async fn api_compat_search(
    State(state): State<AppState>,
    Query(q): Query<ApiCompatQuery>,
) -> Result<Json<Value>, AppError> {
    let page = PageParams { page: q.page, page_size: q.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let db = state.db.clone();
    let keyword = q.keyword.unwrap_or_default();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut where_clause = "WHERE is_on_sale = 1 AND is_delete = 0".to_string();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if !keyword.is_empty() {
            where_clause.push_str(" AND goods_name LIKE ? ESCAPE '\\'");
            let escaped = format!(
                "%{}%",
                keyword.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
            );
            params.push(Box::new(escaped));
        }
        let total: i64 = {
            let sql = format!("SELECT COUNT(*) FROM ecs_goods {where_clause}");
            let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
            conn.query_row(&sql, params_ref.as_slice(), |r| r.get(0))
                .map_err(db_err)?
        };
        let sql = format!(
            "SELECT goods_id, goods_name, shop_price FROM ecs_goods {where_clause} ORDER BY goods_id DESC LIMIT ? OFFSET ?"
        );
        params.push(Box::new(page_size));
        params.push(Box::new(offset));
        let params_ref: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(db_err)?;
        let rows = stmt
            .query_map(params_ref.as_slice(), |r| {
                let price: f64 = r.get(2)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "name": r.get::<_, String>(1)?,
                    "price": cents_to_string((price * 100.0).round() as i64),
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"page": page_no, "page_size": page_size, "total": total, "goods": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

// ---------------------------------------------------------------------------
// certi.php compat — safe store certificate/protocol stub
// ---------------------------------------------------------------------------

/// GET /api/v1/certi
pub async fn certi() -> Result<Json<Value>, AppError> {
    Ok(Json(json!({
        "status": "ok",
        "protocol": "ecshop-rest-v1",
        "message": "store certificate compatibility stub",
    })))
}
