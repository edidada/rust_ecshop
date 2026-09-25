//! Community context: messages (feedback board), comments, tags.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::util::unix_now;

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

fn parse_id(id: &str) -> Result<i64, AppError> {
    match id.parse::<i64>() {
        Ok(v) if v > 0 => Ok(v),
        _ => Err(AppError::Validation("id must be a positive integer".to_string())),
    }
}

#[derive(Deserialize)]
pub struct MessageCreateRequest {
    #[serde(default)]
    username: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    r#type: i64,
    title: String,
    content: String,
    #[serde(default)]
    anonymous: bool,
}

/// GET /api/v1/messages — published public messages (msg_area=1, msg_status=1).
pub async fn messages_list(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT msg_id, user_name, msg_title, msg_content, msg_time, msg_type
                 FROM ecs_feedback WHERE msg_area = 1 AND msg_status = 1 AND parent_id = 0
                 ORDER BY msg_time DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "username": r.get::<_, String>(1)?,
                    "title": r.get::<_, String>(2)?,
                    "content": r.get::<_, String>(3)?,
                    "created_at": r.get::<_, i64>(4)?,
                    "type": r.get::<_, i64>(5)?,
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

/// POST /api/v1/messages — anonymous or authenticated post.
pub async fn messages_create(
    State(state): State<AppState>,
    auth: Option<AuthUser>,
    Json(body): Json<MessageCreateRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let content_len = body.content.len();
    if content_len < 1 || content_len > 2000 {
        return Err(AppError::Validation("content must be 1-2000 bytes".to_string()));
    }
    if body.title.len() > 200 {
        return Err(AppError::Validation("title must be at most 200 bytes".to_string()));
    }
    if body.username.len() > 60 || body.email.len() > 60 {
        return Err(AppError::Validation("username/email must be at most 60 bytes".to_string()));
    }
    let (user_id, user_name) = match (&auth, body.anonymous) {
        (Some(a), false) => (a.user_id, a.user_name.clone()),
        _ => (0, "anonymous".to_string()),
    };
    let display_name = if user_id != 0 { user_name } else { body.username.clone() };
    let db = state.db.clone();
    let now = unix_now();
    let msg_type = body.r#type;
    let title = body.title.clone();
    let content = body.content.clone();
    let email = body.email.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        conn.execute(
            "INSERT INTO ecs_feedback (parent_id, user_id, user_name, user_email, msg_title, msg_type, msg_status, msg_content, msg_time, msg_area)
             VALUES (0, ?1, ?2, ?3, ?4, ?5, 1, ?6, ?7, 1)",
            rusqlite::params![user_id, display_name, email, title, msg_type, content, now],
        )
        .map_err(db_err)?;
        let msg_id = conn.last_insert_rowid();
        Ok(json!({"id": msg_id, "status": "published"}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

/// GET /api/v1/me/messages — current user's top-level messages with first reply.
pub async fn me_messages(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT msg_id, msg_title, msg_content, msg_time, order_id
                 FROM ecs_feedback WHERE user_id = ?1 AND parent_id = 0 ORDER BY msg_time DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                ))
            })
            .map_err(db_err)?;
        let rows: Vec<(i64, String, String, i64, i64)> =
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        let mut items = Vec::new();
        for (msg_id, title, content, time, order_id) in rows {
            let reply: Option<(String, String)> = conn
                .query_row(
                    "SELECT user_name, msg_content FROM ecs_feedback WHERE parent_id = ?1 ORDER BY msg_time LIMIT 1",
                    [msg_id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
                )
                .optional()
                .map_err(db_err)?;
            let reply_json = reply.map(|(name, text)| json!({"user_name": name, "content": text}));
            items.push(json!({
                "id": msg_id,
                "title": title,
                "content": content,
                "created_at": time,
                "order_id": order_id,
                "first_reply": reply_json,
            }));
        }
        Ok(json!({"items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// DELETE /api/v1/me/messages/{id} — owner only, cascades replies in one transaction.
pub async fn me_message_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let msg_id = parse_id(&id)?;
    let user_id = auth.user_id;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let deleted = tx
            .execute(
                "DELETE FROM ecs_feedback WHERE msg_id = ?1 AND user_id = ?2",
                [msg_id, user_id],
            )
            .map_err(db_err)?;
        if deleted == 0 {
            return Err(AppError::NotFound("message not found".to_string()));
        }
        tx.execute("DELETE FROM ecs_feedback WHERE parent_id = ?1", [msg_id])
            .map_err(db_err)?;
        tx.commit().map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/goods/{id}/comments
#[derive(Deserialize)]
pub struct CommentCreateRequest {
    content: String,
}

pub async fn goods_comment_create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<CommentCreateRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let goods_id = parse_id(&id)?;
    let content_len = body.content.len();
    if content_len < 1 || content_len > 2000 {
        return Err(AppError::Validation("content must be 1-2000 bytes".to_string()));
    }
    let db = state.db.clone();
    let now = unix_now();
    let content = body.content.clone();
    let user_id = auth.user_id;
    let user_name = auth.user_name.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM ecs_goods WHERE goods_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                [goods_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if exists == 0 {
            return Err(AppError::NotFound("goods not found".to_string()));
        }
        conn.execute(
            "INSERT INTO ecs_comment (comment_type, id_value, user_id, user_name, content, status, add_time)
             VALUES (0, ?1, ?2, ?3, ?4, 1, ?5)",
            rusqlite::params![goods_id, user_id, user_name, content, now],
        )
        .map_err(db_err)?;
        let comment_id = conn.last_insert_rowid();
        Ok(json!({"id": comment_id, "status": "published"}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

/// GET /api/v1/me/comments — current user's top-level comments with first reply.
pub async fn me_comments(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT c.comment_id, c.id_value, c.comment_type, c.content, c.add_time,
                        COALESCE(g.goods_name, a.title, '')
                 FROM ecs_comment c
                 LEFT JOIN ecs_goods g ON c.comment_type = 0 AND g.goods_id = c.id_value
                 LEFT JOIN ecs_article a ON c.comment_type = 1 AND a.article_id = c.id_value
                 WHERE c.user_id = ?1 AND c.parent_id = 0
                 ORDER BY c.comment_id DESC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(db_err)?;
        let rows: Vec<(i64, i64, i64, String, i64, String)> =
            rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        let mut items = Vec::new();
        for (comment_id, target_id, comment_type, content, add_time, target_name) in rows {
            let reply: Option<String> = conn
                .query_row(
                    "SELECT content FROM ecs_comment WHERE parent_id = ?1 ORDER BY comment_id LIMIT 1",
                    [comment_id],
                    |r| r.get(0),
                )
                .optional()
                .map_err(db_err)?;
            items.push(json!({
                "id": comment_id,
                "target_id": target_id,
                "target_type": comment_type,
                "target_name": target_name,
                "content": content,
                "created_at": add_time,
                "first_reply": reply,
            }));
        }
        Ok(json!({"items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}

/// DELETE /api/v1/me/comments/{id} — owner only.
pub async fn me_comment_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let comment_id = parse_id(&id)?;
    let user_id = auth.user_id;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        let deleted = conn
            .execute(
                "DELETE FROM ecs_comment WHERE comment_id = ?1 AND user_id = ?2",
                [comment_id, user_id],
            )
            .map_err(db_err)?;
        if deleted == 0 {
            return Err(AppError::NotFound("comment not found".to_string()));
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/tags — public tag cloud over sellable goods.
pub async fn tags_cloud(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT t.tag_words, COUNT(*) AS cnt
                 FROM ecs_tag t JOIN ecs_goods g ON g.goods_id = t.goods_id
                 WHERE g.is_on_sale = 1 AND g.is_delete = 0
                 GROUP BY t.tag_words ORDER BY cnt DESC, t.tag_words ASC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([], |r| {
                Ok(json!({
                    "word": r.get::<_, String>(0)?,
                    "count": r.get::<_, i64>(1)?,
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

/// GET /api/v1/me/tags — current user's tags aggregated.
pub async fn me_tags(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let conn = db.blocking_lock();
        let mut stmt = conn
            .prepare(
                "SELECT tag_words, COUNT(*) AS cnt FROM ecs_tag
                 WHERE user_id = ?1 GROUP BY tag_words ORDER BY cnt DESC, tag_words ASC",
            )
            .map_err(db_err)?;
        let rows = stmt
            .query_map([user_id], |r| {
                Ok(json!({
                    "word": r.get::<_, String>(0)?,
                    "count": r.get::<_, i64>(1)?,
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
pub struct TagCreateRequest {
    tag: String,
}

/// POST /api/v1/goods/{id}/tags — 1-10 comma-separated tags, dedup via unique constraint.
pub async fn goods_tag_create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<TagCreateRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let goods_id = parse_id(&id)?;
    let tags: Vec<String> = body
        .tag
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if tags.is_empty() || tags.len() > 10 {
        return Err(AppError::Validation("tag must contain 1-10 comma-separated items".to_string()));
    }
    for t in &tags {
        if t.len() > 255 {
            return Err(AppError::Validation("each tag must be at most 255 bytes".to_string()));
        }
    }
    let db = state.db.clone();
    let user_id = auth.user_id;
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let exists: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM ecs_goods WHERE goods_id = ?1 AND is_on_sale = 1 AND is_delete = 0",
                [goods_id],
                |r| r.get(0),
            )
            .map_err(db_err)?;
        if exists == 0 {
            return Err(AppError::NotFound("goods not found".to_string()));
        }
        for t in &tags {
            let _ = tx.execute(
                "INSERT INTO ecs_tag (user_id, goods_id, tag_words) VALUES (?1, ?2, ?3)",
                rusqlite::params![user_id, goods_id, t],
            );
        }
        tx.commit().map_err(db_err)?;
        let mut stmt = conn
            .prepare("SELECT tag_words, COUNT(*) FROM ecs_tag WHERE goods_id = ?1 GROUP BY tag_words ORDER BY COUNT(*) DESC")
            .map_err(db_err)?;
        let rows = stmt
            .query_map([goods_id], |r| {
                Ok(json!({
                    "word": r.get::<_, String>(0)?,
                    "count": r.get::<_, i64>(1)?,
                }))
            })
            .map_err(db_err)?;
        let items: Vec<Value> = rows.collect::<Result<Vec<_>, _>>().map_err(db_err)?;
        Ok(json!({"items": items}))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok((StatusCode::CREATED, Json(result)))
}

#[derive(Deserialize)]
pub struct TagDeleteRequest {
    tag: String,
}

/// DELETE /api/v1/me/tags — idempotent delete of the user's own tag.
pub async fn me_tag_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(body): Json<TagDeleteRequest>,
) -> Result<StatusCode, AppError> {
    let user_id = auth.user_id;
    let db = state.db.clone();
    let tag = body.tag;
    tokio::task::spawn_blocking(move || -> Result<(), AppError> {
        let conn = db.blocking_lock();
        conn.execute(
            "DELETE FROM ecs_tag WHERE user_id = ?1 AND tag_words = ?2",
            rusqlite::params![user_id, tag],
        )
        .map_err(db_err)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(StatusCode::NO_CONTENT)
}

/// Unused placeholder to keep IntoResponse import; remove if lints complain.
pub fn _unused_into_response() -> impl IntoResponse {
    StatusCode::OK
}
