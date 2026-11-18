use super::*;

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

pub async fn get_provider_config(pool: &MySqlPool, id: u64) -> Result<Option<ProviderConfigRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PROVIDER_COLS} from provider_config where id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?)
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

pub async fn list_all_enabled_provider_configs(pool: &MySqlPool) -> Result<Vec<ProviderConfigRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PROVIDER_COLS} from provider_config where enabled = 1 and shared_provider_id is null and tenant_id in (select id from tenant where enabled = 1) order by id"
    ))
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

pub async fn update_provider_config_blob(pool: &MySqlPool, id: u64, sealed: &[u8]) -> Result<()> {
    sqlx::query("update provider_config set config = ? where id = ?")
        .bind(sealed)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_tenant_provider(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
) -> Result<Option<ProviderConfigRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PROVIDER_COLS} from provider_config where id = ? and tenant_id = ?"
    ))
    .bind(id)
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn update_provider_fees(
    pool: &MySqlPool,
    id: u64,
    name: &str,
    fee_rate: Option<f64>,
    fee_base: Option<u64>,
    fee_currency: Option<&str>,
) -> Result<()> {
    sqlx::query("update provider_config set name = ?, fee_rate = ?, fee_base = ?, fee_currency = ? where id = ?")
        .bind(name)
        .bind(fee_rate)
        .bind(fee_base)
        .bind(fee_currency)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn provider_attempt_count(pool: &MySqlPool, id: u64) -> Result<i64> {
    Ok(sqlx::query_scalar("select count(*) from payment_attempt where provider_config_id = ? or watch_provider_id = ?")
        .bind(id)
        .bind(id)
        .fetch_one(pool)
        .await?)
}

pub async fn delete_provider(pool: &MySqlPool, tenant_id: u64, id: u64) -> Result<bool> {
    sqlx::query("delete from provider_cursor where provider_config_id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    let r = sqlx::query("delete from provider_config where id = ? and tenant_id = ?")
        .bind(id)
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
