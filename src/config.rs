use anyhow::{Context, Result};

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub listen: String,
    pub admin_token: String,
    pub encryption_key: [u8; 32],
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let key = std::env::var("GATEWAY_ENCRYPTION_KEY").context("GATEWAY_ENCRYPTION_KEY")?;
        let key = hex::decode(key.trim()).context("GATEWAY_ENCRYPTION_KEY must be hex")?;
        let encryption_key: [u8; 32] = key
            .try_into()
            .map_err(|_| anyhow::anyhow!("GATEWAY_ENCRYPTION_KEY must be 32 bytes"))?;
        let admin_token = std::env::var("GATEWAY_ADMIN_TOKEN").context("GATEWAY_ADMIN_TOKEN")?;
        anyhow::ensure!(admin_token.len() >= 24, "GATEWAY_ADMIN_TOKEN too short");
        Ok(Self {
            database_url: std::env::var("DATABASE_URL").context("DATABASE_URL")?,
            listen: std::env::var("GATEWAY_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            admin_token,
            encryption_key,
        })
    }
}
