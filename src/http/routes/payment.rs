//! Payment callback: HMAC-SHA256 signed, idempotent, unique (provider, provider_trade_no).
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use serde_json::{json, Value};

use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::rbutil::{col_cents, col_i64, col_str, e, q1};
use crate::shared::util::{cents_to_string, parse_money_cents, unix_now};

/// POST /api/v1/payments/{provider}/callback — no user auth; channel signature required.
pub async fn payment_callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    let signature = headers
        .get("X-Payment-Signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let expected = crate::infrastructure::crypto::hmac_sha256_hex(
        &state.config.payment_callback_secret,
        &body,
    );
    if signature != expected {
        return Err(AppError::Unauthenticated);
    }
    let payload: Value = serde_json::from_slice(&body)
        .map_err(|_| AppError::Validation("callback body must be JSON".to_string()))?;
    let provider_trade_no = payload
        .get("provider_trade_no")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let order_sn = payload
        .get("order_sn")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let amount_str = payload
        .get("amount")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if provider_trade_no.is_empty() || order_sn.is_empty() {
        return Err(AppError::Validation(
            "provider_trade_no and order_sn are required".to_string(),
        ));
    }
    let amount_cents = parse_money_cents(&amount_str, "amount")?;
    let now = unix_now();
    let raw = String::from_utf8_lossy(&body).to_string();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let order = q1(
        &tx,
        "SELECT order_id AS order_id, order_status AS order_status, order_amount AS order_amount
         FROM ecs_order_info WHERE order_sn = ?",
        vec![json!(order_sn)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("order not found".to_string()))?;
    let order_id = col_i64(&order, "order_id");
    let order_status = col_str(&order, "order_status");
    let order_amount_cents = col_cents(&order, "order_amount");
    // Amount must equal the order's due amount.
    if amount_cents != order_amount_cents {
        return Err(AppError::Conflict("callback amount does not match order".to_string()));
    }
    // Unique (provider, provider_trade_no): replays of the same transaction return 200.
    let existing = q1(
        &tx,
        "SELECT order_id AS order_id FROM ecs_pay_log WHERE provider = ? AND provider_trade_no = ?",
        vec![json!(provider), json!(provider_trade_no)],
    )
    .await?
    .map(|r| col_i64(&r, "order_id"));
    if let Some(existing_order) = existing {
        if existing_order == order_id {
            return Ok(Json(json!({"status": "ok", "replayed": true})));
        }
        return Err(AppError::Conflict("provider trade number conflicts".to_string()));
    }
    if order_status == "paid" {
        // Already paid but no pay_log (e.g. paid by balance): record and ack.
        e(
            &tx,
            "INSERT INTO ecs_pay_log (order_id, provider, provider_trade_no, amount, status, raw_payload, received_at)
             VALUES (?, ?, ?, ?, 'paid', ?, ?)",
            vec![
                json!(order_id),
                json!(provider),
                json!(provider_trade_no),
                json!(amount_cents as f64 / 100.0),
                json!(raw),
                json!(now),
            ],
        )
        .await?;
        tx.commit().await?;
        return Ok(Json(json!({"status": "ok", "replayed": true})));
    }
    if order_status != "pending_payment" {
        return Err(AppError::Conflict("order is not pending payment".to_string()));
    }
    e(
        &tx,
        "INSERT INTO ecs_pay_log (order_id, provider, provider_trade_no, amount, status, raw_payload, received_at)
         VALUES (?, ?, ?, ?, 'paid', ?, ?)",
        vec![
            json!(order_id),
            json!(provider),
            json!(provider_trade_no),
            json!(amount_cents as f64 / 100.0),
            json!(raw),
            json!(now),
        ],
    )
    .await?;
    let updated = e(
        &tx,
        "UPDATE ecs_order_info SET order_status = 'paid' WHERE order_id = ? AND order_status = 'pending_payment'",
        vec![json!(order_id)],
    )
    .await?;
    if updated.rows_affected != 1 {
        return Err(AppError::Conflict("order status changed concurrently".to_string()));
    }
    tx.commit().await?;
    Ok(Json(json!({
        "status": "ok",
        "replayed": false,
        "order_id": order_id,
        "order_amount": cents_to_string(order_amount_cents),
    })))
}
