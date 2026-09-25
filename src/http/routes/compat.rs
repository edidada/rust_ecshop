//! Compatibility and supplementary URLs: affiliate, shipments, points, browsing history,
//! personal messages (pm), pick-out, wholesale, captcha, api.php compat, certi.
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::state::AppState;
use crate::infrastructure::crypto;
use crate::shared::error::AppError;
use crate::shared::rbutil::{col_i64, col_str, col_cents, col_bool, col_opt_str, e, insert_id, q, q1};
use crate::shared::util::{cents_to_string, parse_money_cents, unix_now, PageParams};

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
    let rb = state.rb;
    let levels = q(
        rb,
        "WITH RECURSIVE downline(user_id, level) AS (
            SELECT ?, 0
            UNION ALL
            SELECT ur.user_id, d.level + 1
            FROM ecs_user_referral ur JOIN downline d ON ur.referrer_user_id = d.user_id
            WHERE d.level < 10
         )
         SELECT level - 1 AS depth, COUNT(*) AS cnt FROM downline WHERE level > 0 GROUP BY depth ORDER BY depth",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| json!({"level": col_i64(&r, "depth"), "count": col_i64(&r, "cnt")}))
    .collect::<Vec<_>>();
    let orders = q(
        rb,
        "WITH RECURSIVE downline(user_id, level) AS (
            SELECT ?, 0
            UNION ALL
            SELECT ur.user_id, d.level + 1
            FROM ecs_user_referral ur JOIN downline d ON ur.referrer_user_id = d.user_id
            WHERE d.level < 10
         )
         SELECT o.order_sn AS order_sn, u.user_name AS user_name, o.order_amount AS order_amount,
                o.order_id AS order_id, o.created_at AS created_at,
                (SELECT COUNT(*) FROM ecs_affiliate_log al WHERE al.order_id = o.order_id AND al.user_id = ?) AS separated
         FROM ecs_order_info o
         JOIN ecs_users u ON u.user_id = o.user_id
         WHERE o.user_id IN (SELECT user_id FROM downline WHERE user_id <> ?)
         ORDER BY o.order_id DESC LIMIT ? OFFSET ?",
        vec![json!(auth.user_id), json!(auth.user_id), json!(auth.user_id), json!(page_size), json!(offset)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "order_sn": mask_order_sn(&col_str(&r, "order_sn")),
            "user_name": col_str(&r, "user_name"),
            "order_amount": cents_to_string(col_cents(&r, "order_amount")),
            "order_id": col_i64(&r, "order_id"),
            "created_at": col_i64(&r, "created_at"),
            "separated": col_i64(&r, "separated") > 0,
        })
    })
    .collect::<Vec<_>>();
    let logs = q(
        rb,
        "SELECT log_id AS log_id, order_id AS order_id, money AS money, point AS point, separate_type AS separate_type, created_at AS created_at
         FROM ecs_affiliate_log WHERE user_id = ? ORDER BY log_id DESC",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "log_id"),
            "order_id": col_i64(&r, "order_id"),
            "commission": cents_to_string(col_cents(&r, "money")),
            "points": col_i64(&r, "point"),
            "separate_type": col_i64(&r, "separate_type"),
            "created_at": col_i64(&r, "created_at"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({
        "page": page_no,
        "page_size": page_size,
        "levels": levels,
        "orders": orders,
        "logs": logs,
    })))
}

// ---------------------------------------------------------------------------
// GET /api/v1/me/shipments (user.php?act=track_packages)
// ---------------------------------------------------------------------------

/// GET /api/v1/me/shipments — paid/shipped/received orders with shipping info.
pub async fn shipments(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let items = q(
        state.rb,
        "SELECT o.order_id AS order_id, o.order_sn AS order_sn, o.order_status AS order_status,
                s.shipping_name AS shipping_name, o.created_at AS created_at
         FROM ecs_order_info o JOIN ecs_shipping s ON s.shipping_id = o.shipping_id
         WHERE o.user_id = ? AND o.order_status IN ('paid', 'shipped', 'received')
         ORDER BY o.order_id DESC",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "order_id": col_i64(&r, "order_id"),
            "order_sn": col_str(&r, "order_sn"),
            "status": col_str(&r, "order_status"),
            "shipping_name": col_str(&r, "shipping_name"),
            "created_at": col_i64(&r, "created_at"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

// ---------------------------------------------------------------------------
// Points conversion (user.php?act=transform_points)
// ---------------------------------------------------------------------------

/// GET /api/v1/me/points/conversion-options
pub async fn points_options(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let row = q1(
        state.rb,
        "SELECT COALESCE(pay_points, 0) AS pay_points FROM ecs_users WHERE user_id = ?",
        vec![json!(auth.user_id)],
    )
    .await?
    .unwrap_or(json!({"pay_points": 0}));
    // Fixed development rate: 100 points = 1.00 CNY.
    Ok(Json(json!({
        "pay_points": col_i64(&row, "pay_points"),
        "points_per_yuan": 100,
        "min_points": 100,
    })))
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
    let now = unix_now();
    let cents = body.points; // 100 points = 1.00 CNY => 1 point = 1 cent.
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let updated = e(
        &tx,
        "UPDATE ecs_users SET pay_points = pay_points - ? WHERE user_id = ? AND pay_points >= ?",
        vec![json!(body.points), json!(auth.user_id), json!(body.points)],
    )
    .await?;
    if updated.rows_affected == 0 {
        return Err(AppError::Conflict("insufficient points".to_string()));
    }
    e(
        &tx,
        "INSERT INTO ecs_account_balance (user_id, available_cents, frozen_cents) VALUES (?, ?, 0)
         ON CONFLICT(user_id) DO UPDATE SET available_cents = available_cents + ?",
        vec![json!(auth.user_id), json!(cents), json!(cents)],
    )
    .await?;
    e(
        &tx,
        "INSERT INTO ecs_account_log (user_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at)
         VALUES (?, ?, 0, 'points_conversion', 'users', ?, ?)",
        vec![json!(auth.user_id), json!(cents), json!(auth.user_id), json!(now)],
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"converted_points": body.points, "balance": cents_to_string(cents)})),
    ))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/me/browsing-history
// ---------------------------------------------------------------------------

/// DELETE /api/v1/me/browsing-history
pub async fn browsing_history_clear(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<StatusCode, AppError> {
    e(
        state.rb,
        "DELETE FROM ecs_browsing_history WHERE user_id = ?",
        vec![json!(auth.user_id)],
    )
    .await?;
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
    let now = unix_now();
    let exists = q1(
        state.rb,
        "SELECT COUNT(*) AS cnt FROM ecs_users WHERE user_id = ?",
        vec![json!(body.to_user_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if exists == 0 {
        return Err(AppError::NotFound("recipient not found".to_string()));
    }
    let result = e(
        state.rb,
        "INSERT INTO ecs_pm (user_id, from_user_id, title, content, created_at) VALUES (?, ?, ?, ?, ?)",
        vec![
            json!(body.to_user_id),
            json!(auth.user_id),
            json!(body.title),
            json!(body.content),
            json!(now),
        ],
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({"id": insert_id(&result)}))))
}

/// GET /api/v1/me/pms
pub async fn pm_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let items = q(
        state.rb,
        "SELECT p.pm_id AS pm_id, p.from_user_id AS from_user_id, u.user_name AS user_name,
                p.title AS title, p.content AS content, p.is_read AS is_read, p.created_at AS created_at
         FROM ecs_pm p JOIN ecs_users u ON u.user_id = p.from_user_id
         WHERE p.user_id = ? ORDER BY p.pm_id DESC",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "pm_id"),
            "from_user_id": col_i64(&r, "from_user_id"),
            "from_user_name": col_str(&r, "user_name"),
            "title": col_str(&r, "title"),
            "content": col_str(&r, "content"),
            "is_read": col_bool(&r, "is_read"),
            "created_at": col_i64(&r, "created_at"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

/// DELETE /api/v1/me/pms/{id}
pub async fn pm_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let pm_id = parse_id(&id)?;
    let deleted = e(
        state.rb,
        "DELETE FROM ecs_pm WHERE pm_id = ? AND user_id = ?",
        vec![json!(pm_id), json!(auth.user_id)],
    )
    .await?;
    if deleted.rows_affected == 0 {
        return Err(AppError::NotFound("message not found".to_string()));
    }
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
    Query(qq): Query<PickOutQuery>,
) -> Result<Json<Value>, AppError> {
    let page = PageParams { page: qq.page, page_size: qq.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let rb = state.rb;
    let cat_ids: Vec<i64> = match &qq.cat_ids {
        Some(s) if !s.is_empty() => s
            .split(',')
            .map(|x| x.trim().parse::<i64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| AppError::Validation("cat_ids must be integers".to_string()))?,
        _ => Vec::new(),
    };
    let min_cents = match &qq.min_price {
        Some(s) if !s.is_empty() => Some(parse_money_cents(s, "min_price")?),
        _ => None,
    };
    let max_cents = match &qq.max_price {
        Some(s) if !s.is_empty() => Some(parse_money_cents(s, "max_price")?),
        _ => None,
    };
    let mut where_clause = "WHERE is_on_sale = 1 AND is_delete = 0".to_string();
    let mut args: Vec<Value> = Vec::new();
    if !cat_ids.is_empty() {
        let placeholders = cat_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        where_clause.push_str(&format!(" AND cat_id IN ({placeholders})"));
        for c in &cat_ids {
            args.push(json!(c));
        }
    }
    if let Some(min) = min_cents {
        where_clause.push_str(" AND shop_price >= ?");
        args.push(json!(min as f64 / 100.0));
    }
    if let Some(max) = max_cents {
        where_clause.push_str(" AND shop_price <= ?");
        args.push(json!(max as f64 / 100.0));
    }
    let total = q1(
        rb,
        &format!("SELECT COUNT(*) AS cnt FROM ecs_goods {where_clause}"),
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
            "SELECT goods_id AS goods_id, goods_name AS goods_name, shop_price AS shop_price
             FROM ecs_goods {where_clause} ORDER BY goods_id DESC LIMIT ? OFFSET ?"
        ),
        args,
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "goods_id"),
            "name": col_str(&r, "goods_name"),
            "price": cents_to_string(col_cents(&r, "shop_price")),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"page": page_no, "page_size": page_size, "total": total, "items": items})))
}

// ---------------------------------------------------------------------------
// Wholesale (wholesale.php)
// ---------------------------------------------------------------------------

/// GET /api/v1/wholesale-goods
pub async fn wholesale_goods(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let items = q(
        state.rb,
        "SELECT g.goods_id AS goods_id, g.goods_name AS goods_name, MIN(w.price_cents) AS min_price_cents
         FROM ecs_wholesale w JOIN ecs_goods g ON g.goods_id = w.goods_id
         WHERE g.is_on_sale = 1 AND g.is_delete = 0
         GROUP BY g.goods_id ORDER BY g.goods_id",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "goods_id": col_i64(&r, "goods_id"),
            "name": col_str(&r, "goods_name"),
            "min_price": cents_to_string(col_i64(&r, "min_price_cents")),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
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
    let row = q1(
        state.rb,
        "SELECT price_cents AS price_cents, min_quantity AS min_quantity FROM ecs_wholesale
         WHERE goods_id = ? AND min_quantity <= ?
         ORDER BY min_quantity DESC LIMIT 1",
        vec![json!(body.goods_id), json!(body.quantity)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("no wholesale tier for this goods/quantity".to_string()))?;
    let price_cents = col_i64(&row, "price_cents");
    let total = price_cents
        .checked_mul(body.quantity)
        .ok_or_else(|| AppError::Validation("total is out of range".to_string()))?;
    Ok(Json(json!({
        "goods_id": body.goods_id,
        "quantity": body.quantity,
        "min_quantity": col_i64(&row, "min_quantity"),
        "unit_price": cents_to_string(price_cents),
        "total": cents_to_string(total),
    })))
}

// ---------------------------------------------------------------------------
// Captcha (captcha.php) — stateless math challenge with HMAC binding
// ---------------------------------------------------------------------------

/// GET /api/v1/captcha
pub async fn captcha_issue(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let a = rand::random::<u32>() % 19 + 1;
    let b = rand::random::<u32>() % 19 + 1;
    let sig = crypto::hmac_sha256_hex(&state.config.payment_callback_secret, format!("{a}+{b}").as_bytes());
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
    let expected = crypto::hmac_sha256_hex(&state.config.payment_callback_secret, format!("{a}+{b}").as_bytes());
    if parts[2] != expected {
        return Err(AppError::Validation("invalid challenge".to_string()));
    }
    Ok(Json(json!({"valid": a + b == body.answer})))
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
    Query(qq): Query<ApiCompatQuery>,
) -> Result<Json<Value>, AppError> {
    let page = PageParams { page: qq.page, page_size: qq.page_size };
    let (page_no, page_size, offset) = page.resolve();
    let rb = state.rb;
    let keyword = qq.keyword.unwrap_or_default();
    let mut where_clause = "WHERE is_on_sale = 1 AND is_delete = 0".to_string();
    let mut args: Vec<Value> = Vec::new();
    if !keyword.is_empty() {
        where_clause.push_str(" AND goods_name LIKE ? ESCAPE '\\'");
        let escaped = format!(
            "%{}%",
            keyword.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
        );
        args.push(json!(escaped));
    }
    let total = q1(
        rb,
        &format!("SELECT COUNT(*) AS cnt FROM ecs_goods {where_clause}"),
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
            "SELECT goods_id AS goods_id, goods_name AS goods_name, shop_price AS shop_price
             FROM ecs_goods {where_clause} ORDER BY goods_id DESC LIMIT ? OFFSET ?"
        ),
        args,
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "goods_id"),
            "name": col_str(&r, "goods_name"),
            "price": cents_to_string(col_cents(&r, "shop_price")),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"page": page_no, "page_size": page_size, "total": total, "goods": items})))
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

#[allow(dead_code)]
fn _keep(row: &Value) -> Option<String> {
    col_opt_str(row, "x")
}
