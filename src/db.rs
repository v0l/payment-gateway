use anyhow::Result;
use serde::Serialize;
use sqlx::{FromRow, MySqlPool};

#[derive(Clone, Debug, FromRow)]
pub struct Tenant {
    pub id: u64,
    pub name: String,
    pub api_key_hash: String,
    pub webhook_url: Option<String>,
    pub webhook_secret: Vec<u8>,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct ProviderConfigRow {
    pub id: u64,
    pub tenant_id: u64,
    pub method: String,
    pub provider_type: String,
    pub name: String,
    pub enabled: bool,
    #[serde(skip)]
    pub config: Vec<u8>,
    pub fee_rate: Option<f64>,
    pub fee_base: Option<u64>,
    pub fee_currency: Option<String>,
}

pub async fn migrate(pool: &MySqlPool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

pub async fn connect(url: &str) -> Result<MySqlPool> {
    let pool = MySqlPool::connect(url).await?;
    migrate(&pool).await?;
    Ok(pool)
}

const TENANT_COLS: &str = "id, name, api_key_hash, webhook_url, webhook_secret, enabled";
const PROVIDER_COLS: &str =
    "id, tenant_id, method, provider_type, name, enabled, config, fee_rate, fee_base, fee_currency";

pub async fn insert_tenant(
    pool: &MySqlPool,
    name: &str,
    api_key_hash: &str,
    webhook_url: Option<&str>,
    sealed_webhook_secret: &[u8],
) -> Result<u64> {
    let r = sqlx::query(
        "insert into tenant (name, api_key_hash, webhook_url, webhook_secret) values (?, ?, ?, ?)",
    )
    .bind(name)
    .bind(api_key_hash)
    .bind(webhook_url)
    .bind(sealed_webhook_secret)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn list_tenants(pool: &MySqlPool) -> Result<Vec<Tenant>> {
    Ok(
        sqlx::query_as(&format!("select {TENANT_COLS} from tenant order by id"))
            .fetch_all(pool)
            .await?,
    )
}

pub async fn get_tenant(pool: &MySqlPool, id: u64) -> Result<Option<Tenant>> {
    Ok(
        sqlx::query_as(&format!("select {TENANT_COLS} from tenant where id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn tenant_by_key_hash(pool: &MySqlPool, hash: &str) -> Result<Option<Tenant>> {
    Ok(sqlx::query_as(&format!(
        "select {TENANT_COLS} from tenant where api_key_hash = ?"
    ))
    .bind(hash)
    .fetch_optional(pool)
    .await?)
}

pub async fn set_tenant_enabled(pool: &MySqlPool, id: u64, enabled: bool) -> Result<bool> {
    let r = sqlx::query("update tenant set enabled = ? where id = ?")
        .bind(enabled)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn rotate_tenant_key(pool: &MySqlPool, id: u64, api_key_hash: &str) -> Result<bool> {
    let r = sqlx::query("update tenant set api_key_hash = ? where id = ?")
        .bind(api_key_hash)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub struct NewProviderConfig<'a> {
    pub tenant_id: u64,
    pub method: &'a str,
    pub provider_type: &'a str,
    pub name: &'a str,
    pub sealed_config: &'a [u8],
    pub fee_rate: Option<f64>,
    pub fee_base: Option<u64>,
    pub fee_currency: Option<&'a str>,
}

pub async fn insert_provider_config(pool: &MySqlPool, c: NewProviderConfig<'_>) -> Result<u64> {
    let r = sqlx::query(
        "insert into provider_config (tenant_id, method, provider_type, name, config, fee_rate, fee_base, fee_currency) values (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(c.tenant_id)
    .bind(c.method)
    .bind(c.provider_type)
    .bind(c.name)
    .bind(c.sealed_config)
    .bind(c.fee_rate)
    .bind(c.fee_base)
    .bind(c.fee_currency)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn list_provider_configs(
    pool: &MySqlPool,
    tenant_id: u64,
    only_enabled: bool,
) -> Result<Vec<ProviderConfigRow>> {
    let filter = if only_enabled { " and enabled = 1" } else { "" };
    Ok(sqlx::query_as(&format!(
        "select {PROVIDER_COLS} from provider_config where tenant_id = ?{filter} order by id"
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn set_provider_config_enabled(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
    enabled: bool,
) -> Result<bool> {
    let r = sqlx::query("update provider_config set enabled = ? where id = ? and tenant_id = ?")
        .bind(enabled)
        .bind(id)
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
