//! Local integration doubles. Never enabled in production or without an explicit opt-in.
use crate::{
    http::{auth::AuthUser, state::AppState},
    shared::{
        error::AppError,
        rbutil::{col_cents, col_i64, col_str, e, q, q1},
        util::{cents_to_string, unix_now},
    },
};
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub fn enabled() -> Result<(), AppError> {
    if std::env::var("ECSHOP_INTEGRATION_MODE").as_deref() != Ok("mock")
        || std::env::var("ECSHOP_ENV").as_deref() == Ok("production")
    {
        return Err(AppError::NotFound("mock integration disabled".into()));
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct MockPayment {
    pub provider_trade_no: String,
}

/// Simulate a signed provider notification; amount and owner come from the database.
pub async fn pay(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
    Json(body): Json<MockPayment>,
) -> Result<Json<Value>, AppError> {
    enabled()?;
    let order = q1(state.rb, "SELECT order_sn AS order_sn, order_amount AS order_amount FROM ecs_order_info WHERE order_id = ? AND user_id = ?", vec![json!(id), json!(auth.user_id)]).await?
        .ok_or_else(|| AppError::NotFound("order not found".into()))?;
    let paid = q1(
        state.rb,
        "SELECT paid_cents AS paid_cents FROM ecs_order_balance_payment WHERE order_id = ?",
        vec![json!(id)],
    )
    .await?
    .map(|r| col_i64(&r, "paid_cents"))
    .unwrap_or(0);
    let raw = serde_json::to_vec(&json!({"order_sn":col_str(&order,"order_sn"),"amount":cents_to_string(col_cents(&order,"order_amount")-paid),"provider_trade_no":body.provider_trade_no})).map_err(|e| AppError::Internal(e.into()))?;
    let signature =
        crate::infrastructure::crypto::hmac_sha256_hex(&state.config.payment_callback_secret, &raw);
    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Payment-Signature",
        signature
            .parse()
            .map_err(|_| AppError::Validation("invalid signature".into()))?,
    );
    super::payment::payment_callback(State(state), Path("mock".into()), headers, raw.into()).await
}

/// Mock mailbox is owner-scoped and exposes reset/verification links only in mock mode.
pub async fn mailbox(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<Json<Value>, AppError> {
    enabled()?;
    let rows = q(state.rb,"SELECT message_id AS id, recipient AS recipient, template_name AS template, payload AS payload, status AS status FROM ecs_email_outbox WHERE user_id = ? ORDER BY message_id DESC LIMIT 100",vec![json!(auth.user_id)]).await?;
    Ok(Json(json!({"mock":true,"items":rows})))
}

/// Simulate carrier pickup, enabling the real receive transition.
pub async fn ship(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    enabled()?;
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    crate::infrastructure::db::lock_user(&tx, auth.user_id).await?;
    let result=e(&tx,"UPDATE ecs_order_info SET order_status = 'shipped' WHERE order_id = ? AND user_id = ? AND order_status = 'paid'",vec![json!(id),json!(auth.user_id)]).await?;
    if result.rows_affected != 1 {
        return Err(AppError::Conflict("paid order required".into()));
    }
    e(&tx,"INSERT INTO ecs_order_action(order_id,actor_user_id,action,note,created_at) VALUES(?,?,'ship','mock carrier',?)",vec![json!(id),json!(auth.user_id),json!(unix_now())]).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"mock":true,"order_id":id,"status":"shipped","tracking_number":format!("MOCK{id}")}),
    ))
}

/// Simulate a provider settling a previously created deposit payment intent exactly once.
pub async fn deposit(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    enabled()?;
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    crate::infrastructure::db::lock_user(&tx, auth.user_id).await?;
    let intent=q1(&tx,"SELECT amount_cents AS amount_cents,status AS status FROM ecs_account_payment_intent WHERE request_id=? AND user_id=?",vec![json!(id),json!(auth.user_id)]).await?.ok_or_else(||AppError::NotFound("payment intent not found".into()))?;
    if col_str(&intent, "status") == "paid" {
        return Ok(Json(json!({"mock":true,"replayed":true})));
    }
    let updated=e(&tx,"UPDATE ecs_user_account SET status='paid',paid_at=? WHERE rec_id=? AND user_id=? AND process_type='deposit' AND status='pending_payment'",vec![json!(unix_now()),json!(id),json!(auth.user_id)]).await?;
    if updated.rows_affected != 1 {
        return Err(AppError::Conflict("deposit not pending payment".into()));
    }
    let amount = col_i64(&intent, "amount_cents");
    e(
        &tx,
        "INSERT OR IGNORE INTO ecs_account_balance(user_id) VALUES(?)",
        vec![json!(auth.user_id)],
    )
    .await?;
    e(
        &tx,
        "UPDATE ecs_account_balance SET available_cents=available_cents+? WHERE user_id=?",
        vec![json!(amount), json!(auth.user_id)],
    )
    .await?;
    e(
        &tx,
        "UPDATE ecs_account_payment_intent SET status='paid',updated_at=? WHERE request_id=?",
        vec![json!(unix_now()), json!(id)],
    )
    .await?;
    e(&tx,"INSERT INTO ecs_account_log(user_id,available_delta_cents,reason,reference_type,reference_id) VALUES(?,?,'mock_deposit','user_account',?)",vec![json!(auth.user_id),json!(amount),json!(id)]).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"mock":true,"replayed":false,"credited":cents_to_string(amount)}),
    ))
}
