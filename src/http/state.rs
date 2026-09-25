use std::sync::Arc;

use serde::Deserialize;

use crate::infrastructure::sqlite::SharedDb;

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
            port: 8080,
            database_url: "data/ecshop.sqlite3".to_string(),
            payment_callback_secret: "dev-payment-callback-secret".to_string(),
            site_base_url: "http://127.0.0.1:8080".to_string(),
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
    pub db: SharedDb,
}
