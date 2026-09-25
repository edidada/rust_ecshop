//! Account asset context: addresses, favorites, bookings, bonuses, email verification,
//! password resets, newsletter, account balance requests and transactions.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::routes::auth::insert_outbox;
use crate::http::state::AppState;
use crate::infrastructure::crypto;
use crate::shared::error::AppError;
use crate::shared::util::{cents_to_string, parse_money_cents, unix_now};

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        for (level, rid) in [
            ("country", body.country_id),
            ("province", body.province_id),
            ("city", body.city_id),
            ("district", body.district_id),
        ] {
            if rid > 0 {
                let ok: i64 = tx
                    .query_row("SELECT COUNT(*) FROM ecs_region WHERE region_id = ?1", [rid], |r| r.get(0))
                    .map_err(db_err)?;
                if ok == 0 {
                    return Err(AppError::Validation(format!("{level} region not found")));
                }
            }
        }
        if body.is_default {
            tx.execute(
                "UPDATE ecs_user_address SET is_default = 0 WHERE user_id = ?1",
                [user_id],
            )
            .map_err(db_err)?;
        }
        tx.execute(
            "INSERT INTO ecs_user_address (user_id, consignee, country_id, province_id, city_id, district_id, address, zipcode, mobile, is_default)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                user_id, body.consignee, body.country_id, body.province_id, body.city_id,
                body.district_id, body.address, body.zipcode, body.mobile,
                if body.is_default { 1 } else { 0 }
            ],
        )
        .map_err(db_err)?;
        let address_id = tx.last_insert_rowid();
        tx.commit().map_err(db_err)?;
        Ok(json!({"id": address_id}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

/// GET /api/v1/me/addresses
pub async fn address_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT address_id, consignee, country_id, province_id, city_id, district_id, address, zipcode, mobile, is_default
                 FROM ecs_user_address WHERE user_id = ?1 ORDER BY is_default DESC, address_id",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "consignee": r.get::<_, String>(1)?,
                    "country_id": r.get::<_, i64>(2)?,
                    "province_id": r.get::<_, i64>(3)?,
                    "city_id": r.get::<_, i64>(4)?,
                    "district_id": r.get::<_, i64>(5)?,
                    "address": r.get::<_, String>(6)?,
                    "zipcode": r.get::<_, String>(7)?,
                    "mobile": r.get::<_, String>(8)?,
                    "is_default": r.get::<_, i64>(9)? != 0,
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

/// PATCH /api/v1/me/addresses/{id}
pub async fn address_update(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<AddressRequest>,
) -> Result<StatusCode, AppError> {
    let address_id = parse_id(&id)?;
    let db = state.db.clone();
    let user_id = auth.user_id;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        if body.is_default {
            tx.execute(
                "UPDATE ecs_user_address SET is_default = 0 WHERE user_id = ?1",
                [user_id],
            )
            .map_err(db_err)?;
        }
        let updated = tx
            .execute(
                "UPDATE ecs_user_address SET consignee = ?1, country_id = ?2, province_id = ?3, city_id = ?4,
                 district_id = ?5, address = ?6, zipcode = ?7, mobile = ?8, is_default = ?9
                 WHERE address_id = ?10 AND user_id = ?11",
                rusqlite::params![
                    body.consignee, body.country_id, body.province_id, body.city_id, body.district_id,
                    body.address, body.zipcode, body.mobile,
                    if body.is_default { 1 } else { 0 },
                    address_id, user_id
                ],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(AppError::NotFound("address not found".to_string()));
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/me/addresses/{id}
pub async fn address_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let address_id = parse_id(&id)?;
    let db = state.db.clone();
    let user_id = auth.user_id;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        let deleted = conn
            .execute(
                "DELETE FROM ecs_user_address WHERE address_id = ?1 AND user_id = ?2",
                [address_id, user_id],
            )
            .map_err(db_err)?;
        if deleted == 0 {
            return Err(AppError::NotFound("address not found".to_string()));
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let goods_id = body.goods_id;
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let ok: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM ecs_goods WHERE goods_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                [goods_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if ok == 0 {
            return Err(AppError::NotFound("goods not found".to_string()));
        }
        let exists: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM ecs_collect_goods WHERE user_id = ?1 AND goods_id = ?2",
                [user_id, goods_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if exists > 0 {
            return Err(AppError::Conflict("goods already in favorites".to_string()));
        }
        tx.execute(
            "INSERT INTO ecs_collect_goods (user_id, goods_id, add_time) VALUES (?1, ?2, ?3)",
            [user_id, goods_id, now],
        )
        .map_err(db_err)?;
        let rec_id = tx.last_insert_rowid();
        tx.commit().map_err(db_err)?;
        Ok(json!({"id": rec_id, "goods_id": goods_id, "attention": false}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

/// GET /api/v1/me/favorites
pub async fn favorite_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT c.rec_id, c.goods_id, g.goods_name, g.shop_price, g.goods_thumb, c.is_attention
                 FROM ecs_collect_goods c JOIN ecs_goods g ON g.goods_id = c.goods_id
                 WHERE c.user_id = ?1 ORDER BY c.rec_id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                let price: f64 = r.get(3)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "goods_id": r.get::<_, i64>(1)?,
                    "name": r.get::<_, String>(2)?,
                    "price": cents_to_string((price * 100.0).round() as i64),
                    "thumb": r.get::<_, String>(4)?,
                    "attention": r.get::<_, i64>(5)? != 0,
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        let updated = conn
            .execute(
                "UPDATE ecs_collect_goods SET is_attention = ?1 WHERE rec_id = ?2 AND user_id = ?3",
                rusqlite::params![if body.attention { 1 } else { 0 }, rec_id, user_id],
            )
            .map_err(db_err)?;
        if updated == 0 {
            return Err(AppError::NotFound("favorite not found".to_string()));
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// DELETE /api/v1/me/favorites/{id}
pub async fn favorite_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    let db = state.db.clone();
    let user_id = auth.user_id;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        let deleted = conn
            .execute(
                "DELETE FROM ecs_collect_goods WHERE rec_id = ?1 AND user_id = ?2",
                [rec_id, user_id],
            )
            .map_err(db_err)?;
        if deleted == 0 {
            return Err(AppError::NotFound("favorite not found".to_string()));
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let ok: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM ecs_goods WHERE goods_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                [body.goods_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if ok == 0 {
            return Err(AppError::NotFound("goods not found".to_string()));
        }
        // Unique (user, goods) prevents TOCTOU duplicates.
        let inserted = tx
            .execute(
                "INSERT INTO ecs_booking_goods (user_id, email, link_man, tel, goods_id, goods_desc, goods_number, booking_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                rusqlite::params![
                    user_id, body.email, body.linkman, body.telephone, body.goods_id,
                    body.description, body.goods_number, now
                ],
            );
        match inserted {
            Ok(_) => {}
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
                return Err(AppError::Conflict("booking already exists for this goods".to_string()));
            }
            Err(e) => return Err(db_err(e)),
        }
        let rec_id = tx.last_insert_rowid();
        tx.commit().map_err(db_err)?;
        Ok(json!({"id": rec_id}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

/// GET /api/v1/me/bookings
pub async fn booking_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT b.rec_id, b.goods_id, g.goods_name, b.goods_number, b.booking_time, b.is_dispose, b.dispose_note
                 FROM ecs_booking_goods b JOIN ecs_goods g ON g.goods_id = b.goods_id
                 WHERE b.user_id = ?1 ORDER BY b.booking_time DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "goods_id": r.get::<_, i64>(1)?,
                    "goods_name": r.get::<_, String>(2)?,
                    "goods_number": r.get::<_, i64>(3)?,
                    "booking_time": r.get::<_, i64>(4)?,
                    "is_dispose": r.get::<_, i64>(5)? != 0,
                    "dispose_note": r.get::<_, String>(6)?,
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

/// DELETE /api/v1/me/bookings/{id}
pub async fn booking_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    let db = state.db.clone();
    let user_id = auth.user_id;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        let deleted = conn
            .execute(
                "DELETE FROM ecs_booking_goods WHERE rec_id = ?1 AND user_id = ?2",
                [rec_id, user_id],
            )
            .map_err(db_err)?;
        if deleted == 0 {
            return Err(AppError::NotFound("booking not found".to_string()));
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let now = unix_now();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let row: Option<i64> = tx
            .query_row(
                "SELECT b.bonus_id FROM ecs_user_bonus b JOIN ecs_bonus_type t ON t.type_id = b.bonus_type_id
                 WHERE b.bonus_sn = ?1",
                [sn_value],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let Some(bonus_id) = row else {
            return Err(AppError::NotFound("bonus not found".to_string()));
        };
        let use_end: i64 = tx
            .query_row(
                "SELECT t.use_end_date FROM ecs_user_bonus b JOIN ecs_bonus_type t ON t.type_id = b.bonus_type_id WHERE b.bonus_id = ?1",
                [bonus_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if now > use_end {
            return Err(AppError::Conflict("bonus is expired".to_string()));
        }
        // Atomic claim: user_id=0 is the unclaimed marker.
        let claimed = tx
            .execute(
                "UPDATE ecs_user_bonus SET user_id = ?1 WHERE bonus_id = ?2 AND user_id = 0",
                [user_id, bonus_id],
            )
            .map_err(db_err)?;
        if claimed == 0 {
            return Err(AppError::Conflict("bonus already claimed".to_string()));
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/me/bonuses
pub async fn bonus_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let now = unix_now();
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT b.bonus_id, b.bonus_sn, t.type_name, t.type_money, t.use_start_date, t.use_end_date, b.order_id, b.used_time
                 FROM ecs_user_bonus b JOIN ecs_bonus_type t ON t.type_id = b.bonus_type_id
                 WHERE b.user_id = ?1 ORDER BY b.bonus_id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                let money: f64 = r.get(3)?;
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    (money * 100.0).round() as i64,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                ))
            })
            .map_err(db_err)?;
        let rows: Vec<(i64, i64, String, i64, i64, i64, i64, i64)> =
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        let items: Vec<Value> = rows
            .into_iter()
            .map(|(id, sn, name, money, use_start, use_end, order_id, used_time)| {
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
                    "id": id,
                    "bonus_sn": sn.to_string(),
                    "type_name": name,
                    "amount": cents_to_string(money),
                    "status": status,
                    "order_id": if used_time > 0 { Some(order_id) } else { None },
                })
            })
            .collect();
        Ok(json!({"items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let email: String = tx
            .query_row(
                "SELECT email FROM ecs_users WHERE user_id = ?1",
                [user_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        // Duplicate requests atomically replace the old token.
        tx.execute(
            "INSERT INTO ecs_email_verification_tokens (user_id, token_hash, expires_at, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(user_id) DO UPDATE SET token_hash = ?2, expires_at = ?3, created_at = ?4, consumed_at = NULL",
            rusqlite::params![user_id, token_hash, now + 86_400, now],
        )
        .map_err(db_err)?;
        insert_outbox(&tx, Some(user_id), &email, "verify_email", &json!({"token": token}))
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(json!({"status": "queued"}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::ACCEPTED, Json(result)))
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
    let db = state.db.clone();
    let now = unix_now();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let row: Option<i64> = tx
            .query_row(
                "SELECT user_id FROM ecs_email_verification_tokens
                 WHERE token_hash = ?1 AND consumed_at IS NULL AND expires_at > ?2",
                rusqlite::params![token_hash, now],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let Some(user_id) = row else {
            return Err(AppError::Conflict("invalid or expired token".to_string()));
        };
        tx.execute(
            "INSERT OR IGNORE INTO ecs_email_verified_users (user_id, verified_at) VALUES (?1, ?2)",
            [user_id, now],
        )
        .map_err(db_err)?;
        let consumed = tx
            .execute(
                "UPDATE ecs_email_verification_tokens SET consumed_at = ?1 WHERE token_hash = ?2 AND consumed_at IS NULL",
                rusqlite::params![now, token_hash],
            )
            .map_err(db_err)?;
        if consumed != 1 {
            return Err(AppError::Conflict("token already consumed".to_string()));
        }
        tx.execute("UPDATE ecs_users SET is_validated = 1 WHERE user_id = ?1", [user_id])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
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
    let db = state.db.clone();
    let email = body.email;
    let now = unix_now();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let user: Option<i64> = tx
            .query_row(
                "SELECT user_id FROM ecs_users WHERE email = ?1",
                [&email],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        if let Some(user_id) = user {
            let token = crypto::random_token(32);
            let token_hash = crypto::sha256_hex(token.as_bytes());
            tx.execute("DELETE FROM ecs_password_reset_tokens WHERE user_id = ?1", [user_id])
                .map_err(db_err)?;
            tx.execute(
                "INSERT INTO ecs_password_reset_tokens (user_id, token_hash, expires_at, created_at) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![user_id, token_hash, now + 3_600, now],
            )
            .map_err(db_err)?;
            insert_outbox(&tx, Some(user_id), &email, "password_reset", &json!({"token": token}))
                .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
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
    let db = state.db.clone();
    let now = unix_now();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let row: Option<i64> = tx
            .query_row(
                "SELECT user_id FROM ecs_password_reset_tokens
                 WHERE token_hash = ?1 AND consumed_at IS NULL AND expires_at > ?2",
                rusqlite::params![token_hash, now],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let Some(user_id) = row else {
            return Err(AppError::Conflict("invalid or expired token".to_string()));
        };
        let consumed = tx
            .execute(
                "UPDATE ecs_password_reset_tokens SET consumed_at = ?1 WHERE token_hash = ?2 AND consumed_at IS NULL",
                rusqlite::params![now, token_hash],
            )
            .map_err(db_err)?;
        if consumed != 1 {
            return Err(AppError::Conflict("token already consumed".to_string()));
        }
        tx.execute(
            "UPDATE ecs_users SET password_hash = ?1 WHERE user_id = ?2",
            rusqlite::params![new_hash, user_id],
        )
        .map_err(db_err)?;
        tx.execute("DELETE FROM ecs_sessions WHERE user_id = ?1", [user_id])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
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
    let db = state.db.clone();
    let now = unix_now();
    let action = action.to_string();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let existing: Option<(i64, Option<String>)> = tx
            .query_row(
                "SELECT status, token_hash FROM ecs_email_list WHERE email = ?1",
                [&email],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let token = crypto::random_token(32);
        let token_hash = crypto::sha256_hex(token.as_bytes());
        match existing {
            Some((status, _)) => {
                let subscribed = action == "subscribe";
                // Idempotent: already subscribed/unsubscribed => accept without a new mail.
                if (subscribed && status == 1) || (!subscribed && status == 0) {
                    tx.commit().map_err(db_err)?;
                    return Ok(());
                }
                tx.execute(
                    "UPDATE ecs_email_list SET token_hash = ?1, pending_action = ?2, token_expires_at = ?3, updated_at = ?4 WHERE email = ?5",
                    rusqlite::params![token_hash, action, now + 86_400, now, email],
                )
                .map_err(db_err)?;
            }
            None => {
                tx.execute(
                    "INSERT INTO ecs_email_list (email, status, token_hash, pending_action, token_expires_at, updated_at)
                     VALUES (?1, 0, ?2, ?3, ?4, ?5)",
                    rusqlite::params![email, token_hash, action, now + 86_400, now],
                )
                .map_err(db_err)?;
            }
        }
        let template = if action == "subscribe" { "newsletter_subscribe" } else { "newsletter_unsubscribe" };
        insert_outbox(&tx, None, &email, template, &json!({"token": token})).map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
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
    let db = state.db.clone();
    let now = unix_now();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        let updated = conn
            .execute(
                "UPDATE ecs_email_list
                 SET status = CASE WHEN ?1 = 'subscribe' THEN 1 ELSE 0 END,
                     pending_action = '', token_hash = NULL, token_expires_at = NULL, updated_at = ?2
                 WHERE token_hash = ?3 AND pending_action = ?1 AND token_expires_at > ?2",
                rusqlite::params![pending, now, token_hash],
            )
            .map_err(db_err)?;
        if updated != 1 {
            return Err(AppError::Conflict("invalid or expired token".to_string()));
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let now = unix_now();
    let kind = body.kind;
    let payment_id = body.payment_id;
    let note = body.note;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let status = match kind.as_str() {
            "deposit" => {
                let pid = payment_id
                    .ok_or_else(|| AppError::Validation("payment_id is required for deposits".to_string()))?;
                let enabled: i64 = tx
                    .query_row("SELECT enabled FROM ecs_payment WHERE pay_id = ?1", [pid], |r| r.get(0))
                    .optional()
                    .map_err(db_err)?
                    .ok_or_else(|| AppError::NotFound("payment not found".to_string()))?;
                if enabled != 1 {
                    return Err(AppError::NotFound("payment not enabled".to_string()));
                }
                "pending_payment"
            }
            _ => {
                // Withdrawal: atomically freeze available balance.
                let updated = tx
                    .execute(
                        "UPDATE ecs_account_balance SET available_cents = available_cents - ?1, frozen_cents = frozen_cents + ?1
                         WHERE user_id = ?2 AND available_cents >= ?1",
                        rusqlite::params![amount_cents, user_id],
                    )
                    .map_err(db_err)?;
                if updated == 0 {
                    return Err(AppError::Conflict("insufficient balance".to_string()));
                }
                tx.execute(
                    "INSERT INTO ecs_account_log (user_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at)
                     VALUES (?1, ?2, ?3, 'withdrawal_freeze', 'user_account', 0, ?4)",
                    rusqlite::params![user_id, -amount_cents, amount_cents, now],
                )
                .map_err(db_err)?;
                "pending_review"
            }
        };
        tx.execute(
            "INSERT INTO ecs_user_account (user_id, amount_cents, process_type, payment_id, user_note, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![user_id, amount_cents, kind, payment_id, note, status, now],
        )
        .map_err(db_err)?;
        let rec_id = tx.last_insert_rowid();
        tx.commit().map_err(db_err)?;
        Ok(json!({
            "id": rec_id,
            "kind": kind,
            "amount": cents_to_string(amount_cents),
            "status": status,
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

/// GET /api/v1/me/account/requests
pub async fn account_request_list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let balance: Option<(i64, i64)> = conn
            .query_row(
                "SELECT available_cents, frozen_cents FROM ecs_account_balance WHERE user_id = ?1",
                [user_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let (available, frozen) = balance.unwrap_or((0, 0));
        let mut stmt = conn
            .prepare(
                "SELECT rec_id, process_type, amount_cents, payment_id, status, created_at, paid_at
                 FROM ecs_user_account WHERE user_id = ?1 ORDER BY rec_id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "kind": r.get::<_, String>(1)?,
                    "amount": cents_to_string(r.get::<_, i64>(2)?),
                    "payment_id": r.get::<_, Option<i64>>(3)?,
                    "status": r.get::<_, String>(4)?,
                    "created_at": r.get::<_, i64>(5)?,
                    "paid_at": r.get::<_, Option<i64>>(6)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({
            "available_balance": cents_to_string(available),
            "frozen_balance": cents_to_string(frozen),
            "items": items,
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// DELETE /api/v1/me/account/requests/{id} — cancels unprocessed requests, unfreezes withdrawals.
pub async fn account_request_cancel(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let rec_id = parse_id(&id)?;
    let db = state.db.clone();
    let user_id = auth.user_id;
    let now = unix_now();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let row: Option<(String, i64)> = tx
            .query_row(
                "SELECT process_type, amount_cents FROM ecs_user_account
                 WHERE rec_id = ?1 AND user_id = ?2 AND ((process_type = 'deposit' AND status = 'pending_payment')
                    OR (process_type = 'withdrawal' AND status = 'pending_review'))",
                [rec_id, user_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let Some((kind, amount)) = row else {
            return Err(AppError::NotFound("cancellable request not found".to_string()));
        };
        if kind == "withdrawal" {
            tx.execute(
                "UPDATE ecs_account_balance SET frozen_cents = frozen_cents - ?1, available_cents = available_cents + ?1
                 WHERE user_id = ?2",
                rusqlite::params![amount, user_id],
            )
            .map_err(db_err)?;
            tx.execute(
                "INSERT INTO ecs_account_log (user_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at)
                 VALUES (?1, ?2, ?3, 'withdrawal_cancel_unfreeze', 'user_account', ?4, ?5)",
                rusqlite::params![user_id, amount, -amount, rec_id, now],
            )
            .map_err(db_err)?;
        }
        tx.execute("DELETE FROM ecs_user_account WHERE rec_id = ?1", [rec_id])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
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
    let db = state.db.clone();
    let user_id = auth.user_id;
    let now = unix_now();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let row: Option<(i64, String)> = tx
            .query_row(
                "SELECT amount_cents, status FROM ecs_user_account
                 WHERE rec_id = ?1 AND user_id = ?2 AND process_type = 'deposit' AND status = 'pending_payment'",
                [rec_id, user_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let Some((amount, _status)) = row else {
            return Err(AppError::NotFound("pending deposit not found".to_string()));
        };
        let fee_cents: i64 = tx
            .query_row(
                "SELECT pay_fee FROM ecs_payment WHERE pay_id = ?1 AND enabled = 1",
                [body.payment_id],
                |r| r.get::<_, f64>(0).map(|f| (f * 100.0).round() as i64),
            )
            .optional()
            .map_err(db_err)?
            .ok_or_else(|| AppError::NotFound("payment not found or disabled".to_string()))?;
        // Idempotent: one intent per request; repeat calls update the same row.
        tx.execute(
            "INSERT INTO ecs_account_payment_intent (request_id, user_id, payment_id, amount_cents, fee_cents, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?6)
             ON CONFLICT(request_id) DO UPDATE SET payment_id = ?3, fee_cents = ?5, updated_at = ?6",
            rusqlite::params![rec_id, user_id, body.payment_id, amount, fee_cents, now],
        )
        .map_err(db_err)?;
        let intent_id = tx.last_insert_rowid();
        tx.commit().map_err(db_err)?;
        Ok(json!({
            "id": intent_id,
            "request_id": rec_id,
            "payment_id": body.payment_id,
            "amount": cents_to_string(amount),
            "fee": cents_to_string(fee_cents),
            "total": cents_to_string(amount + fee_cents),
            "status": "pending",
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// GET /api/v1/me/account/transactions
pub async fn account_transactions(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let balance: Option<(i64, i64)> = conn
            .query_row(
                "SELECT available_cents, frozen_cents FROM ecs_account_balance WHERE user_id = ?1",
                [user_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let (available, frozen) = balance.unwrap_or((0, 0));
        let mut stmt = conn
            .prepare(
                "SELECT log_id, available_delta_cents, frozen_delta_cents, reason, reference_type, reference_id, created_at
                 FROM ecs_account_log WHERE user_id = ?1 ORDER BY log_id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                let available_delta: i64 = r.get(1)?;
                let frozen_delta: i64 = r.get(2)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "available_delta": format_signed_cents(available_delta),
                    "frozen_delta": format_signed_cents(frozen_delta),
                    "reason": r.get::<_, String>(3)?,
                    "reference_type": r.get::<_, String>(4)?,
                    "reference_id": r.get::<_, i64>(5)?,
                    "created_at": r.get::<_, i64>(6)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({
            "available_balance": cents_to_string(available),
            "frozen_balance": cents_to_string(frozen),
            "items": items,
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

fn format_signed_cents(cents: i64) -> String {
    if cents >= 0 {
        cents_to_string(cents)
    } else {
        format!("-{}", cents_to_string(-cents))
    }
}
