//! Auth context: register, availability checks, login, logout, me profile, password changes.
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::state::AppState;
use crate::infrastructure::crypto;
use crate::shared::error::AppError;
use crate::shared::util::unix_now;

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

fn validate_password(password: &str) -> Result<(), AppError> {
    let len = password.len();
    if !(8..=1024).contains(&len) {
        return Err(AppError::Validation("password must be 8-1024 bytes".to_string()));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct AvailabilityRequest {
    #[serde(default)]
    username: String,
    #[serde(default)]
    email: String,
}

/// POST /api/v1/auth/availability/username
pub async fn username_availability(
    State(state): State<AppState>,
    Json(body): Json<AvailabilityRequest>,
) -> Result<Json<Value>, AppError> {
    if body.username.is_empty() || body.username.len() > 60 {
        return Err(AppError::Validation("username must be 1-60 bytes".to_string()));
    }
    let db = state.db.clone();
    let username = body.username;
    let available = tokio::task::spawn_blocking(move || -> Result<bool, AppError> {
        let conn = db.blocking_lock();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ecs_users WHERE user_name = ?1",
                [&username],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        Ok(exists == 0)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(json!({"available": available})))
}

/// POST /api/v1/auth/availability/email
pub async fn email_availability(
    State(state): State<AppState>,
    Json(body): Json<AvailabilityRequest>,
) -> Result<Json<Value>, AppError> {
    if body.email.is_empty() || body.email.len() > 120 {
        return Err(AppError::Validation("email must be 1-120 bytes".to_string()));
    }
    let db = state.db.clone();
    let email = body.email;
    let available = tokio::task::spawn_blocking(move || -> Result<bool, AppError> {
        let conn = db.blocking_lock();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ecs_users WHERE email = ?1",
                [&email],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        Ok(exists == 0)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(json!({"available": available})))
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    username: String,
    email: String,
    password: String,
    #[serde(default)]
    agreement_accepted: bool,
}

/// POST /api/v1/auth/register — creates user + session, returns Bearer token once.
pub async fn register(
    State(state): State<AppState>,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    if !body.agreement_accepted {
        return Err(AppError::Validation("agreement must be accepted".to_string()));
    }
    if body.username.is_empty() || body.username.len() > 60 {
        return Err(AppError::Validation("username must be 1-60 bytes".to_string()));
    }
    if body.email.is_empty() || body.email.len() > 120 {
        return Err(AppError::Validation("email must be 1-120 bytes".to_string()));
    }
    validate_password(&body.password)?;
    let password_hash = crypto::hash_password(&body.password);
    let db = state.db.clone();
    let username = body.username;
    let email = body.email;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let exists: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM ecs_users WHERE user_name = ?1 OR email = ?2",
                rusqlite::params![username, email],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if exists > 0 {
            return Err(AppError::Conflict("username or email already registered".to_string()));
        }
        tx.execute(
            "INSERT INTO ecs_users (user_name, email, password_hash) VALUES (?1, ?2, ?3)",
            rusqlite::params![username, email, password_hash],
        )
        .map_err(db_err)?;
        let user_id = tx.last_insert_rowid();
        let token = crypto::random_token(32);
        let token_hash = crypto::sha256_hex(token.as_bytes());
        tx.execute(
            "INSERT INTO ecs_sessions (token_hash, user_id) VALUES (?1, ?2)",
            rusqlite::params![token_hash, user_id],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(json!({"user_id": user_id, "username": username, "access_token": token}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

#[derive(Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

/// POST /api/v1/auth/login
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let username = body.username;
    let password = body.password;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let row: Option<(i64, String)> = tx
            .query_row(
                "SELECT user_id, password_hash FROM ecs_users WHERE user_name = ?1",
                [&username],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_err)?;
        let Some((user_id, stored_hash)) = row else {
            return Err(AppError::Unauthenticated);
        };
        if !crypto::verify_password(&password, &stored_hash) {
            return Err(AppError::Unauthenticated);
        }
        let token = crypto::random_token(32);
        let token_hash = crypto::sha256_hex(token.as_bytes());
        tx.execute(
            "INSERT INTO ecs_sessions (token_hash, user_id) VALUES (?1, ?2)",
            rusqlite::params![token_hash, user_id],
        )
        .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(json!({"user_id": user_id, "username": username, "access_token": token}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// POST /api/v1/auth/logout — invalidates the current session.
pub async fn logout(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, AppError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .unwrap_or("")
        .trim()
        .to_string();
    let token_hash = crypto::sha256_hex(token.as_bytes());
    let db = state.db.clone();
    let user_id = auth.user_id;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        conn.execute(
            "DELETE FROM ecs_sessions WHERE token_hash = ?1 AND user_id = ?2",
            rusqlite::params![token_hash, user_id],
        )
        .map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/me
pub async fn me(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let row = conn
            .query_row(
                "SELECT user_id, user_name, email, mobile_phone, is_validated FROM ecs_users WHERE user_id = ?1",
                [user_id],
                |r| {
                    Ok(json!({
                        "id": r.get::<_, i64>(0)?,
                        "username": r.get::<_, String>(1)?,
                        "email": r.get::<_, String>(2)?,
                        "mobile": r.get::<_, String>(3)?,
                        "email_verified": r.get::<_, i64>(4)? != 0,
                    }))
                },
            )
            .optional()
            .map_err(db_err)?;
        row.ok_or_else(|| AppError::Unauthenticated)
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct ProfilePatchRequest {
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    mobile: Option<String>,
}

/// PATCH /api/v1/me
pub async fn me_patch(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<ProfilePatchRequest>,
) -> Result<StatusCode, AppError> {
    if let Some(email) = &body.email {
        if email.is_empty() || email.len() > 120 {
            return Err(AppError::Validation("email must be 1-120 bytes".to_string()));
        }
    }
    let db = state.db.clone();
    let user_id = auth.user_id;
    let email = body.email;
    let mobile = body.mobile;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        if let Some(email) = &email {
            let conflict: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM ecs_users WHERE email = ?1 AND user_id <> ?2",
                    rusqlite::params![email, user_id],
                    |r| r.get(0),
                )
                .map_err(db_err)?;
            if conflict > 0 {
                return Err(AppError::Conflict("email already used".to_string()));
            }
            tx.execute(
                "UPDATE ecs_users SET email = ?1 WHERE user_id = ?2",
                rusqlite::params![email, user_id],
            )
            .map_err(db_err)?;
        }
        if let Some(mobile) = &mobile {
            tx.execute(
                "UPDATE ecs_users SET mobile_phone = ?1 WHERE user_id = ?2",
                rusqlite::params![mobile, user_id],
            )
            .map_err(db_err)?;
        }
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct PasswordPatchRequest {
    current_password: String,
    new_password: String,
}

/// PATCH /api/v1/me/password — verifies old password, atomically re-hashes and revokes sessions.
pub async fn me_password_patch(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<PasswordPatchRequest>,
) -> Result<StatusCode, AppError> {
    validate_password(&body.new_password)?;
    let db = state.db.clone();
    let user_id = auth.user_id;
    let current = body.current_password;
    let new_hash = crypto::hash_password(&body.new_password);
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let stored: Option<String> = tx
            .query_row(
                "SELECT password_hash FROM ecs_users WHERE user_id = ?1",
                [user_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        let Some(stored) = stored else {
            return Err(AppError::Unauthenticated);
        };
        if !crypto::verify_password(&current, &stored) {
            return Err(AppError::Conflict("current password is wrong".to_string()));
        }
        // Conditional update prevents TOCTOU between verify and write.
        let updated = tx
            .execute(
                "UPDATE ecs_users SET password_hash = ?1 WHERE user_id = ?2 AND password_hash = ?3",
                rusqlite::params![new_hash, user_id, stored],
            )
            .map_err(db_err)?;
        if updated != 1 {
            return Err(AppError::Conflict("password changed concurrently".to_string()));
        }
        tx.execute("DELETE FROM ecs_sessions WHERE user_id = ?1", [user_id])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// Helper used by other modules to insert a transactional email outbox row.
pub fn insert_outbox(
    tx: &rusqlite::Transaction,
    user_id: Option<i64>,
    recipient: &str,
    template: &str,
    payload: &Value,
) -> Result<i64, rusqlite::Error> {
    tx.execute(
        "INSERT INTO ecs_email_outbox (user_id, recipient, template_name, payload, status) VALUES (?1, ?2, ?3, ?4, 'pending')",
        rusqlite::params![user_id, recipient, template, payload.to_string()],
    )?;
    Ok(tx.last_insert_rowid())
}

/// Unused placeholder to keep IntoResponse import; remove if lints complain.
pub fn _unused_into_response() -> impl IntoResponse {
    StatusCode::OK
}

#[allow(dead_code)]
fn _now_unused() -> i64 {
    unix_now()
}
