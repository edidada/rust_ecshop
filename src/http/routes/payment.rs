//! Payment callback: HMAC-SHA256 signed, idempotent, unique (provider, provider_trade_no).
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::HeaderMap;
use axum::Json;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use crate::http::state::AppState;
use crate::shared::error::AppError;
use crate::shared::util::{cents_to_string, parse_money_cents, unix_now};

fn db_err(e: rusqlite::Error) -> AppError {
    AppError::Internal(e.into())
}

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
        return Err(AppError::Validation("provider_trade_no and order_sn are required".to_string()));
    }
    let amount_cents = parse_money_cents(&amount_str, "amount")?;
    let db = state.db.clone();
    let now = unix_now();
    let raw = String::from_utf8_lossy(&body).to_string();
    let result = tokio::task::spawn_blocking(move || -> Result<Value, AppError> {
        let mut conn = db.blocking_lock();
        let tx = conn.transaction().map_err(db_err)?;
        let order: Option<(i64, String, i64)> = tx
            .query_row(
                "SELECT order_id, order_status, order_amount FROM ecs_order_info WHERE order_sn = ?1",
                [&order_sn],
                |r| {
                    let amount: f64 = r.get(2)?;
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        (amount * 100.0).round() as i64,
                    ))
                },
            )
            .optional()
            .map_err(db_err)?;
        let Some((order_id, order_status, order_amount_cents)) = order else {
            return Err(AppError::NotFound("order not found".to_string()));
        };
        // Amount must equal the order's due amount.
        if amount_cents != order_amount_cents {
            return Err(AppError::Conflict("callback amount does not match order".to_string()));
        }
        // Unique (provider, provider_trade_no): replays of the same transaction return 200.
        let existing: Option<i64> = tx
            .query_row(
                "SELECT order_id FROM ecs_pay_log WHERE provider = ?1 AND provider_trade_no = ?2",
                rusqlite::params![provider, provider_trade_no],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_err)?;
        if let Some(existing_order) = existing {
            if existing_order == order_id {
                return Ok(json!({"status": "ok", "replayed": true}));
            }
            return Err(AppError::Conflict("provider trade number conflicts".to_string()));
        }
        if order_status == "paid" {
            // Already paid but no pay_log (e.g. paid by balance): record and ack.
            tx.execute(
                "INSERT INTO ecs_pay_log (order_id, provider, provider_trade_no, amount, status, raw_payload, received_at)
                 VALUES (?1, ?2, ?3, ?4, 'paid', ?5, ?6)",
                rusqlite::params![
                    order_id, provider, provider_trade_no,
                    amount_cents as f64 / 100.0, raw, now
                ],
            )
            .map_err(db_err)?;
            tx.commit().map_err(db_err)?;
            return Ok(json!({"status": "ok", "replayed": true}));
        }
        if order_status != "pending_payment" {
            return Err(AppError::Conflict("order is not pending payment".to_string()));
        }
        tx.execute(
            "INSERT INTO ecs_pay_log (order_id, provider, provider_trade_no, amount, status, raw_payload, received_at)
             VALUES (?1, ?2, ?3, ?4, 'paid', ?5, ?6)",
            rusqlite::params![
                order_id, provider, provider_trade_no,
                amount_cents as f64 / 100.0, raw, now
            ],
        )
        .map_err(db_err)?;
        let updated = tx
            .execute(
                "UPDATE ecs_order_info SET order_status = 'paid' WHERE order_id = ?1 AND order_status = 'pending_payment'",
                [order_id],
            )
            .map_err(db_err)?;
        if updated != 1 {
            return Err(AppError::Conflict("order status changed concurrently".to_string()));
        }
        tx.commit().map_err(db_err)?;
        Ok(json!({
            "status": "ok",
            "replayed": false,
            "order_id": order_id,
            "order_amount": cents_to_string(order_amount_cents),
        }))
    })
    .await
    .map_err(|e| AppError::Internal(e.into()))??;
    Ok(Json(result))
}
