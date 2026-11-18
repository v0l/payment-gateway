use super::*;

pub async fn insert_tenant(
    pool: &MySqlPool,
    name: &str,
    api_key_hash: &str,
    webhook_url: Option<&str>,
    sealed_webhook_secret: &[u8],
) -> Result<u64> {
    let r = sqlx::query("insert into tenant (name, api_key_hash, webhook_secret) values (?, ?, ?)")
        .bind(name)
        .bind(api_key_hash)
        .bind(sealed_webhook_secret)
        .execute(pool)
        .await?;
    let id = r.last_insert_id();
    set_default_key(pool, id, api_key_hash).await?;
    set_default_endpoint_url(pool, id, webhook_url).await?;
    Ok(id)
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
    if r.rows_affected() > 0 {
        set_default_key(pool, id, api_key_hash).await?;
    }
    Ok(r.rows_affected() > 0)
}

pub struct TenantSettings {
    pub webhook_url: Option<String>,
    pub display_name: Option<String>,
    pub logo_url: Option<String>,
    pub accent_color: Option<String>,
    pub allowed_origins: Option<String>,
    pub default_success_url: Option<String>,
    pub default_cancel_url: Option<String>,
}

pub async fn update_tenant_settings(pool: &MySqlPool, id: u64, s: &TenantSettings) -> Result<bool> {
    let r = sqlx::query(
        "update tenant set display_name = ?, logo_url = ?, accent_color = ?, allowed_origins = ?, default_success_url = ?, default_cancel_url = ? where id = ?",
    )
    .bind(&s.display_name)
    .bind(&s.logo_url)
    .bind(&s.accent_color)
    .bind(&s.allowed_origins)
    .bind(&s.default_success_url)
    .bind(&s.default_cancel_url)
    .bind(id)
    .execute(pool)
    .await?;
    set_default_endpoint_url(pool, id, s.webhook_url.as_deref()).await?;
    Ok(r.rows_affected() > 0)
}

pub async fn tenant_by_name(pool: &MySqlPool, name: &str) -> Result<Option<Tenant>> {
    Ok(
        sqlx::query_as(&format!("select {TENANT_COLS} from tenant where name = ?"))
            .bind(name)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn set_tenant_require_2fa(pool: &MySqlPool, id: u64, required: bool) -> Result<bool> {
    let r = sqlx::query("update tenant set require_2fa = ? where id = ?")
        .bind(required)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn set_tenant_trusted(pool: &MySqlPool, id: u64, trusted: bool) -> Result<bool> {
    let r = sqlx::query("update tenant set trusted = ? where id = ?")
        .bind(trusted)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn set_tenant_limits(
    pool: &MySqlPool,
    id: u64,
    o: &crate::limits::LimitOverrides,
) -> Result<bool> {
    let r = sqlx::query(
        "update tenant set limit_payments_per_day = ?, limit_pending_payments = ?, limit_amount_per_payment = ?, limit_volume_per_day = ?, limit_providers = ?, limit_webhook_endpoints = ?, limit_attempts_per_payment = ? where id = ?",
    )
    .bind(o.limit_payments_per_day)
    .bind(o.limit_pending_payments)
    .bind(o.limit_amount_per_payment)
    .bind(o.limit_volume_per_day)
    .bind(o.limit_providers)
    .bind(o.limit_webhook_endpoints)
    .bind(o.limit_attempts_per_payment)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn lock_tenant<'e, E: Executor<'e, Database = MySql>>(ex: E, id: u64) -> Result<()> {
    sqlx::query("select id from tenant where id = ? for update")
        .bind(id)
        .fetch_optional(ex)
        .await?;
    Ok(())
}
