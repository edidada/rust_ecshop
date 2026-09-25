use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use rusqlite::OptionalExtension;

use crate::http::state::AppState;
use crate::shared::error::AppError;

/// Authenticated user resolved from the Bearer token session.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i64,
    pub user_name: String,
}

pub async fn authenticate(state: &AppState, headers: &axum::http::HeaderMap) -> Result<AuthUser, AppError> {
    let token = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(AppError::Unauthenticated)?;
    let token = token.trim();
    if token.is_empty() {
        return Err(AppError::Unauthenticated);
    }
    let token_hash = crate::infrastructure::crypto::sha256_hex(token.as_bytes());
    let db = state.db.clone();
    let row = tokio::task::spawn_blocking(move || -> Result<Option<(i64, String)>, rusqlite::Error> {
        let conn = db.blocking_lock();
        conn.query_row(
            "SELECT u.user_id, u.user_name FROM ecs_sessions s JOIN ecs_users u ON u.user_id = s.user_id WHERE s.token_hash = ?1",
            [&token_hash],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))?
    .map_err(|e| AppError::Internal(e.into()))?;
    row.map(|(user_id, user_name)| AuthUser { user_id, user_name })
        .ok_or(AppError::Unauthenticated)
}

#[axum::async_trait]
impl<S: Send + Sync> FromRequestParts<S> for AuthUser
where
    S: Send + Sync,
{
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let state = parts
            .extensions
            .get::<AppState>()
            .cloned()
            .ok_or(AppError::Unauthenticated)?;
        authenticate(&state, &parts.headers).await
    }
}
