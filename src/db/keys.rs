use super::*;

#[derive(Clone, Debug, FromRow)]
pub struct ApiKeyRow {
    pub id: u64,
    pub tenant_id: u64,
    pub name: String,
    pub key_prefix: String,
    pub scopes: String,
    pub mode: Option<String>,
    pub expires: Option<DateTime<Utc>>,
    pub last_used: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub created: DateTime<Utc>,
}

const KEY_COLS: &str =
    "id, tenant_id, name, key_prefix, scopes, mode, expires, last_used, revoked_at, created";

pub async fn set_default_key<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    key_hash: &str,
) -> Result<()> {
    sqlx::query(
        "insert into api_key (tenant_id, name, key_hash, key_prefix, scopes) values (?, 'default', ?, 'pgw_', 'admin') on duplicate key update key_hash = values(key_hash), revoked_at = null",
    )
    .bind(tenant_id)
    .bind(key_hash)
    .execute(ex)
    .await?;
    Ok(())
}

pub struct NewApiKey<'a> {
    pub tenant_id: u64,
    pub name: &'a str,
    pub key_hash: &'a str,
    pub key_prefix: &'a str,
    pub scopes: &'a str,
    pub mode: &'a str,
    pub expires: Option<DateTime<Utc>>,
}

pub async fn insert_api_key(pool: &MySqlPool, k: &NewApiKey<'_>) -> Result<u64> {
    let r = sqlx::query(
        "insert into api_key (tenant_id, name, key_hash, key_prefix, scopes, mode, expires) values (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(k.tenant_id)
    .bind(k.name)
    .bind(k.key_hash)
    .bind(k.key_prefix)
    .bind(k.scopes)
    .bind(k.mode)
    .bind(k.expires)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn api_key_by_hash(pool: &MySqlPool, hash: &str) -> Result<Option<ApiKeyRow>> {
    Ok(sqlx::query_as(&format!(
        "select {KEY_COLS} from api_key where key_hash = ?"
    ))
    .bind(hash)
    .fetch_optional(pool)
    .await?)
}

pub async fn list_api_keys(pool: &MySqlPool, tenant_id: u64) -> Result<Vec<ApiKeyRow>> {
    Ok(sqlx::query_as(&format!(
        "select {KEY_COLS} from api_key where tenant_id = ? order by id"
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn revoke_api_key(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
    now: DateTime<Utc>,
) -> Result<bool> {
    let r = sqlx::query(
        "update api_key set revoked_at = ? where id = ? and tenant_id = ? and revoked_at is null",
    )
    .bind(now)
    .bind(id)
    .bind(tenant_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn touch_api_key(pool: &MySqlPool, id: u64, now: DateTime<Utc>) -> Result<()> {
    sqlx::query(
        "update api_key set last_used = ? where id = ? and (last_used is null or last_used < ?)",
    )
    .bind(now)
    .bind(id)
    .bind(now - chrono::Duration::seconds(60))
    .execute(pool)
    .await?;
    Ok(())
}
