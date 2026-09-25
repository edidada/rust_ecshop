use serde::Deserialize;

/// Immutable application configuration, built once at startup.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub host: String,
    pub port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
        }
    }
}

impl AppConfig {
    /// Load from environment variables with defaults; only called at startup.
    pub fn from_env() -> Self {
        let host = std::env::var("ECShop_HOST").unwrap_or_else(|_| Self::default().host);
        let port = std::env::var("ECShop_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(Self::default().port);
        Self { host, port }
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
    pub config: std::sync::Arc<AppConfig>,
}
