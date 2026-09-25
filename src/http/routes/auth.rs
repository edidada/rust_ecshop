//! Auth context: register, availability checks, login, logout, me profile, password changes.
use rbatis::executor::Executor;
use serde_json::{json, Value};

use crate::http::auth::{insert_session_tx, revoke_user_sessions, AuthUser};
use crate::http::state::AppState;
use crate::infrastructure::crypto;
use crate::shared::error::AppError;
use crate::shared::rbutil::{col_i64, col_str, col_opt_str, e, insert_id, q1};

/// Insert a transactional email outbox row.
pub async fn insert_outbox<E: Executor>(
    tx: &E,
    user_id: Option<i64>,
    recipient: &str,
    template: &str,
    payload: &Value,
) -> Result<(), AppError> {
    e(
        tx,
        "INSERT INTO ecs_email_outbox (user_id, recipient, template_name, payload, status) VALUES (?, ?, ?, ?, 'pending')",
        vec![
            json!(user_id),
            json!(recipient),
            json!(template),
            json!(payload.to_string()),
        ],
    )
    .await?;
    Ok(())
}

fn validate_password(password: &str) -> Result<(), AppError> {
    let len = password.len();
    if !(8..=1024).contains(&len) {
        return Err(AppError::Validation("password must be 8-1024 bytes".to_string()));
    }
    Ok(())
}

#[derive(serde::Deserialize)]
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
    let row = q1(
        state.rb,
        "SELECT COUNT(*) AS cnt FROM ecs_users WHERE user_name = ?",
        vec![json!(body.username)],
    )
    .await?;
    let available = col_i64(&row.unwrap_or(json!({"cnt": 1})), "cnt") == 0;
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
    let row = q1(
        state.rb,
        "SELECT COUNT(*) AS cnt FROM ecs_users WHERE email = ?",
        vec![json!(body.email)],
    )
    .await?;
    let available = col_i64(&row.unwrap_or(json!({"cnt": 1})), "cnt") == 0;
    Ok(Json(json!({"available": available})))
}

#[derive(serde::Deserialize)]
pub struct RegisterRequest {
    username: String,
    email: String,
    password: String,
    #[serde(default)]
    agreement_accepted: bool,
}

/// POST /api/v1/auth/register — creates user + session in one transaction.
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

    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let conflict = q1(
        &tx,
        "SELECT COUNT(*) AS cnt FROM ecs_users WHERE user_name = ? OR email = ?",
        vec![json!(body.username), json!(body.email)],
    )
    .await?;
    if col_i64(&conflict.unwrap_or_default(), "cnt") > 0 {
        return Err(AppError::Conflict("username or email already registered".to_string()));
    }
    let result = e(
        &tx,
        "INSERT INTO ecs_users (user_name, email, password_hash) VALUES (?, ?, ?)",
        vec![json!(body.username), json!(body.email), json!(password_hash)],
    )
    .await?;
    let user_id = insert_id(&result);
    let token = crypto::random_token(32);
    let token_hash = crypto::sha256_hex(token.as_bytes());
    insert_session_tx(&tx, &token_hash, user_id).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"user_id": user_id, "username": body.username, "access_token": token})),
    ))
}

#[derive(serde::Deserialize)]
pub struct LoginRequest {
    username: String,
    password: String,
}

/// POST /api/v1/auth/login — verifies credentials and opens a session transactionally.
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<Value>, AppError> {
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let row = q1(
        &tx,
        "SELECT user_id AS user_id, password_hash AS password_hash FROM ecs_users WHERE user_name = ?",
        vec![json!(body.username)],
    )
    .await?;
    let Some(row) = row else {
        return Err(AppError::Unauthenticated);
    };
    let user_id = col_i64(&row, "user_id");
    let stored_hash = col_str(&row, "password_hash");
    if !crypto::verify_password(&body.password, &stored_hash) {
        return Err(AppError::Unauthenticated);
    }
    let token = crypto::random_token(32);
    let token_hash = crypto::sha256_hex(token.as_bytes());
    insert_session_tx(&tx, &token_hash, user_id).await?;
    tx.commit().await?;
    Ok(Json(json!({"user_id": user_id, "username": body.username, "access_token": token})))
}

/// POST /api/v1/auth/logout — invalidates the current session.
pub async fn logout(
    State(state): State<AppState>,
    auth: AuthUser,
    headers: axum::http::HeaderMap,
) -> Result<StatusCode, AppError> {
    let token = crate::http::auth::extract_bearer_token(&headers).unwrap_or_default();
    let token_hash = crypto::sha256_hex(token.as_bytes());
    e(
        state.rb,
        "DELETE FROM ecs_sessions WHERE token_hash = ? AND user_id = ?",
        vec![json!(token_hash), json!(auth.user_id)],
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/me
pub async fn me(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let row = q1(
        state.rb,
        "SELECT user_id AS user_id, user_name AS user_name, email AS email, mobile_phone AS mobile_phone, is_validated AS is_validated
         FROM ecs_users WHERE user_id = ?",
        vec![json!(auth.user_id)],
    )
    .await?
    .ok_or(AppError::Unauthenticated)?;
    Ok(Json(json!({
        "id": col_i64(&row, "user_id"),
        "username": col_str(&row, "user_name"),
        "email": col_str(&row, "email"),
        "mobile": col_str(&row, "mobile_phone"),
        "email_verified": col_i64(&row, "is_validated") != 0,
    })))
}

#[derive(serde::Deserialize)]
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
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    if let Some(email) = &body.email {
        let conflict = q1(
            &tx,
            "SELECT COUNT(*) AS cnt FROM ecs_users WHERE email = ? AND user_id <> ?",
            vec![json!(email), json!(auth.user_id)],
        )
        .await?;
        if col_i64(&conflict.unwrap_or_default(), "cnt") > 0 {
            return Err(AppError::Conflict("email already used".to_string()));
        }
        e(
            &tx,
            "UPDATE ecs_users SET email = ? WHERE user_id = ?",
            vec![json!(email), json!(auth.user_id)],
        )
        .await?;
    }
    if let Some(mobile) = &body.mobile {
        e(
            &tx,
            "UPDATE ecs_users SET mobile_phone = ? WHERE user_id = ?",
            vec![json!(mobile), json!(auth.user_id)],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(serde::Deserialize)]
pub struct PasswordPatchRequest {
    current_password: String,
    new_password: String,
}

/// PATCH /api/v1/me/password — verifies old password, re-hashes and revokes sessions in one tx.
pub async fn me_password_patch(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<PasswordPatchRequest>,
) -> Result<StatusCode, AppError> {
    validate_password(&body.new_password)?;
    let new_hash = crypto::hash_password(&body.new_password);
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let row = q1(
        &tx,
        "SELECT password_hash AS password_hash FROM ecs_users WHERE user_id = ?",
        vec![json!(auth.user_id)],
    )
    .await?
    .ok_or(AppError::Unauthenticated)?;
    let stored_hash = col_str(&row, "password_hash");
    if !crypto::verify_password(&body.current_password, &stored_hash) {
        return Err(AppError::Conflict("current password is wrong".to_string()));
    }
    // Conditional update prevents TOCTOU between verify and write.
    let updated = e(
        &tx,
        "UPDATE ecs_users SET password_hash = ? WHERE user_id = ? AND password_hash = ?",
        vec![json!(new_hash), json!(auth.user_id), json!(stored_hash)],
    )
    .await?;
    if updated.rows_affected != 1 {
        return Err(AppError::Conflict("password changed concurrently".to_string()));
    }
    revoke_user_sessions(&tx, auth.user_id).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[allow(dead_code)]
fn _keep_imports() -> Option<String> {
    col_opt_str(&json!({}), "x")
}

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
