use std::sync::Arc;

use rbatis::rbatis::RBatis;
use serde::Deserialize;

/// Immutable application configuration, built once at startup.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub payment_callback_secret: String,
    pub site_base_url: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 28080,
            database_url: "sqlite://data/ecshop.sqlite3".to_string(),
            payment_callback_secret: "dev-payment-callback-secret".to_string(),
            site_base_url: "http://127.0.0.1:28080".to_string(),
        }
    }
}

impl AppConfig {
    /// Load from environment variables with defaults; only called at startup.
    pub fn from_env() -> Self {
        let d = Self::default();
        let host = std::env::var("ECSHOP_HOST").unwrap_or_else(|_| d.host.clone());
        let port = std::env::var("ECSHOP_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(d.port);
        let database_url = std::env::var("ECSHOP_DATABASE_URL").unwrap_or_else(|_| d.database_url.clone());
        let payment_callback_secret =
            std::env::var("ECSHOP_PAYMENT_CALLBACK_SECRET").unwrap_or_else(|_| d.payment_callback_secret.clone());
        let site_base_url = std::env::var("ECSHOP_SITE_BASE_URL").unwrap_or_else(|_| d.site_base_url.clone());
        Self {
            host,
            port,
            database_url,
            payment_callback_secret,
            site_base_url,
        }
    }

    /// Fail fast when production boots with an unsafe fallback secret.
    /// Development keeps the documented default so `cargo run` works out of the box.
    pub fn validate(&self) -> Result<(), String> {
        let unsafe_default = self.payment_callback_secret.is_empty()
            || self.payment_callback_secret == "dev-payment-callback-secret";
        if unsafe_default && std::env::var("ECSHOP_ENV").as_deref() == Ok("production") {
            return Err(
                "ECSHOP_PAYMENT_CALLBACK_SECRET must be set when ECSHOP_ENV=production".to_string(),
            );
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct RawConfig {
    #[serde(default)]
    host: String,
    #[serde(default)]
    port: u16,
}

/// Shared application state injected into handlers.
#[derive(Clone)]
pub struct AppState {
    pub config: Arc<AppConfig>,
    pub rb: &'static RBatis,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_default_secret_in_production() {
        std::env::remove_var("ECSHOP_ENV");
        let config = AppConfig::default();
        assert!(config.validate().is_ok(), "dev default is allowed outside production");
        std::env::set_var("ECSHOP_ENV", "production");
        assert!(AppConfig::default().validate().is_err());
        let fixed = AppConfig {
            payment_callback_secret: "real-secret-from-env".to_string(),
            ..AppConfig::default()
        };
        assert!(fixed.validate().is_ok());
        std::env::remove_var("ECSHOP_ENV");
    }
}
