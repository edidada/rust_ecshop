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
    let token = extract_bearer_token(headers).ok_or(AppError::Unauthenticated)?;
    let token_hash = crate::infrastructure::crypto::sha256_hex(token.as_bytes());
    let db = state.db.clone();
    let row = tokio::task::spawn_blocking(move || find_session_user(&db, &token_hash))
        .await
        .map_err(|e| AppError::Internal(e.into()))?
        .map_err(|e| AppError::Internal(e.into()))?;
    row.map(|(user_id, user_name)| AuthUser { user_id, user_name })
        .ok_or(AppError::Unauthenticated)
}

/// Extract the raw Bearer token from the Authorization header.
fn extract_bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Look up a session token hash. Split out of `authenticate` so it is unit-testable.
pub fn find_session_user(
    db: &crate::infrastructure::sqlite::SharedDb,
    token_hash: &str,
) -> Result<Option<(i64, String)>, rusqlite::Error> {
    let conn = db.blocking_lock();
    conn.query_row(
        "SELECT u.user_id, u.user_name FROM ecs_sessions s JOIN ecs_users u ON u.user_id = s.user_id WHERE s.token_hash = ?1",
        [token_hash],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
    )
    .optional()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn test_db() -> crate::infrastructure::sqlite::SharedDb {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let schema = include_str!("../../docs/sql/sqlite/001_ecshop_catalog.sql");
        conn.execute_batch(schema).unwrap();
        Arc::new(Mutex::new(conn))
    }

    fn seed_session(db: &crate::infrastructure::sqlite::SharedDb, token_hash: &str) {
        let conn = db.blocking_lock();
        conn.execute(
            "INSERT INTO ecs_users (user_id, user_name, email, password_hash) VALUES (1, 'alice', 'alice@example.test', 'x')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO ecs_sessions (token_hash, user_id) VALUES (?1, 1)",
            [token_hash],
        )
        .unwrap();
    }

    #[test]
    fn find_session_user_returns_seeded_session() {
        let db = test_db();
        let hash = crate::infrastructure::crypto::sha256_hex(b"token-a");
        seed_session(&db, &hash);
        let found = find_session_user(&db, &hash).unwrap();
        assert_eq!(found, Some((1, "alice".to_string())));
    }

    #[test]
    fn find_session_user_misses_unknown_token() {
        let db = test_db();
        let hash = crate::infrastructure::crypto::sha256_hex(b"token-a");
        seed_session(&db, &hash);
        let other = crate::infrastructure::crypto::sha256_hex(b"token-b");
        let found = find_session_user(&db, &other).unwrap();
        assert_eq!(found, None);
    }

    #[test]
    fn bearer_token_extraction_follows_contract() {
        let mut headers = axum::http::HeaderMap::new();
        assert_eq!(extract_bearer_token(&headers), None);
        headers.insert(axum::http::header::AUTHORIZATION, "Basic abc".parse().unwrap());
        assert_eq!(extract_bearer_token(&headers), None);
        headers.insert(axum::http::header::AUTHORIZATION, "Bearer ".parse().unwrap());
        assert_eq!(extract_bearer_token(&headers), None);
        headers.insert(axum::http::header::AUTHORIZATION, "Bearer tok-1".parse().unwrap());
        assert_eq!(extract_bearer_token(&headers), Some("tok-1".to_string()));
    }
}
