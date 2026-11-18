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
    let mailer = payment_gateway::mailer::from_env()?;
    let service = Service::with_mailer(
        pool,
        sealer,
        registry,
        Arc::new(HttpRates::mempool()),
        config.public_url.clone(),
        mailer.unwrap_or_else(|| Arc::new(payment_gateway::mailer::DisabledMailer)),
    );
    let sandbox = match payment_gateway::sandbox::SandboxConfig::from_env()? {
        Some(cfg) => Some(payment_gateway::sandbox::Sandbox::from_config(
            cfg,
            service.clone(),
        )?),
        None => None,
    };
    let _workers = worker::spawn(service.clone(), Duration::from_secs(5));
    let state = AppState::new(service, config.admin_token, config.allow_mock, sandbox);
    let listener = tokio::net::TcpListener::bind(&config.listen).await?;
    log::info!("listening on {}", config.listen);
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
