//! HTTP edge and assembly entry. Some helpers stay uncovered until their
//! endpoints mature (error factories, intent replay flags), matching the
//! skeleton-stage policy of the repo.
#![allow(dead_code)]

mod app;
mod application;
mod http;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rust_ecshop=debug,tower_http=debug".into()),
        )
        .init();
    app::run()
        .await
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(())
}
