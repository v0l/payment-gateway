use anyhow::Result;
use payment_gateway::api::{self, AppState};
use payment_gateway::config::Config;
use payment_gateway::crypto::Sealer;
use payment_gateway::db;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::init();
    let config = Config::from_env()?;
    let pool = db::connect(&config.database_url).await?;
    let state = AppState {
        pool,
        sealer: Sealer::new(&config.encryption_key),
        admin_token: config.admin_token,
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
