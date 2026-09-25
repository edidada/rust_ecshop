use rbatis::executor::Executor;
use rbatis::rbatis::RBatis;
use serde_json::json;

use crate::shared::error::AppError;
use crate::shared::rbutil::{q1};

/// Look up a session token hash. Split out so it is unit-testable.
pub async fn find_session_user(rb: &RBatis, token_hash: &str) -> Result<Option<(i64, String)>, AppError> {
    let row = q1(
        rb,
        "SELECT u.user_id AS user_id, u.user_name AS user_name FROM ecs_sessions s JOIN ecs_users u ON u.user_id = s.user_id WHERE s.token_hash = ?",
        vec![json!(token_hash)],
    )
    .await?;
    Ok(row.map(|row| {
        (
            crate::shared::rbutil::col_i64(&row, "user_id"),
            crate::shared::rbutil::col_str(&row, "user_name"),
        )
    }))
}

/// Insert a session row for a user inside an existing transaction executor.
pub async fn insert_session_tx<E: Executor>(tx: &E, token_hash: &str, user_id: i64) -> Result<(), AppError> {
    crate::shared::rbutil::e(
        tx,
        "INSERT INTO ecs_sessions (token_hash, user_id) VALUES (?, ?)",
        vec![json!(token_hash), json!(user_id)],
    )
    .await?;
    Ok(())
}

/// Revoke all sessions of a user (used by password change/reset).
pub async fn revoke_user_sessions<E: Executor>(tx: &E, user_id: i64) -> Result<(), AppError> {
    crate::shared::rbutil::e(
        tx,
        "DELETE FROM ecs_sessions WHERE user_id = ?",
        vec![json!(user_id)],
    )
    .await?;
    Ok(())
}

/// Parse the Bearer token from the Authorization header.
pub fn extract_bearer_token(headers: &axum::http::HeaderMap) -> Option<String> {
    let value = headers.get(axum::http::header::AUTHORIZATION)?.to_str().ok()?;
    let token = value.strip_prefix("Bearer ")?.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

/// Authenticated user resolved from the Bearer token session.
#[derive(Debug, Clone)]
pub struct AuthUser {
    pub user_id: i64,
    pub user_name: String,
}

pub async fn authenticate(rb: &RBatis, headers: &axum::http::HeaderMap) -> Result<AuthUser, AppError> {
    let token = extract_bearer_token(headers).ok_or(AppError::Unauthenticated)?;
    let token_hash = crate::infrastructure::crypto::sha256_hex(token.as_bytes());
    find_session_user(rb, &token_hash)
        .await?
        .map(|(user_id, user_name)| AuthUser { user_id, user_name })
        .ok_or(AppError::Unauthenticated)
}

#[axum::async_trait]
impl FromRequestParts<AppState> for AuthUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        authenticate(state.rb, &parts.headers).await
    }
}

use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::http::state::AppState;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db;

    /// Serialize DB-touching tests: they share one SQLite file per process and
    /// concurrent writers would otherwise hit SQLITE_BUSY.
    static DB_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn test_rb() -> &'static RBatis {
        // Uses the global static; schema bootstrap runs once per process.
        // Note: ":memory:" would give each pooled connection its own empty DB,
        // so tests use a per-process scratch file database instead.
        let url = format!("sqlite://target/rbatis-test-{}.sqlite3", std::process::id());
        db::init(&url).await.expect("init test sqlite")
    }

    #[tokio::test]
    async fn find_session_user_misses_unknown_token() {
        let rb = test_rb().await;
        let hash = crate::infrastructure::crypto::sha256_hex(b"no-such-token");
        let found = find_session_user(rb, &hash).await.unwrap();
        assert_eq!(found, None);
    }

    #[tokio::test]
    async fn find_session_user_returns_seeded_session() {
        let _guard = DB_TEST_LOCK.lock().await;
        let rb = test_rb().await;
        let token = format!("token-{}", std::process::id());
        let hash = crate::infrastructure::crypto::sha256_hex(token.as_bytes());
        crate::shared::rbutil::e(
            rb,
            "INSERT OR IGNORE INTO ecs_users (user_id, user_name, email, password_hash) VALUES (991, 'rbatis-test-user', 'rbatis-test@example.test', 'x')",
            vec![],
        )
        .await
        .unwrap();
        crate::shared::rbutil::e(
            rb,
            "INSERT OR REPLACE INTO ecs_sessions (token_hash, user_id) VALUES (?, 991)",
            vec![json!(hash)],
        )
        .await
        .unwrap();
        let found = find_session_user(rb, &hash).await.unwrap();
        assert_eq!(found, Some((991, "rbatis-test-user".to_string())));
    }

    #[tokio::test]
    async fn transaction_rolls_back_on_explicit_rollback() {
        let _guard = DB_TEST_LOCK.lock().await;
        let rb = test_rb().await;
        crate::shared::rbutil::e(
            rb,
            "INSERT OR IGNORE INTO ecs_users (user_id, user_name, email, password_hash) VALUES (992, 'rbatis-tx-user', 'rbatis-tx@example.test', 'x')",
            vec![],
        )
        .await
        .unwrap();
        {
            let tx = db::begin(rb).await.unwrap();
            insert_session_tx(&tx, "rollback-check-hash", 992).await.unwrap();
            // Explicit rollback: uncommitted changes must stay invisible.
            tx.rollback().await.unwrap();
        }
        let found = find_session_user(rb, "rollback-check-hash").await.unwrap();
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
