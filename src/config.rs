use anyhow::{Context, Result};

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub listen: String,
    pub admin_token: String,
    pub encryption_key: [u8; 32],
    pub public_url: String,
    pub allow_mock: bool,
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
        let public_url = std::env::var("GATEWAY_PUBLIC_URL").context("GATEWAY_PUBLIC_URL")?;
        anyhow::ensure!(
            url::Url::parse(&public_url).is_ok_and(|u| matches!(u.scheme(), "http" | "https")),
            "GATEWAY_PUBLIC_URL must be an http(s) URL"
        );
        Ok(Self {
            database_url: std::env::var("DATABASE_URL").context("DATABASE_URL")?,
            listen: std::env::var("GATEWAY_LISTEN").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            admin_token,
            encryption_key,
            public_url,
            allow_mock: std::env::var("GATEWAY_ALLOW_MOCK").is_ok_and(|v| v == "1"),
        })
    }
}
