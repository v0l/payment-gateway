use super::*;

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
        "update tenant set webhook_url = ?, display_name = ?, logo_url = ?, accent_color = ?, allowed_origins = ?, default_success_url = ?, default_cancel_url = ? where id = ?",
    )
    .bind(&s.webhook_url)
    .bind(&s.display_name)
    .bind(&s.logo_url)
    .bind(&s.accent_color)
    .bind(&s.allowed_origins)
    .bind(&s.default_success_url)
    .bind(&s.default_cancel_url)
    .bind(id)
    .execute(pool)
    .await?;
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

pub async fn set_tenant_trusted(pool: &MySqlPool, id: u64, trusted: bool) -> Result<bool> {
    let r = sqlx::query("update tenant set trusted = ? where id = ?")
        .bind(trusted)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
