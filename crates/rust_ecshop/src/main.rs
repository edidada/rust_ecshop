// Skeleton stage: modules are not wired to handlers yet.
#![allow(dead_code)]

mod app;
mod application;
mod domain;
mod http;
mod infrastructure;
mod shared;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "rust_ecshop=debug,tower_http=debug".into()),
        )
        .init();
    app::run().await.map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}
