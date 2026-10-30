//! Payment context.
//!
//! Modern mock adapter: HMAC-SHA256 signed JSON callback with db-level
//! idempotency on (provider, provider_trade_no).
//!
//! Legacy protocols (source-compatible, offline-verified only):
//! - POST /api/v1/me/orders/:id/payment-intents creates a server-side payment
//!   intent binding pay_id, provider, external number and the remaining due in
//!   integer cents.
//! - Chinabank MD5 form (v_oid is the server-side intent number).
//! - Alipay ksort MD5 form (out_trade_no is the server-side intent number).
//! - /respond.php dispatches by `code` whitelist; duplicate form keys are
//!   rejected so the signature cannot be parsed two ways.
//! - /receive.php is the authenticated confirm-receipt action (GET or POST),
//!   not a payment callback.
//!
//! Keys come from environment variables only; an unconfigured channel is
//! disabled. No fake signature acceptance and no real channel is contacted.
use axum::body::Bytes;
use axum::extract::{Path, RawQuery, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use rbatis::executor::Executor;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::http::auth::AuthUser;
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
    if provider == "mock" {
        super::mock::enabled()?;
    }
    if provider.len() > 60 {
        return Err(AppError::Validation("provider too long".into()));
    }
    let signature = headers
        .get("X-Payment-Signature")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !crate::infrastructure::crypto::verify_hmac_sha256_hex(
        &state.config.payment_callback_secret,
        &body,
        signature,
    ) {
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
        return Err(AppError::Conflict(
            "provider trade number conflicts".to_string(),
        ));
    }
    if order_status == "paid" {
        return Err(AppError::Conflict(
            "order already paid; reconcile duplicate collection".into(),
        ));
    }
    let balance_paid = q1(
        &tx,
        "SELECT paid_cents AS paid_cents FROM ecs_order_balance_payment WHERE order_id = ?",
        vec![json!(order_id)],
    )
    .await?
    .map(|r| col_i64(&r, "paid_cents"))
    .unwrap_or(0);
    if amount_cents <= 0 || amount_cents != order_amount_cents - balance_paid {
        return Err(AppError::Conflict(
            "callback amount does not match remaining due".into(),
        ));
    }
    if order_status != "pending_payment" {
        return Err(AppError::Conflict(
            "order is not pending payment".to_string(),
        ));
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
        return Err(AppError::Conflict(
            "order status changed concurrently".to_string(),
        ));
    }
    tx.commit().await?;
    Ok(Json(json!({
        "status": "ok",
        "replayed": false,
        "order_id": order_id,
        "order_amount": cents_to_string(order_amount_cents),
    })))
}

// ---------------------------------------------------------------------------
// Payment intents
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct PaymentIntentRequest {
    provider: String,
}

/// POST /api/v1/me/orders/{id}/payment-intents — bind a legacy channel payment
/// attempt to the remaining due of one owned pending order.
pub async fn payment_intent_create(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<String>,
    Json(body): Json<PaymentIntentRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    let provider = body.provider.trim().to_ascii_lowercase();
    if !matches!(provider.as_str(), "chinabank" | "alipay") {
        return Err(AppError::Validation("unsupported payment provider".into()));
    }
    let enabled = match provider.as_str() {
        "chinabank" => state.config.chinabank_enabled(),
        _ => state.config.alipay_enabled(),
    };
    if !enabled {
        return Err(AppError::NotFound("payment channel disabled".into()));
    }
    let ok_id = id
        .parse::<i64>()
        .map_err(|_| AppError::Validation("id must be a positive integer".to_string()))?;
    let now = unix_now();
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    crate::infrastructure::db::lock_user(&tx, auth.user_id).await?;
    let order = q1(
        &tx,
        "SELECT pay_id AS pay_id, order_status AS order_status, order_amount AS order_amount
         FROM ecs_order_info WHERE order_id = ? AND user_id = ?",
        vec![json!(ok_id), json!(auth.user_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("order not found".to_string()))?;
    if col_str(&order, "order_status") != "pending_payment" {
        return Err(AppError::Conflict("order is not pending payment".into()));
    }
    let paid = q1(
        &tx,
        "SELECT paid_cents AS paid_cents FROM ecs_order_balance_payment WHERE order_id = ?",
        vec![json!(ok_id)],
    )
    .await?
    .map(|r| col_i64(&r, "paid_cents"))
    .unwrap_or(0);
    let remaining = col_cents(&order, "order_amount").saturating_sub(paid);
    if remaining <= 0 {
        return Err(AppError::Conflict("order already fully paid".into()));
    }
    // A new intent invalidates pending intents of the same order: only one
    // open channel attempt may exist at a time.
    e(
        &tx,
        "UPDATE ecs_payment_intent SET status = 'expired' WHERE order_id = ? AND status = 'pending'",
        vec![json!(ok_id)],
    )
    .await?;
    let intent_sn = format!(
        "PI{}{}",
        now,
        crate::infrastructure::crypto::random_token(6)
    );
    let expires_at = now + 1800;
    e(
        &tx,
        "INSERT INTO ecs_payment_intent(intent_sn, order_id, user_id, pay_id, provider, amount_cents, status, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
        vec![
            json!(intent_sn),
            json!(ok_id),
            json!(auth.user_id),
            json!(col_i64(&order, "pay_id")),
            json!(provider),
            json!(remaining),
            json!(now),
            json!(expires_at),
        ],
    )
    .await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({
            "order_id": ok_id,
            "intent_sn": intent_sn,
            "provider": provider,
            "pay_id": col_i64(&order, "pay_id"),
            "amount": cents_to_string(remaining),
            "expires_at": expires_at,
        })),
    ))
}

// ---------------------------------------------------------------------------
// Shared intent settlement
// ---------------------------------------------------------------------------

enum Settle {
    Ok { replayed: bool },
    Rejected(String),
}

/// Consume one intent exactly once after all second checks (amount, pay_id,
/// expiry, provider, remaining due). Called inside an open transaction.
/// The trade number lands in ecs_pay_log (provider, provider_trade_no) which
/// makes the settlement globally idempotent and rejects cross-order reuse.
async fn settle_intent<E: Executor>(
    tx: &E,
    provider: &str,
    intent_sn: &str,
    amount_cents: i64,
    trade_no: &str,
    raw: &str,
) -> Result<Settle, AppError> {
    let Some(intent) = q1(
        tx,
        "SELECT * FROM ecs_payment_intent WHERE intent_sn = ?",
        vec![json!(intent_sn)],
    )
    .await?
    else {
        return Ok(Settle::Rejected("invalid payment intent".into()));
    };
    if provider != col_str(&intent, "provider") || amount_cents != col_i64(&intent, "amount_cents")
    {
        return Ok(Settle::Rejected("payment intent mismatch".into()));
    }
    let order_id = col_i64(&intent, "order_id");
    // Existing channel transaction: same order replays, others are reuse.
    let logged = q1(
        tx,
        "SELECT order_id AS order_id FROM ecs_pay_log WHERE provider = ? AND provider_trade_no = ?",
        vec![json!(provider), json!(trade_no)],
    )
    .await?
    .map(|r| col_i64(&r, "order_id"));
    if col_str(&intent, "status") == "paid" {
        return Ok(match logged {
            Some(logged_order) if logged_order == order_id => Settle::Ok { replayed: true },
            _ => Settle::Rejected("payment intent already settled".into()),
        });
    }
    if col_str(&intent, "status") != "pending" {
        return Ok(Settle::Rejected("payment intent is not pending".into()));
    }
    if col_i64(&intent, "expires_at") <= unix_now() {
        e(
            tx,
            "UPDATE ecs_payment_intent SET status = 'expired' WHERE intent_sn = ?",
            vec![json!(intent_sn)],
        )
        .await?;
        return Ok(Settle::Rejected("payment intent expired".into()));
    }
    let order = q1(
        tx,
        "SELECT order_status AS order_status, pay_id AS pay_id, order_amount AS order_amount
         FROM ecs_order_info WHERE order_id = ?",
        vec![json!(order_id)],
    )
    .await?
    .ok_or_else(|| AppError::NotFound("order not found".to_string()))?;
    if col_i64(&order, "pay_id") != col_i64(&intent, "pay_id") {
        e(
            tx,
            "UPDATE ecs_payment_intent SET status = 'expired' WHERE intent_sn = ?",
            vec![json!(intent_sn)],
        )
        .await?;
        return Ok(Settle::Rejected(
            "payment intent invalidated by payment change".into(),
        ));
    }
    if col_str(&order, "order_status") != "pending_payment" {
        return Ok(Settle::Rejected("order is not pending payment".into()));
    }
    let balance_paid = q1(
        tx,
        "SELECT paid_cents AS paid_cents FROM ecs_order_balance_payment WHERE order_id = ?",
        vec![json!(order_id)],
    )
    .await?
    .map(|r| col_i64(&r, "paid_cents"))
    .unwrap_or(0);
    if amount_cents != col_cents(&order, "order_amount").saturating_sub(balance_paid) {
        return Ok(Settle::Rejected("payment intent amount is stale".into()));
    }
    if matches!(logged, Some(logged_order) if logged_order != order_id) {
        return Ok(Settle::Rejected("provider trade number reuse".into()));
    }
    e(
        tx,
        "UPDATE ecs_payment_intent SET status = 'paid' WHERE intent_sn = ? AND status = 'pending'",
        vec![json!(intent_sn)],
    )
    .await?;
    e(
        tx,
        "INSERT INTO ecs_pay_log (order_id, provider, provider_trade_no, amount, status, raw_payload, received_at)
         VALUES (?, ?, ?, ?, 'paid', ?, ?)",
        vec![
            json!(order_id),
            json!(provider),
            json!(trade_no),
            json!(amount_cents as f64 / 100.0),
            json!(raw),
            json!(unix_now()),
        ],
    )
    .await?;
    let updated = e(
        tx,
        "UPDATE ecs_order_info SET order_status = 'paid' WHERE order_id = ? AND order_status = 'pending_payment'",
        vec![json!(order_id)],
    )
    .await?;
    if updated.rows_affected != 1 {
        return Ok(Settle::Rejected("order status changed concurrently".into()));
    }
    Ok(Settle::Ok { replayed: false })
}

// ---------------------------------------------------------------------------
// Legacy form helpers
// ---------------------------------------------------------------------------

const MAX_LEGACY_BODY: usize = 32 * 1024;
const MAX_LEGACY_FIELDS: usize = 64;

fn percent_decode(component: &str) -> Result<String, AppError> {
    let input = component.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        match input[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' => {
                if input.len() < index + 3 {
                    return Err(AppError::Validation("malformed form encoding".into()));
                }
                let hi = (input[index + 1] as char)
                    .to_digit(16)
                    .ok_or_else(|| AppError::Validation("malformed form encoding".into()))?;
                let lo = (input[index + 2] as char)
                    .to_digit(16)
                    .ok_or_else(|| AppError::Validation("malformed form encoding".into()))?;
                out.push((hi * 16 + lo) as u8);
                index += 3;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| AppError::Validation("form is not UTF-8".into()))
}

/// Parse one urlencoded source; empty segments are skipped, every key must be
/// present exactly once to keep the signature unambiguous.
fn parse_form(source: &str) -> Result<Vec<(String, String)>, AppError> {
    let mut pairs = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for chunk in source.split('&') {
        if chunk.is_empty() {
            continue;
        }
        if pairs.len() >= MAX_LEGACY_FIELDS {
            return Err(AppError::Validation("too many form fields".into()));
        }
        let (key, value) = chunk
            .split_once('=')
            .ok_or_else(|| AppError::Validation("malformed form pair".into()))?;
        let key = percent_decode(key)?;
        if key.is_empty() || !seen.insert(key.clone()) {
            return Err(AppError::Validation("duplicate form field".into()));
        }
        pairs.push((key, percent_decode(value)?));
    }
    Ok(pairs)
}

fn form_value<'a>(pairs: &'a [(String, String)], field: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(k, _)| k == field)
        .map(|(_, v)| v.as_str())
}

/// Merge the query string and form body with strict duplicate-key rejection.
fn raw_pairs(
    body: impl Into<Vec<u8>>,
    query: Option<String>,
) -> Result<Vec<(String, String)>, AppError> {
    let bytes: Vec<u8> = body.into();
    if bytes.len() > MAX_LEGACY_BODY {
        return Err(AppError::Validation("form body too large".into()));
    }
    let body_text = String::from_utf8(bytes)
        .map_err(|_| AppError::Validation("form body is not UTF-8".into()))?;
    let mut pairs = match query {
        Some(query) => parse_form(&query)?,
        None => Vec::new(),
    };
    for (key, value) in parse_form(&body_text)? {
        if pairs.iter().any(|(k, _)| *k == key) {
            return Err(AppError::Validation("duplicate form field".into()));
        }
        pairs.push((key, value));
    }
    Ok(pairs)
}

fn form_source(pairs: &[(String, String)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

// ---------------------------------------------------------------------------
// Chinabank: v_oid + v_pstatus + v_amount + v_moneytype + key, upper-case MD5
// ---------------------------------------------------------------------------

async fn chinabank_settle(
    state: &AppState,
    pairs: Vec<(String, String)>,
) -> Result<String, AppError> {
    // PHP dies with plain 'error' when the channel has no configured key.
    if !state.config.chinabank_enabled() {
        return Ok("error".to_string());
    }
    let intent_sn = form_value(&pairs, "v_oid")
        .ok_or_else(|| AppError::Validation("missing form field v_oid".into()))?
        .trim()
        .to_string();
    let v_pstatus = form_value(&pairs, "v_pstatus")
        .ok_or_else(|| AppError::Validation("missing form field v_pstatus".into()))?
        .trim()
        .to_string();
    let v_amount = form_value(&pairs, "v_amount")
        .ok_or_else(|| AppError::Validation("missing form field v_amount".into()))?
        .trim()
        .to_string();
    let v_moneytype = form_value(&pairs, "v_moneytype")
        .ok_or_else(|| AppError::Validation("missing form field v_moneytype".into()))?
        .trim()
        .to_string();
    let v_md5str = form_value(&pairs, "v_md5str")
        .ok_or_else(|| AppError::Validation("missing form field v_md5str".into()))?
        .trim()
        .to_string();
    if !v_moneytype.eq_ignore_ascii_case("CNY") {
        return Ok("error".into());
    }
    let source = format!(
        "{intent_sn}{v_pstatus}{v_amount}{v_moneytype}{}",
        state.config.chinabank_key
    );
    let expected = crate::infrastructure::crypto::md5_hex(source.as_bytes()).to_uppercase();
    if !crate::infrastructure::crypto::eq_hex_ignore_case(&expected, &v_md5str) {
        return Ok("error".into());
    }
    if v_pstatus != "20" {
        // Verified failure notice: never marks the order paid.
        return Ok("error".into());
    }
    let amount_cents = parse_money_cents(&v_amount, "v_amount")?;
    let raw = form_source(&pairs);
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let settle =
        settle_intent(&tx, "chinabank", &intent_sn, amount_cents, &intent_sn, &raw).await?;
    tx.commit().await?;
    Ok(match settle {
        Settle::Ok { .. } => "ok".into(),
        Settle::Rejected(_) => "error".into(),
    })
}

/// POST /chinabank_receive.php — the bank reconciliation endpoint.
pub async fn chinabank_receive(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Result<String, AppError> {
    let pairs = raw_pairs(body, query)?;
    chinabank_settle(&state, pairs).await
}

// ---------------------------------------------------------------------------
// Alipay: ksort form MD5, excluding sign/sign_type/code
// ---------------------------------------------------------------------------

async fn alipay_settle(state: &AppState, pairs: Vec<(String, String)>) -> Result<String, AppError> {
    if !state.config.alipay_enabled() {
        return Ok("fail".to_string());
    }
    let provided = form_value(&pairs, "sign")
        .ok_or_else(|| AppError::Validation("missing form field sign".into()))?
        .trim()
        .to_string();
    let sign_type = form_value(&pairs, "sign_type").unwrap_or("").trim();
    if !sign_type.is_empty() && !sign_type.eq_ignore_ascii_case("MD5") {
        return Ok("fail".into());
    }
    let mut sorted = pairs.clone();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut source = String::new();
    for (key, value) in &sorted {
        if key == "sign" || key == "sign_type" || key == "code" {
            continue;
        }
        use std::fmt::Write;
        let _ = write!(source, "{key}={value}&");
    }
    // Drop the trailing '&', then append the key exactly like the PHP plugin.
    source.pop();
    source.push_str(&state.config.alipay_key);
    if crate::infrastructure::crypto::md5_hex(source.as_bytes()) != provided.to_ascii_lowercase() {
        return Ok("fail".into());
    }
    // When configured, merchant identity fields must match the server copy;
    // the request cannot nominate its own account.
    for (field, configured) in [
        ("seller_email", state.config.alipay_account.as_str()),
        ("partner", state.config.alipay_partner.as_str()),
    ] {
        if configured.is_empty() {
            continue;
        }
        let supplied = form_value(&pairs, field)
            .ok_or_else(|| AppError::Validation(format!("missing form field {field}")))?
            .trim()
            .to_ascii_lowercase();
        if supplied != configured.to_ascii_lowercase() {
            return Ok("fail".into());
        }
    }
    let trade_no = form_value(&pairs, "trade_no")
        .ok_or_else(|| AppError::Validation("missing form field trade_no".into()))?
        .trim()
        .to_string();
    let out_trade_no = form_value(&pairs, "out_trade_no")
        .ok_or_else(|| AppError::Validation("missing form field out_trade_no".into()))?
        .trim()
        .to_string();
    let total_fee = form_value(&pairs, "total_fee").unwrap_or("").to_string();
    let trade_status = form_value(&pairs, "trade_status").unwrap_or("").trim();
    if !matches!(
        trade_status,
        "TRADE_SUCCESS" | "TRADE_FINISHED" | "WAIT_SELLER_SEND_GOODS"
    ) {
        // A verified intermediate status never settles the order.
        return Ok("fail".into());
    }
    let amount_cents = parse_money_cents(&total_fee, "total_fee")?;
    let raw = form_source(&pairs);
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    let settle = settle_intent(&tx, "alipay", &out_trade_no, amount_cents, &trade_no, &raw).await?;
    tx.commit().await?;
    Ok(match settle {
        Settle::Ok { .. } => "success".into(),
        Settle::Rejected(_) => "fail".into(),
    })
}

/// /respond.php — legacy gateway dispatcher. Only whitelisted `code` values
/// reach their protocol handler; unknown plugins report as not enabled.
pub async fn respond_dispatch(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Result<String, AppError> {
    let pairs = raw_pairs(body, query)?;
    let code = form_value(&pairs, "code")
        .ok_or_else(|| AppError::Validation("payment channel not enabled".into()))?
        .trim()
        .to_ascii_lowercase();
    match code.as_str() {
        "chinabank" => chinabank_settle(&state, pairs).await,
        "alipay" => alipay_settle(&state, pairs).await,
        // Other historical plugins (PayPal IPN, UnionPay, Tenpay, ...) stay
        // explicitly disabled; no fake verification success.
        _ => Err(AppError::Validation("payment channel not enabled".into())),
    }
}

/// /receive.php — confirmed receipt with Bearer auth and ownership bound to
/// the current user. This replaces the legacy consignee-name weak check.
pub async fn receive_confirm(
    State(state): State<AppState>,
    auth: AuthUser,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Result<Json<Value>, AppError> {
    let order_id = raw_pairs(body, query)?
        .iter()
        .find(|(k, _)| k == "id")
        .map(|(_, v)| v.to_owned())
        .ok_or_else(|| AppError::Validation("missing order id".into()))?
        .parse::<i64>()
        .map_err(|_| AppError::Validation("id must be a positive integer".into()))?;
    let tx = crate::infrastructure::db::begin(state.rb).await?;
    crate::infrastructure::db::lock_user(&tx, auth.user_id).await?;
    let summary = super::order::confirm_receipt_tx(&tx, order_id, auth.user_id).await?;
    tx.commit().await?;
    Ok(Json(summary))
}
