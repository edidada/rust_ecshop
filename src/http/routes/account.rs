//! Account asset context: addresses, favorites, bookings, bonuses, email verification,
//! password resets, newsletter, account balance requests and transactions.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::routes::auth::insert_outbox;
use crate::http::state::AppState;
use crate::infrastructure::crypto;
use crate::shared::error::AppError;
use crate::shared::rbutil::{
    col_bool, col_i64, col_opt_i64, col_str, col_cents, e, insert_id, q, q1,
};
use crate::shared::util::{cents_to_string, parse_money_cents, unix_now};

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation("id must be a positive integer".to_string())),
    }
}

// ---------------------------------------------------------------------------
// Addresses
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct AddressRequest {
    consignee: String,
    #[serde(default)]
    country_id: i64,
    #[serde(default)]
    province_id: i64,
    #[serde(default)]
    city_id: i64,
    #[serde(default)]
    district_id: i64,
    address: String,
    #[serde(default)]
    zipcode: String,
    mobile: String,
    #[serde(default)]
    is_default: bool,
}

/// POST /api/v1/me/addresses
pub async fn address_create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<AddressRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if body.consignee.is_empty() || body.address.is_empty() || body.mobile.is_empty() {
        return Err(AppError::Validation("consignee/address/mobile are required".to_string()));
    }
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    for (level, rid) in [
        ("country", body.country_id),
        ("province", body.province_id),
        ("city", body.city_id),
        ("district", body.district_id),
    ] {
        if rid > 0 {
            let exists = q1(
                &tx,
                "SELECT COUNT(*) AS cnt FROM ecs_region WHERE region_id = ?",
                vec![json!(rid)],
            )
            .await?
            .map(|r| col_i64(&r, "cnt"))
            .unwrap_or(0);
            if exists == 0 {
                return Err(AppError::Validation(format!("{level} region not found")));
            }
        }
    }
    if body.is_default {
        e(
            &tx,
            "UPDATE ecs_user_address SET is_default = 0 WHERE user_id = ?",
            vec![json!(auth.user_id)],
        )
        .await?;
    }
    let result = e(
        &tx,
        "INSERT INTO ecs_user_address (user_id, consignee, country_id, province_id, city_id, district_id, address, zipcode, mobile, is_default)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            json!(auth.user_id),
            json!(body.consignee),
            json!(body.country_id),
            json!(body.province_id),
            json!(body.city_id),
            json!(body.district_id),
            json!(body.address),
            json!(body.zipcode),
            json!(body.mobile),
            json!(if body.is_default { 1 } else { 0 }),
        ],
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"id": insert_id(&result)}))))
}

/// GET /api/v1/me/addresses
pub async fn address_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let items = q(
        state.rb,
        "SELECT address_id AS address_id, consignee AS consignee, country_id AS country_id, province_id AS province_id,
                city_id AS city_id, district_id AS district_id, address AS address, zipcode AS zipcode, mobile AS mobile, is_default AS is_default
         FROM ecs_user_address WHERE user_id = ? ORDER BY is_default DESC, address_id",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "address_id"),
            "consignee": col_str(&r, "consignee"),
            "country_id": col_i64(&r, "country_id"),
            "province_id": col_i64(&r, "province_id"),
            "city_id": col_i64(&r, "city_id"),
            "district_id": col_i64(&r, "district_id"),
            "address": col_str(&r, "address"),
            "zipcode": col_str(&r, "zipcode"),
            "mobile": col_str(&r, "mobile"),
            "is_default": col_bool(&r, "is_default"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

/// PATCH /api/v1/me/addresses/{id}
pub async fn address_update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<AddressRequest>,
) -> Result<StatusCode, AppError> {
    let address_id = parse_id(&id)?;
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    if body.is_default {
        e(
            &tx,
            "UPDATE ecs_user_address SET is_default = 0 WHERE user_id = ?",
            vec![json!(auth.user_id)],
        )
        .await?;
    }
    let updated = e(
        &tx,
        "UPDATE ecs_user_address SET consignee = ?, country_id = ?, province_id = ?, city_id = ?,
         district_id = ?, address = ?, zipcode = ?, mobile = ?, is_default = ?
         WHERE address_id = ? AND user_id = ?",
        vec![
            json!(body.consignee),
            json!(body.country_id),
            json!(body.province_id),
            json!(body.city_id),
            json!(body.district_id),
            json!(body.address),
            json!(body.zipcode),
            json!(body.mobile),
            json!(if body.is_default { 1 } else { 0 }),
            json!(address_id),
            json!(auth.user_id),
        ],
    )
    .await?;
    if updated.rows_affected == 0 {
        return Err(AppError::NotFound("address not found".to_string()));
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/me/addresses/{id}
pub async fn address_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let address_id = parse_id(&id)?;
    let deleted = e(
        state.rb,
        "DELETE FROM ecs_user_address WHERE address_id = ? AND user_id = ?",
        vec![json!(address_id), json!(auth.user_id)],
    )
    .await?;
    if deleted.rows_affected == 0 {
        return Err(AppError::NotFound("address not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Favorites
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct FavoriteCreateRequest {
    goods_id: i64,
}

/// POST /api/v1/me/favorites
pub async fn favorite_create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<FavoriteCreateRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if body.goods_id <= 0 {
        return Err(AppError::Validation("goods_id must be positive".to_string()));
    }
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let ok = q1(
        &tx,
        "SELECT COUNT(*) AS cnt FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
        vec![json!(body.goods_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if ok == 0 {
        return Err(AppError::NotFound("goods not found".to_string()));
    }
    let exists = q1(
        &tx,
        "SELECT COUNT(*) AS cnt FROM ecs_collect_goods WHERE user_id = ? AND goods_id = ?",
        vec![json!(auth.user_id), json!(body.goods_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if exists > 0 {
        return Err(AppError::Conflict("goods already in favorites".to_string()));
    }
    let result = e(
        &tx,
        "INSERT INTO ecs_collect_goods (user_id, goods_id, add_time) VALUES (?, ?, ?)",
        vec![json!(auth.user_id), json!(body.goods_id), json!(unix_now())],
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"id": insert_id(&result), "goods_id": body.goods_id, "attention": false})),
    ))
}

/// GET /api/v1/me/favorites
pub async fn favorite_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let items = q(
        state.rb,
        "SELECT c.rec_id AS rec_id, c.goods_id AS goods_id, g.goods_name AS goods_name, g.shop_price AS shop_price,
                g.goods_thumb AS goods_thumb, c.is_attention AS is_attention
         FROM ecs_collect_goods c JOIN ecs_goods g ON g.goods_id = c.goods_id
         WHERE c.user_id = ? ORDER BY c.rec_id DESC",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "rec_id"),
            "goods_id": col_i64(&r, "goods_id"),
            "name": col_str(&r, "goods_name"),
            "price": cents_to_string(col_cents(&r, "shop_price")),
            "thumb": col_str(&r, "goods_thumb"),
            "attention": col_bool(&r, "is_attention"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

#[derive(Deserialize)]
pub struct FavoritePatchRequest {
    attention: bool,
}

/// PATCH /api/v1/me/favorites/{id}
pub async fn favorite_update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<FavoritePatchRequest>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    let updated = e(
        state.rb,
        "UPDATE ecs_collect_goods SET is_attention = ? WHERE rec_id = ? AND user_id = ?",
        vec![
            json!(if body.attention { 1 } else { 0 }),
            json!(rec_id),
            json!(auth.user_id),
        ],
    )
    .await?;
    if updated.rows_affected == 0 {
        return Err(AppError::NotFound("favorite not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/me/favorites/{id}
pub async fn favorite_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    let deleted = e(
        state.rb,
        "DELETE FROM ecs_collect_goods WHERE rec_id = ? AND user_id = ?",
        vec![json!(rec_id), json!(auth.user_id)],
    )
    .await?;
    if deleted.rows_affected == 0 {
        return Err(AppError::NotFound("favorite not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Bookings (out-of-stock notifications)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct BookingCreateRequest {
    goods_id: i64,
    #[serde(default = "one")]
    goods_number: i64,
    #[serde(default)]
    description: String,
    #[serde(default)]
    linkman: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    telephone: String,
}

fn one() -> i64 {
    1
}

/// POST /api/v1/me/bookings
pub async fn booking_create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<BookingCreateRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if body.goods_id <= 0 {
        return Err(AppError::Validation("goods_id must be positive".to_string()));
    }
    if body.goods_number < 1 {
        return Err(AppError::Validation("goods_number must be at least 1".to_string()));
    }
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let ok = q1(
        &tx,
        "SELECT COUNT(*) AS cnt FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
        vec![json!(body.goods_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if ok == 0 {
        return Err(AppError::NotFound("goods not found".to_string()));
    }
    // Unique (user, goods) prevents TOCTOU duplicates.
    let result = e(
        &tx,
        "INSERT INTO ecs_booking_goods (user_id, email, link_man, tel, goods_id, goods_desc, goods_number, booking_time)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        vec![
            json!(auth.user_id),
            json!(body.email),
            json!(body.linkman),
            json!(body.telephone),
            json!(body.goods_id),
            json!(body.description),
            json!(body.goods_number),
            json!(unix_now()),
        ],
    )
    .await;
    let rec_id = match result {
        Ok(r) => insert_id(&r),
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("UNIQUE") || msg.contains("unique") {
                return Err(AppError::Conflict("booking already exists for this goods".to_string()));
            }
            return Err(err);
        }
    };
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({"id": rec_id}))))
}

/// GET /api/v1/me/bookings
pub async fn booking_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let items = q(
        state.rb,
        "SELECT b.rec_id AS rec_id, b.goods_id AS goods_id, g.goods_name AS goods_name, b.goods_number AS goods_number,
                b.booking_time AS booking_time, b.is_dispose AS is_dispose, b.dispose_note AS dispose_note
         FROM ecs_booking_goods b JOIN ecs_goods g ON g.goods_id = b.goods_id
         WHERE b.user_id = ? ORDER BY b.booking_time DESC",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "rec_id"),
            "goods_id": col_i64(&r, "goods_id"),
            "goods_name": col_str(&r, "goods_name"),
            "goods_number": col_i64(&r, "goods_number"),
            "booking_time": col_i64(&r, "booking_time"),
            "is_dispose": col_bool(&r, "is_dispose"),
            "dispose_note": col_str(&r, "dispose_note"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

/// DELETE /api/v1/me/bookings/{id}
pub async fn booking_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    let deleted = e(
        state.rb,
        "DELETE FROM ecs_booking_goods WHERE rec_id = ? AND user_id = ?",
        vec![json!(rec_id), json!(auth.user_id)],
    )
    .await?;
    if deleted.rows_affected == 0 {
        return Err(AppError::NotFound("booking not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Bonuses
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct BonusClaimRequest {
    bonus_sn: String,
}

/// POST /api/v1/me/bonuses/claim
pub async fn bonus_claim(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<BonusClaimRequest>,
) -> Result<StatusCode, AppError> {
    let sn = body.bonus_sn.trim().to_string();
    if sn.is_empty() || sn.len() > 20 || !sn.chars().all(|c| c.is_ascii_digit()) {
        return Err(AppError::Validation("bonus_sn must be 1-20 digit string".to_string()));
    }
    let sn_value: i64 = sn
        .parse()
        .map_err(|_| AppError::Validation("bonus_sn is out of range".to_string()))?;
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let row = q1(
        &tx,
        "SELECT b.bonus_id AS bonus_id, t.use_end_date AS use_end_date
         FROM ecs_user_bonus b JOIN ecs_bonus_type t ON t.type_id = b.bonus_type_id
         WHERE b.bonus_sn = ?",
        vec![json!(sn_value)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("bonus not found".to_string()))?;
    let bonus_id = col_i64(&row, "bonus_id");
    if now > col_i64(&row, "use_end_date") {
        return Err(AppError::Conflict("bonus is expired".to_string()));
    }
    // Atomic claim: user_id=0 is the unclaimed marker.
    let claimed = e(
        &tx,
        "UPDATE ecs_user_bonus SET user_id = ? WHERE bonus_id = ? AND user_id = 0",
        vec![json!(auth.user_id), json!(bonus_id)],
    )
    .await?;
    if claimed.rows_affected == 0 {
        return Err(AppError::Conflict("bonus already claimed".to_string()));
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/me/bonuses
pub async fn bonus_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let now = unix_now();
    let items = q(
        state.rb,
        "SELECT b.bonus_id AS bonus_id, b.bonus_sn AS bonus_sn, t.type_name AS type_name, t.type_money AS type_money,
                t.use_start_date AS use_start_date, t.use_end_date AS use_end_date, b.order_id AS order_id, b.used_time AS used_time
         FROM ecs_user_bonus b JOIN ecs_bonus_type t ON t.type_id = b.bonus_type_id
         WHERE b.user_id = ? ORDER BY b.bonus_id DESC",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        let used_time = col_i64(&r, "used_time");
        let use_start = col_i64(&r, "use_start_date");
        let use_end = col_i64(&r, "use_end_date");
        let status = if used_time > 0 {
            "used"
        } else if now < use_start {
            "not_started"
        } else if now > use_end {
            "expired"
        } else {
            "available"
        };
        json!({
            "id": col_i64(&r, "bonus_id"),
            "bonus_sn": col_i64(&r, "bonus_sn").to_string(),
            "type_name": col_str(&r, "type_name"),
            "amount": cents_to_string(col_cents(&r, "type_money")),
            "status": status,
            "order_id": if used_time > 0 { Some(col_i64(&r, "order_id")) } else { None },
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

// ---------------------------------------------------------------------------
// Email verification
// ---------------------------------------------------------------------------

/// POST /api/v1/me/email-verifications — queues a verify_email outbox message.
pub async fn email_verification_request(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let token = crypto::random_token(32);
    let token_hash = crypto::sha256_hex(token.as_bytes());
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let email = q1(
        &tx,
        "SELECT email AS email FROM ecs_users WHERE user_id = ?",
        vec![json!(auth.user_id)],
    )
    .await?
    .map(|r| col_str(&r, "email"))
    .unwrap_or_default();
    // Duplicate requests atomically replace the old token.
    e(
        &tx,
        "INSERT INTO ecs_email_verification_tokens (user_id, token_hash, expires_at, created_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(user_id) DO UPDATE SET token_hash = ?, expires_at = ?, created_at = ?, consumed_at = NULL",
        vec![
            json!(auth.user_id),
            json!(token_hash),
            json!(now + 86_400),
            json!(now),
            json!(token_hash),
            json!(now + 86_400),
            json!(now),
        ],
    )
    .await?;
    insert_outbox(&tx, Some(auth.user_id), &email, "verify_email", &json!({"token": token})).await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"status": "queued"}))))
}

#[derive(Deserialize)]
pub struct TokenConfirmRequest {
    token: String,
}

/// POST /api/v1/email-verifications/confirm
pub async fn email_verification_confirm(
    State(state): State<AppState>,
    Json(body): Json<TokenConfirmRequest>,
) -> Result<StatusCode, AppError> {
    if body.token.len() != 64 {
        return Err(AppError::Validation("token must be 64 hex chars".to_string()));
    }
    let token_hash = crypto::sha256_hex(body.token.as_bytes());
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let row = q1(
        &tx,
        "SELECT user_id AS user_id FROM ecs_email_verification_tokens
         WHERE token_hash = ? AND consumed_at IS NULL AND expires_at > ?",
        vec![json!(token_hash), json!(now)],
    )
    .await?
    .ok_or_else(|| AppError::Conflict("invalid or expired token".to_string()))?;
    let user_id = col_i64(&row, "user_id");
    e(
        &tx,
        "INSERT OR IGNORE INTO ecs_email_verified_users (user_id, verified_at) VALUES (?, ?)",
        vec![json!(user_id), json!(now)],
    )
    .await?;
    let consumed = e(
        &tx,
        "UPDATE ecs_email_verification_tokens SET consumed_at = ? WHERE token_hash = ? AND consumed_at IS NULL",
        vec![json!(now), json!(token_hash)],
    )
    .await?;
    if consumed.rows_affected != 1 {
        return Err(AppError::Conflict("token already consumed".to_string()));
    }
    e(
        &tx,
        "UPDATE ecs_users SET is_validated = 1 WHERE user_id = ?",
        vec![json!(user_id)],
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Password resets
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PasswordResetRequest {
    email: String,
}

/// POST /api/v1/password-resets — always 202 to prevent account enumeration.
pub async fn password_reset_request(
    State(state): State<AppState>,
    Json(body): Json<PasswordResetRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let user = q1(
        &tx,
        "SELECT user_id AS user_id FROM ecs_users WHERE email = ?",
        vec![json!(body.email)],
    )
    .await?
    .map(|r| col_i64(&r, "user_id"));
    if let Some(user_id) = user {
        let token = crypto::random_token(32);
        let token_hash = crypto::sha256_hex(token.as_bytes());
        e(
            &tx,
            "DELETE FROM ecs_password_reset_tokens WHERE user_id = ?",
            vec![json!(user_id)],
        )
        .await?;
        e(
            &tx,
            "INSERT INTO ecs_password_reset_tokens (user_id, token_hash, expires_at, created_at) VALUES (?, ?, ?, ?)",
            vec![json!(user_id), json!(token_hash), json!(now + 3_600), json!(now)],
        )
        .await?;
        insert_outbox(&tx, Some(user_id), &body.email, "password_reset", &json!({"token": token})).await?;
    }
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"status": "accepted"}))))
}

#[derive(Deserialize)]
pub struct PasswordResetConfirmRequest {
    token: String,
    new_password: String,
}

/// POST /api/v1/password-resets/confirm — consumes token, re-hashes, revokes sessions.
pub async fn password_reset_confirm(
    State(state): State<AppState>,
    Json(body): Json<PasswordResetConfirmRequest>,
) -> Result<StatusCode, AppError> {
    let len = body.new_password.len();
    if !(8..=1024).contains(&len) {
        return Err(AppError::Validation("password must be 8-1024 bytes".to_string()));
    }
    let token_hash = crypto::sha256_hex(body.token.as_bytes());
    let new_hash = crypto::hash_password(&body.new_password);
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let row = q1(
        &tx,
        "SELECT user_id AS user_id FROM ecs_password_reset_tokens
         WHERE token_hash = ? AND consumed_at IS NULL AND expires_at > ?",
        vec![json!(token_hash), json!(now)],
    )
    .await?
    .ok_or_else(|| AppError::Conflict("invalid or expired token".to_string()))?;
    let user_id = col_i64(&row, "user_id");
    let consumed = e(
        &tx,
        "UPDATE ecs_password_reset_tokens SET consumed_at = ? WHERE token_hash = ? AND consumed_at IS NULL",
        vec![json!(now), json!(token_hash)],
    )
    .await?;
    if consumed.rows_affected != 1 {
        return Err(AppError::Conflict("token already consumed".to_string()));
    }
    e(
        &tx,
        "UPDATE ecs_users SET password_hash = ? WHERE user_id = ?",
        vec![json!(new_hash), json!(user_id)],
    )
    .await?;
    crate::http::auth::revoke_user_sessions(&tx, user_id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Newsletter
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct NewsletterRequest {
    email: String,
}

/// POST /api/v1/newsletter-subscriptions — 202, one-time token into outbox.
pub async fn newsletter_subscribe(
    State(state): State<AppState>,
    Json(body): Json<NewsletterRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    newsletter_action(state, body.email, "subscribe").await
}

/// DELETE /api/v1/newsletter-subscriptions
pub async fn newsletter_unsubscribe(
    State(state): State<AppState>,
    Json(body): Json<NewsletterRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    newsletter_action(state, body.email, "unsubscribe").await
}

async fn newsletter_action(
    state: AppState,
    email: String,
    action: &str,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if email.is_empty() || email.len() > 120 {
        return Err(AppError::Validation("email must be 1-120 bytes".to_string()));
    }
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let existing = q1(
        &tx,
        "SELECT status AS status FROM ecs_email_list WHERE email = ?",
        vec![json!(email)],
    )
    .await?
    .map(|r| col_i64(&r, "status"));
    let token = crypto::random_token(32);
    let token_hash = crypto::sha256_hex(token.as_bytes());
    match existing {
        Some(status) => {
            let subscribed = action == "subscribe";
            // Idempotent: already subscribed/unsubscribed => accept without a new mail.
            if (subscribed && status == 1) || (!subscribed && status == 0) {
                tx.commit().await?;
                return Ok((StatusCode::ACCEPTED, Json(json!({"status": "accepted"}))));
            }
            e(
                &tx,
                "UPDATE ecs_email_list SET token_hash = ?, pending_action = ?, token_expires_at = ?, updated_at = ? WHERE email = ?",
                vec![json!(token_hash), json!(action), json!(now + 86_400), json!(now), json!(email)],
            )
            .await?;
        }
        None => {
            e(
                &tx,
                "INSERT INTO ecs_email_list (email, status, token_hash, pending_action, token_expires_at, updated_at)
                 VALUES (?, 0, ?, ?, ?, ?)",
                vec![json!(email), json!(token_hash), json!(action), json!(now + 86_400), json!(now)],
            )
            .await?;
        }
    }
    let template = if action == "subscribe" { "newsletter_subscribe" } else { "newsletter_unsubscribe" };
    insert_outbox(&tx, None, &email, template, &json!({"token": token})).await?;
    tx.commit().await?;
    Ok((StatusCode::ACCEPTED, Json(json!({"status": "accepted"}))))
}

/// POST /api/v1/newsletter-subscriptions/confirm and /api/v1/newsletter-unsubscriptions/confirm
pub async fn newsletter_confirm(
    State(state): State<AppState>,
    Path(action): Path<String>,
    Json(body): Json<TokenConfirmRequest>,
) -> Result<StatusCode, AppError> {
    if body.token.len() != 64 {
        return Err(AppError::Validation("token must be 64 hex chars".to_string()));
    }
    let pending = match action.as_str() {
        "subscriptions" => "subscribe",
        "unsubscriptions" => "unsubscribe",
        _ => return Err(AppError::NotFound("unknown confirm action".to_string())),
    };
    let token_hash = crypto::sha256_hex(body.token.as_bytes());
    let now = unix_now();
    let updated = e(
        state.rb,
        "UPDATE ecs_email_list
         SET status = CASE WHEN ? = 'subscribe' THEN 1 ELSE 0 END,
             pending_action = '', token_hash = NULL, token_expires_at = NULL, updated_at = ?
         WHERE token_hash = ? AND pending_action = ? AND token_expires_at > ?",
        vec![json!(pending), json!(now), json!(token_hash), json!(pending), json!(now)],
    )
    .await?;
    if updated.rows_affected != 1 {
        return Err(AppError::Conflict("invalid or expired token".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// Account balance requests (deposit / withdrawal)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct AccountRequestCreate {
    kind: String,
    amount: String,
    #[serde(default)]
    payment_id: Option<i64>,
    #[serde(default)]
    note: String,
}

/// POST /api/v1/me/account/requests
pub async fn account_request_create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<AccountRequestCreate>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let amount_cents = parse_money_cents(&body.amount, "amount")?;
    if amount_cents <= 0 {
        return Err(AppError::Validation("amount must be positive".to_string()));
    }
    if body.kind != "deposit" && body.kind != "withdrawal" {
        return Err(AppError::Validation("kind must be deposit or withdrawal".to_string()));
    }
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let status = match body.kind.as_str() {
        "deposit" => {
            let pid = body
                .payment_id
                .ok_or_else(|| AppError::Validation("payment_id is required for deposits".to_string()))?;
            let enabled = q1(
                &tx,
                "SELECT enabled AS enabled FROM ecs_payment WHERE pay_id = ?",
                vec![json!(pid)],
            )
            .await?
            .map(|r| col_i64(&r, "enabled"))
            .ok_or_else(|| AppError::NotFound("payment not found".to_string()))?;
            if enabled != 1 {
                return Err(AppError::NotFound("payment not enabled".to_string()));
            }
            "pending_payment"
        }
        _ => {
            // Withdrawal: atomically freeze available balance.
            let updated = e(
                &tx,
                "UPDATE ecs_account_balance SET available_cents = available_cents - ?, frozen_cents = frozen_cents + ?
                 WHERE user_id = ? AND available_cents >= ?",
                vec![json!(amount_cents), json!(amount_cents), json!(auth.user_id), json!(amount_cents)],
            )
            .await?;
            if updated.rows_affected == 0 {
                return Err(AppError::Conflict("insufficient balance".to_string()));
            }
            e(
                &tx,
                "INSERT INTO ecs_account_log (user_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at)
                 VALUES (?, ?, ?, 'withdrawal_freeze', 'user_account', 0, ?)",
                vec![json!(auth.user_id), json!(-amount_cents), json!(amount_cents), json!(now)],
            )
            .await?;
            "pending_review"
        }
    };
    let result = e(
        &tx,
        "INSERT INTO ecs_user_account (user_id, amount_cents, process_type, payment_id, user_note, status, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        vec![
            json!(auth.user_id),
            json!(amount_cents),
            json!(body.kind),
            json!(body.payment_id),
            json!(body.note),
            json!(status),
            json!(now),
        ],
    )
    .await?;
    let rec_id = insert_id(&result);
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "id": rec_id,
            "kind": body.kind,
            "amount": cents_to_string(amount_cents),
            "status": status,
        })),
    ))
}

/// GET /api/v1/me/account/requests
pub async fn account_request_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let rb = state.rb;
    let balance = q1(
        rb,
        "SELECT available_cents AS available_cents, frozen_cents AS frozen_cents FROM ecs_account_balance WHERE user_id = ?",
        vec![json!(auth.user_id)],
    )
    .await?
    .unwrap_or(json!({"available_cents": 0, "frozen_cents": 0}));
    let items = q(
        rb,
        "SELECT rec_id AS rec_id, process_type AS process_type, amount_cents AS amount_cents, payment_id AS payment_id,
                status AS status, created_at AS created_at, paid_at AS paid_at
         FROM ecs_user_account WHERE user_id = ? ORDER BY rec_id DESC",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "rec_id"),
            "kind": col_str(&r, "process_type"),
            "amount": cents_to_string(col_i64(&r, "amount_cents")),
            "payment_id": col_opt_i64(&r, "payment_id"),
            "status": col_str(&r, "status"),
            "created_at": col_i64(&r, "created_at"),
            "paid_at": col_opt_i64(&r, "paid_at"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({
        "available_balance": cents_to_string(col_i64(&balance, "available_cents")),
        "frozen_balance": cents_to_string(col_i64(&balance, "frozen_cents")),
        "items": items,
    })))
}

/// DELETE /api/v1/me/account/requests/{id} — cancels unprocessed requests, unfreezes withdrawals.
pub async fn account_request_cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let row = q1(
        &tx,
        "SELECT process_type AS process_type, amount_cents AS amount_cents FROM ecs_user_account
         WHERE rec_id = ? AND user_id = ? AND ((process_type = 'deposit' AND status = 'pending_payment')
            OR (process_type = 'withdrawal' AND status = 'pending_review'))",
        vec![json!(rec_id), json!(auth.user_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("cancellable request not found".to_string()))?;
    let kind = col_str(&row, "process_type");
    let amount = col_i64(&row, "amount_cents");
    if kind == "withdrawal" {
        e(
            &tx,
            "UPDATE ecs_account_balance SET frozen_cents = frozen_cents - ?, available_cents = available_cents + ?
             WHERE user_id = ?",
            vec![json!(amount), json!(amount), json!(auth.user_id)],
        )
        .await?;
        e(
            &tx,
            "INSERT INTO ecs_account_log (user_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at)
             VALUES (?, ?, ?, 'withdrawal_cancel_unfreeze', 'user_account', ?, ?)",
            vec![json!(auth.user_id), json!(amount), json!(-amount), json!(rec_id), json!(now)],
        )
        .await?;
    }
    e(
        &tx,
        "DELETE FROM ecs_user_account WHERE rec_id = ?",
        vec![json!(rec_id)],
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct AccountPaymentRequest {
    payment_id: i64,
}

/// POST /api/v1/me/account/requests/{id}/payment — idempotent payment intent upsert.
pub async fn account_request_payment(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<AccountPaymentRequest>,
) -> Result<Json<Value>, AppError> {
    let rec_id = parse_id(&id)?;
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let row = q1(
        &tx,
        "SELECT amount_cents AS amount_cents FROM ecs_user_account
         WHERE rec_id = ? AND user_id = ? AND process_type = 'deposit' AND status = 'pending_payment'",
        vec![json!(rec_id), json!(auth.user_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("pending deposit not found".to_string()))?;
    let amount = col_i64(&row, "amount_cents");
    let fee_cents = q1(
        &tx,
        "SELECT pay_fee AS pay_fee FROM ecs_payment WHERE pay_id = ? AND enabled = 1",
        vec![json!(body.payment_id)],
    )
    .await?
    .map(|r| col_cents(&r, "pay_fee"))
    .ok_or_else(|| AppError::NotFound("payment not found or disabled".to_string()))?;
    // Idempotent: one intent per request; repeat calls update the same row.
    e(
        &tx,
        "INSERT INTO ecs_account_payment_intent (request_id, user_id, payment_id, amount_cents, fee_cents, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 'pending', ?, ?)
         ON CONFLICT(request_id) DO UPDATE SET payment_id = ?, fee_cents = ?, updated_at = ?",
        vec![
            json!(rec_id),
            json!(auth.user_id),
            json!(body.payment_id),
            json!(amount),
            json!(fee_cents),
            json!(now),
            json!(now),
            json!(body.payment_id),
            json!(fee_cents),
            json!(now),
        ],
    )
    .await?;
    let intent_id = q1(
        &tx,
        "SELECT intent_id AS intent_id FROM ecs_account_payment_intent WHERE request_id = ?",
        vec![json!(rec_id)],
    )
    .await?
    .map(|r| col_i64(&r, "intent_id"))
    .unwrap_or(0);
    tx.commit().await?;
    Ok(Json(json!({
        "id": intent_id,
        "request_id": rec_id,
        "payment_id": body.payment_id,
        "amount": cents_to_string(amount),
        "fee": cents_to_string(fee_cents),
        "total": cents_to_string(amount + fee_cents),
        "status": "pending",
    })))
}

/// GET /api/v1/me/account/transactions
pub async fn account_transactions(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let rb = state.rb;
    let balance = q1(
        rb,
        "SELECT available_cents AS available_cents, frozen_cents AS frozen_cents FROM ecs_account_balance WHERE user_id = ?",
        vec![json!(auth.user_id)],
    )
    .await?
    .unwrap_or(json!({"available_cents": 0, "frozen_cents": 0}));
    let items = q(
        rb,
        "SELECT log_id AS log_id, available_delta_cents AS available_delta_cents, frozen_delta_cents AS frozen_delta_cents,
                reason AS reason, reference_type AS reference_type, reference_id AS reference_id, created_at AS created_at
         FROM ecs_account_log WHERE user_id = ? ORDER BY log_id DESC",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| {
        let available_delta = col_i64(&r, "available_delta_cents");
        let frozen_delta = col_i64(&r, "frozen_delta_cents");
        json!({
            "id": col_i64(&r, "log_id"),
            "available_delta": format_signed_cents(available_delta),
            "frozen_delta": format_signed_cents(frozen_delta),
            "reason": col_str(&r, "reason"),
            "reference_type": col_str(&r, "reference_type"),
            "reference_id": col_i64(&r, "reference_id"),
            "created_at": col_i64(&r, "created_at"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({
        "available_balance": cents_to_string(col_i64(&balance, "available_cents")),
        "frozen_balance": cents_to_string(col_i64(&balance, "frozen_cents")),
        "items": items,
    })))
}

fn format_signed_cents(cents: i64) -> String {
    if cents >= 0 {
        cents_to_string(cents)
    } else {
        format!("-{}", cents_to_string(-cents))
    }
}
