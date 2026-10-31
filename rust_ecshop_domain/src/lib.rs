//! Pure business language of the port: entities, value objects, state
//! machines, repository traits, business errors and pure helpers.
//!
//! Layering contract:
//! - `domain`/`error`/`util` depend on nothing but `std`, `serde` and the error
//!   stack (thiserror/anyhow);
//! - `http_compat` is the only module touching a web framework: it maps the
//!   business error onto the unified JSON error body (HTTP edge, no logic).

pub mod domain;
pub mod error;
pub mod http_compat;
pub mod util;
