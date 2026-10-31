//! Database and external implementations of the ECShop port: rbatis pooling,
//! SQLite->MySQL dialect translation, hashing helpers and query utilities.
//!
//! Every rbatis type error is mapped here, at the source, into the pure
//! business error `rust_ecshop_domain::error::AppError`. The public surface of
//! this crate therefore never leaks the database driver error type.

pub mod crypto;
pub mod db;
pub mod dialect;
pub mod rbutil;

// Transaction guards / Executor bounds are part of the public db surface.
pub use rbatis;

use rust_ecshop_domain::error::AppError;

/// Map any driver/IO-ish error into `AppError::Internal` with a message.
pub fn internal<E: std::fmt::Debug + std::fmt::Display>(error: E) -> AppError {
    // Driver errors often render as a bare "internal error"; keep the Debug
    // chain so bootstrap/dialect failures stay diagnosable in logs.
    let text = format!("{error}");
    if text.is_empty() || text == "internal error" {
        AppError::Internal(anyhow::anyhow!("{error:?}"))
    } else {
        AppError::Internal(anyhow::anyhow!(text))
    }
}
