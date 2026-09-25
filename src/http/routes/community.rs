//! Community context: messages (feedback board), comments, tags.
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::rbutil::{col_i64, col_str, col_opt_str, e, insert_id, q, q1};
use crate::shared::util::unix_now;

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
    let items = q(
        state.rb,
        "SELECT msg_id AS msg_id, user_name AS user_name, msg_title AS msg_title, msg_content AS msg_content,
                msg_time AS msg_time, msg_type AS msg_type
         FROM ecs_feedback WHERE msg_area = 1 AND msg_status = 1 AND parent_id = 0
         ORDER BY msg_time DESC",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| {
        json!({
            "id": col_i64(&r, "msg_id"),
            "username": col_str(&r, "user_name"),
            "title": col_str(&r, "msg_title"),
            "content": col_str(&r, "msg_content"),
            "created_at": col_i64(&r, "msg_time"),
            "type": col_i64(&r, "msg_type"),
        })
    })
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

/// POST /api/v1/messages — anonymous or authenticated post.
pub async fn messages_create(
    State(state): State<AppState>,
    auth: Option<AuthUser>,
    Json(body): Json<MessageCreateRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let content_len = body.content.len();
    if !(1..=2000).contains(&content_len) {
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
    let now = unix_now();
    let result = e(
        state.rb,
        "INSERT INTO ecs_feedback (parent_id, user_id, user_name, user_email, msg_title, msg_type, msg_status, msg_content, msg_time, msg_area)
         VALUES (0, ?, ?, ?, ?, ?, 1, ?, ?, 1)",
        vec![
            json!(user_id),
            json!(display_name),
            json!(body.email),
            json!(body.title),
            json!(body.r#type),
            json!(body.content),
            json!(now),
        ],
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({"id": insert_id(&result), "status": "published"}))))
}

/// GET /api/v1/me/messages — current user's top-level messages with first reply.
pub async fn me_messages(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let rb = state.rb;
    let rows = q(
        rb,
        "SELECT msg_id AS msg_id, msg_title AS msg_title, msg_content AS msg_content, msg_time AS msg_time, order_id AS order_id
         FROM ecs_feedback WHERE user_id = ? AND parent_id = 0 ORDER BY msg_time DESC",
        vec![json!(auth.user_id)],
    )
    .await?;
    let mut items = Vec::new();
    for row in rows {
        let msg_id = col_i64(&row, "msg_id");
        let reply = q1(
            rb,
            "SELECT user_name AS user_name, msg_content AS msg_content FROM ecs_feedback WHERE parent_id = ? ORDER BY msg_time LIMIT 1",
            vec![json!(msg_id)],
        )
        .await?
        .map(|r| json!({"user_name": col_str(&r, "user_name"), "content": col_str(&r, "msg_content")}));
        items.push(json!({
            "id": msg_id,
            "title": col_str(&row, "msg_title"),
            "content": col_str(&row, "msg_content"),
            "created_at": col_i64(&row, "msg_time"),
            "order_id": col_i64(&row, "order_id"),
            "first_reply": reply,
        }));
    }
    Ok(Json(json!({"items": items})))
}

/// DELETE /api/v1/me/messages/{id} — owner only, cascades replies in one transaction.
pub async fn me_message_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let msg_id = parse_id(&id)?;
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let deleted = e(
        &tx,
        "DELETE FROM ecs_feedback WHERE msg_id = ? AND user_id = ?",
        vec![json!(msg_id), json!(auth.user_id)],
    )
    .await?;
    if deleted.rows_affected == 0 {
        return Err(AppError::NotFound("message not found".to_string()));
    }
    e(
        &tx,
        "DELETE FROM ecs_feedback WHERE parent_id = ?",
        vec![json!(msg_id)],
    )
    .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct CommentCreateRequest {
    content: String,
}

/// POST /api/v1/goods/{id}/comments
pub async fn goods_comment_create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<CommentCreateRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let goods_id = parse_id(&id)?;
    let content_len = body.content.len();
    if !(1..=2000).contains(&content_len) {
        return Err(AppError::Validation("content must be 1-2000 bytes".to_string()));
    }
    let rb = state.rb;
    let exists = q1(
        rb,
        "SELECT COUNT(*) AS cnt FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
        vec![json!(goods_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if exists == 0 {
        return Err(AppError::NotFound("goods not found".to_string()));
    }
    let result = e(
        rb,
        "INSERT INTO ecs_comment (comment_type, id_value, user_id, user_name, content, status, add_time)
         VALUES (0, ?, ?, ?, ?, 1, ?)",
        vec![
            json!(goods_id),
            json!(auth.user_id),
            json!(auth.user_name),
            json!(body.content),
            json!(unix_now()),
        ],
    )
    .await?;
    Ok((StatusCode::CREATED, Json(json!({"id": insert_id(&result), "status": "published"}))))
}

/// GET /api/v1/me/comments — current user's top-level comments with first reply.
pub async fn me_comments(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let rb = state.rb;
    let rows = q(
        rb,
        "SELECT c.comment_id AS comment_id, c.id_value AS id_value, c.comment_type AS comment_type, c.content AS content,
                c.add_time AS add_time, COALESCE(g.goods_name, a.title, '') AS target_name
         FROM ecs_comment c
         LEFT JOIN ecs_goods g ON c.comment_type = 0 AND g.goods_id = c.id_value
         LEFT JOIN ecs_article a ON c.comment_type = 1 AND a.article_id = c.id_value
         WHERE c.user_id = ? AND c.parent_id = 0
         ORDER BY c.comment_id DESC",
        vec![json!(auth.user_id)],
    )
    .await?;
    let mut items = Vec::new();
    for row in rows {
        let comment_id = col_i64(&row, "comment_id");
        let reply = q1(
            rb,
            "SELECT content AS content FROM ecs_comment WHERE parent_id = ? ORDER BY comment_id LIMIT 1",
            vec![json!(comment_id)],
        )
        .await?
        .map(|r| col_str(&r, "content"));
        items.push(json!({
            "id": comment_id,
            "target_id": col_i64(&row, "id_value"),
            "target_type": col_i64(&row, "comment_type"),
            "target_name": col_str(&row, "target_name"),
            "content": col_str(&row, "content"),
            "created_at": col_i64(&row, "add_time"),
            "first_reply": reply,
        }));
    }
    Ok(Json(json!({"items": items})))
}

/// DELETE /api/v1/me/comments/{id} — owner only.
pub async fn me_comment_delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let comment_id = parse_id(&id)?;
    let deleted = e(
        state.rb,
        "DELETE FROM ecs_comment WHERE comment_id = ? AND user_id = ?",
        vec![json!(comment_id), json!(auth.user_id)],
    )
    .await?;
    if deleted.rows_affected == 0 {
        return Err(AppError::NotFound("comment not found".to_string()));
    }
    Ok(StatusCode::NO_CONTENT)
}

/// GET /api/v1/tags — public tag cloud over sellable goods.
pub async fn tags_cloud(State(state): State<AppState>) -> Result<Json<Value>, AppError> {
    let items = q(
        state.rb,
        "SELECT t.tag_words AS tag_words, COUNT(*) AS cnt
         FROM ecs_tag t JOIN ecs_goods g ON g.goods_id = t.goods_id
         WHERE g.is_on_sale = 1 AND g.is_delete = 0
         GROUP BY t.tag_words ORDER BY cnt DESC, t.tag_words ASC",
        vec![],
    )
    .await?
    .into_iter()
    .map(|r| json!({"word": col_str(&r, "tag_words"), "count": col_i64(&r, "cnt")}))
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
}

/// GET /api/v1/me/tags — current user's tags aggregated.
pub async fn me_tags(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    let items = q(
        state.rb,
        "SELECT tag_words AS tag_words, COUNT(*) AS cnt FROM ecs_tag
         WHERE user_id = ? GROUP BY tag_words ORDER BY cnt DESC, tag_words ASC",
        vec![json!(auth.user_id)],
    )
    .await?
    .into_iter()
    .map(|r| json!({"word": col_str(&r, "tag_words"), "count": col_i64(&r, "cnt")}))
    .collect::<Vec<_>>();
    Ok(Json(json!({"items": items})))
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
    let rb = state.rb;
    let tx = crate::infrastructure::db::begin(rb).await?;
    let exists = q1(
        &tx,
        "SELECT COUNT(*) AS cnt FROM ecs_goods WHERE goods_id = ? AND is_on_sale = 1 AND is_delete = 0",
        vec![json!(goods_id)],
    )
    .await?
    .map(|r| col_i64(&r, "cnt"))
    .unwrap_or(0);
    if exists == 0 {
        return Err(AppError::NotFound("goods not found".to_string()));
    }
    for t in &tags {
        // Duplicate tags are ignored through the partial unique index.
        let _ = e(
            &tx,
            "INSERT INTO ecs_tag (user_id, goods_id, tag_words) VALUES (?, ?, ?)",
            vec![json!(auth.user_id), json!(goods_id), json!(t)],
        )
        .await;
    }
    tx.commit().await?;
    let items = q(
        rb,
        "SELECT tag_words AS tag_words, COUNT(*) AS cnt FROM ecs_tag WHERE goods_id = ? GROUP BY tag_words ORDER BY COUNT(*) DESC",
        vec![json!(goods_id)],
    )
    .await?
    .into_iter()
    .map(|r| json!({"word": col_str(&r, "tag_words"), "count": col_i64(&r, "cnt")}))
    .collect::<Vec<_>>();
    Ok((StatusCode::CREATED, Json(json!({"items": items}))))
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
    e(
        state.rb,
        "DELETE FROM ecs_tag WHERE user_id = ? AND tag_words = ?",
        vec![json!(auth.user_id), json!(body.tag)],
    )
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[allow(dead_code)]
fn _keep(row: &Value) -> Option<String> {
    col_opt_str(row, "x")
}
