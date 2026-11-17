use anyhow::Result;
use payment_gateway::api::{self, AppState};
use payment_gateway::config::Config;
use payment_gateway::crypto::Sealer;
use payment_gateway::db;
use payment_gateway::providers::{DefaultFactory, Registry};
use payment_gateway::rates::HttpRates;
use payment_gateway::service::Service;
use payment_gateway::worker;
use std::sync::Arc;
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    let sealer = Sealer::new(&config.encryption_key);
    let registry = Registry::new(
        pool.clone(),
        sealer.clone(),
        Arc::new(DefaultFactory::new(config.allow_mock)),
    );
    let service = Service::new(
        pool,
        sealer,
        registry,
        Arc::new(HttpRates::mempool()),
        config.public_url.clone(),
    );
    let _workers = worker::spawn(service.clone(), Duration::from_secs(5));
    let state = AppState {
        service,
        admin_token: config.admin_token,
        allow_mock: config.allow_mock,
    };
    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    log::info!("listening on {}", config.listen);
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
