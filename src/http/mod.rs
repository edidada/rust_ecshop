//! HTTP layer: router, middleware, handlers, DTO validation.
//! Responsible for HTTP status codes and cookies only; no SQL or business rules.

pub mod error;
pub mod routes;
pub mod server;
pub mod state;

pub use server::run;
