use super::*;
use crate::shared::FeeTerms;

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct SharedProviderRow {
    pub id: u64,
    pub name: String,
    pub method: String,
    pub provider_type: String,
    pub network: String,
    #[serde(skip)]
    pub config: Vec<u8>,
    pub enabled: bool,
    pub fee_bps: u32,
    pub fee_base_msat: u64,
    pub allow_untrusted: bool,
    pub allow_trusted: bool,
}

impl SharedProviderRow {
    pub fn terms(&self) -> FeeTerms {
        FeeTerms {
            bps: self.fee_bps,
            base_msat: self.fee_base_msat,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SharedAttempt {
    pub shared_provider_id: u64,
    pub terms: FeeTerms,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct FeeOverrideRow {
    pub tenant_id: u64,
    pub shared_provider_id: u64,
    pub fee_bps: Option<u32>,
    pub fee_base_msat: Option<u64>,
}

const SHARED_COLS: &str = "id, name, method, provider_type, network, config, enabled, fee_bps, fee_base_msat, allow_untrusted, allow_trusted";

pub struct NewSharedProvider<'a> {
    pub name: &'a str,
    pub method: &'a str,
    pub provider_type: &'a str,
    pub network: &'a str,
    pub sealed_config: &'a [u8],
    pub fee_bps: u32,
    pub fee_base_msat: u64,
    pub allow_untrusted: bool,
    pub allow_trusted: bool,
}

pub async fn insert_shared_provider(pool: &MySqlPool, n: &NewSharedProvider<'_>) -> Result<u64> {
    let r = sqlx::query(
        "insert into shared_provider (name, method, provider_type, network, config, fee_bps, fee_base_msat, allow_untrusted, allow_trusted) values (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(n.name)
    .bind(n.method)
    .bind(n.provider_type)
    .bind(n.network)
    .bind(n.sealed_config)
    .bind(n.fee_bps)
    .bind(n.fee_base_msat)
    .bind(n.allow_untrusted)
    .bind(n.allow_trusted)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn get_shared_provider(pool: &MySqlPool, id: u64) -> Result<Option<SharedProviderRow>> {
    Ok(sqlx::query_as(&format!(
        "select {SHARED_COLS} from shared_provider where id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn list_shared_providers(
    pool: &MySqlPool,
    only_enabled: bool,
) -> Result<Vec<SharedProviderRow>> {
    let filter = if only_enabled {
        " where enabled = 1"
    } else {
        ""
    };
    Ok(sqlx::query_as(&format!(
        "select {SHARED_COLS} from shared_provider{filter} order by id"
    ))
    .fetch_all(pool)
    .await?)
}

pub struct SharedUpdate<'a> {
    pub name: &'a str,
    pub enabled: bool,
    pub fee_bps: u32,
    pub fee_base_msat: u64,
    pub allow_untrusted: bool,
    pub allow_trusted: bool,
}

pub async fn update_shared_provider(pool: &MySqlPool, id: u64, u: &SharedUpdate<'_>) -> Result<()> {
    sqlx::query(
        "update shared_provider set name = ?, enabled = ?, fee_bps = ?, fee_base_msat = ?, allow_untrusted = ?, allow_trusted = ? where id = ?",
    )
    .bind(u.name)
    .bind(u.enabled)
    .bind(u.fee_bps)
    .bind(u.fee_base_msat)
    .bind(u.allow_untrusted)
    .bind(u.allow_trusted)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_shared_config_blob(pool: &MySqlPool, id: u64, sealed: &[u8]) -> Result<()> {
    sqlx::query("update shared_provider set config = ? where id = ?")
        .bind(sealed)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_fee_override(
    pool: &MySqlPool,
    tenant_id: u64,
    shared_provider_id: u64,
) -> Result<Option<FeeOverrideRow>> {
    Ok(sqlx::query_as(
        "select tenant_id, shared_provider_id, fee_bps, fee_base_msat from shared_provider_override where tenant_id = ? and shared_provider_id = ?",
    )
    .bind(tenant_id)
    .bind(shared_provider_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn list_fee_overrides(
    pool: &MySqlPool,
    shared_provider_id: u64,
) -> Result<Vec<FeeOverrideRow>> {
    Ok(sqlx::query_as(
        "select tenant_id, shared_provider_id, fee_bps, fee_base_msat from shared_provider_override where shared_provider_id = ? order by tenant_id",
    )
    .bind(shared_provider_id)
    .fetch_all(pool)
    .await?)
}

pub async fn set_fee_override(
    pool: &MySqlPool,
    tenant_id: u64,
    shared_provider_id: u64,
    fee_bps: Option<u32>,
    fee_base_msat: Option<u64>,
) -> Result<()> {
    if fee_bps.is_none() && fee_base_msat.is_none() {
        sqlx::query(
            "delete from shared_provider_override where tenant_id = ? and shared_provider_id = ?",
        )
        .bind(tenant_id)
        .bind(shared_provider_id)
        .execute(pool)
        .await?;
        return Ok(());
    }
    sqlx::query(
        "insert into shared_provider_override (tenant_id, shared_provider_id, fee_bps, fee_base_msat) values (?, ?, ?, ?) on duplicate key update fee_bps = values(fee_bps), fee_base_msat = values(fee_base_msat)",
    )
    .bind(tenant_id)
    .bind(shared_provider_id)
    .bind(fee_bps)
    .bind(fee_base_msat)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn effective_terms(
    pool: &MySqlPool,
    tenant_id: u64,
    shared: &SharedProviderRow,
) -> Result<FeeTerms> {
    let o = get_fee_override(pool, tenant_id, shared.id).await?;
    Ok(FeeTerms {
        bps: o.as_ref().and_then(|o| o.fee_bps).unwrap_or(shared.fee_bps),
        base_msat: o
            .as_ref()
            .and_then(|o| o.fee_base_msat)
            .unwrap_or(shared.fee_base_msat),
    })
}

pub async fn enable_shared_for_tenant(
    pool: &MySqlPool,
    tenant_id: u64,
    shared: &SharedProviderRow,
    sealed_empty: &[u8],
) -> Result<u64> {
    sqlx::query(
        "insert into provider_config (tenant_id, method, provider_type, name, config, shared_provider_id, enabled) values (?, ?, ?, ?, ?, ?, 1) on duplicate key update enabled = 1",
    )
    .bind(tenant_id)
    .bind(&shared.method)
    .bind(&shared.provider_type)
    .bind(format!("Platform: {}", shared.name))
    .bind(sealed_empty)
    .bind(shared.id)
    .execute(pool)
    .await?;
    let id: u64 = sqlx::query_scalar(
        "select id from provider_config where tenant_id = ? and shared_provider_id = ?",
    )
    .bind(tenant_id)
    .bind(shared.id)
    .fetch_one(pool)
    .await?;
    Ok(id)
}

pub async fn disable_shared_for_tenant(
    pool: &MySqlPool,
    tenant_id: u64,
    shared_provider_id: u64,
) -> Result<bool> {
    let r = sqlx::query(
        "update provider_config set enabled = 0 where tenant_id = ? and shared_provider_id = ?",
    )
    .bind(tenant_id)
    .bind(shared_provider_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn tenant_shared_config(
    pool: &MySqlPool,
    tenant_id: u64,
    shared_provider_id: u64,
) -> Result<Option<ProviderConfigRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PROVIDER_COLS} from provider_config where tenant_id = ? and shared_provider_id = ?"
    ))
    .bind(tenant_id)
    .bind(shared_provider_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn pending_attempts_of_shared(
    pool: &MySqlPool,
    watch_key: u64,
) -> Result<Vec<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where watch_provider_id = ? and status = 'pending' order by id limit 500"
    ))
    .bind(watch_key)
    .fetch_all(pool)
    .await?)
}

pub async fn set_attempt_platform_fee<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    attempt_id: u64,
    fee: u64,
    net: u64,
) -> Result<()> {
    sqlx::query("update payment_attempt set platform_fee = ?, platform_net = ? where id = ?")
        .bind(fee)
        .bind(net)
        .bind(attempt_id)
        .execute(ex)
        .await?;
    Ok(())
}
