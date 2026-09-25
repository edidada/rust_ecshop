//! rbatis query helpers shared by all route modules.
//!
//! `q`/`q1`/`e` are generic over `Executor`, so they work both with the pool
//! (`&'static RBatis`) and with an open transaction (`RBatisTxExecutor`).

use rbatis::executor::Executor;
use rbatis::rbatis::RBatis;
use rbatis::rbdc::db::ExecResult;
use serde_json::Value;

use crate::shared::error::AppResult;

/// Convert serde_json values into rbatis bind values.
pub fn args(items: Vec<Value>) -> Vec<rbs::Value> {
    items
        .into_iter()
        .map(|a| serde_json::from_value::<rbs::Value>(a).unwrap_or(rbs::Value::Null))
        .collect()
}

pub fn rbs_to_json(v: rbs::Value) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

/// Query many rows as JSON objects.
pub async fn q<E: Executor>(e: &E, sql: &str, params: Vec<Value>) -> AppResult<Vec<Value>> {
    let rows = Executor::query(e, sql, args(params)).await?;
    let json = rbs_to_json(rows);
    Ok(match json {
        Value::Array(items) => items,
        Value::Null => Vec::new(),
        other => vec![other],
    })
}

/// Query at most one row.
pub async fn q1<E: Executor>(e: &E, sql: &str, params: Vec<Value>) -> AppResult<Option<Value>> {
    Ok(q(e, sql, params).await?.into_iter().next())
}

/// Execute a write statement.
pub async fn e<E: Executor>(e: &E, sql: &str, params: Vec<Value>) -> AppResult<ExecResult> {
    Ok(Executor::exec(e, sql, args(params)).await?)
}

pub fn insert_id(result: &ExecResult) -> i64 {
    result.last_insert_id.as_i64().unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Column accessors (tolerant of number/string/NULL representations)
// ---------------------------------------------------------------------------

fn as_f64(v: Option<&Value>) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.trim().parse().unwrap_or(0.0),
        Some(Value::Bool(b))
            if *b => {
                1.0
            }
        _ => 0.0,
    }
}

/// Read a numeric column as f64 (DECIMAL columns may arrive as REAL or string).
pub fn col_f64(row: &Value, key: &str) -> f64 {
    as_f64(row.get(key))
}

/// Read a numeric column as i64.
pub fn col_i64(row: &Value, key: &str) -> i64 {
    row.get(key).and_then(|v| v.as_i64()).unwrap_or_else(|| as_f64(row.get(key)) as i64)
}

/// Read a DECIMAL money column as integer cents.
pub fn col_cents(row: &Value, key: &str) -> i64 {
    (col_f64(row, key) * 100.0).round() as i64
}

pub fn col_str(row: &Value, key: &str) -> String {
    match row.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

pub fn col_opt_str(row: &Value, key: &str) -> Option<String> {
    match row.get(key) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        _ => None,
    }
}

pub fn col_opt_i64(row: &Value, key: &str) -> Option<i64> {
    match row.get(key) {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

pub fn col_bool(row: &Value, key: &str) -> bool {
    col_i64(row, key) != 0
}

/// Reference to the initialized global pool.
pub fn rb() -> &'static RBatis {
    crate::infrastructure::db::rb()
}
