use super::*;

pub const HEALTH_WINDOW_HOURS: i64 = 24;

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct ProviderHealthRow {
    pub provider_config_id: u64,
    pub window_start: DateTime<Utc>,
    pub successes: u64,
    pub errors: u64,
    pub consecutive_failures: u32,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

const HEALTH_COLS: &str = "provider_config_id, window_start, successes, errors, consecutive_failures, last_success_at, last_error_at, last_error";

pub async fn claim_provider_event(
    pool: &MySqlPool,
    provider_id: u64,
    event_id: &str,
) -> Result<bool> {
    let r = sqlx::query(
        "insert ignore into provider_event (provider_config_id, event_id, created) values (?, ?, ?)",
    )
    .bind(provider_id)
    .bind(event_id)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn release_provider_event(
    pool: &MySqlPool,
    provider_id: u64,
    event_id: &str,
) -> Result<()> {
    sqlx::query("delete from provider_event where provider_config_id = ? and event_id = ?")
        .bind(provider_id)
        .bind(event_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn purge_provider_events(pool: &MySqlPool, before: DateTime<Utc>) -> Result<u64> {
    let r = sqlx::query("delete from provider_event where created < ?")
        .bind(before)
        .execute(pool)
        .await?;
    Ok(r.rows_affected())
}

pub async fn record_provider_success(pool: &MySqlPool, id: u64) -> Result<()> {
    let now = Utc::now();
    let cutoff = now - chrono::Duration::hours(HEALTH_WINDOW_HOURS);
    sqlx::query(
        "insert into provider_health (provider_config_id, window_start, successes, errors, consecutive_failures, last_success_at) values (?, ?, 1, 0, 0, ?) \
         on duplicate key update \
         errors = if(window_start < ?, 0, errors), \
         successes = if(window_start < ?, 1, successes + 1), \
         window_start = if(window_start < ?, ?, window_start), \
         consecutive_failures = 0, last_success_at = ?",
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(cutoff)
    .bind(cutoff)
    .bind(cutoff)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn record_provider_failure(pool: &MySqlPool, id: u64, error: &str) -> Result<u32> {
    let now = Utc::now();
    let cutoff = now - chrono::Duration::hours(HEALTH_WINDOW_HOURS);
    let error: String = error.chars().take(400).collect();
    sqlx::query(
        "insert into provider_health (provider_config_id, window_start, successes, errors, consecutive_failures, last_error_at, last_error) values (?, ?, 0, 1, 1, ?, ?) \
         on duplicate key update \
         successes = if(window_start < ?, 0, successes), \
         errors = if(window_start < ?, 1, errors + 1), \
         window_start = if(window_start < ?, ?, window_start), \
         consecutive_failures = consecutive_failures + 1, last_error_at = ?, last_error = ?",
    )
    .bind(id)
    .bind(now)
    .bind(now)
    .bind(&error)
    .bind(cutoff)
    .bind(cutoff)
    .bind(cutoff)
    .bind(now)
    .bind(now)
    .bind(&error)
    .execute(pool)
    .await?;
    let n: u32 = sqlx::query_scalar(
        "select consecutive_failures from provider_health where provider_config_id = ?",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    Ok(n)
}

pub async fn get_provider_health(pool: &MySqlPool, id: u64) -> Result<Option<ProviderHealthRow>> {
    Ok(sqlx::query_as(&format!(
        "select {HEALTH_COLS} from provider_health where provider_config_id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn list_tenant_provider_health(
    pool: &MySqlPool,
    tenant_id: u64,
) -> Result<Vec<ProviderHealthRow>> {
    Ok(sqlx::query_as(&format!(
        "select {HEALTH_COLS} from provider_health where provider_config_id in (select id from provider_config where tenant_id = ?)"
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn auto_disable_provider(pool: &MySqlPool, id: u64) -> Result<bool> {
    let r = sqlx::query(
        "update provider_config set enabled = 0, auto_disabled_at = ? where id = ? and enabled = 1",
    )
    .bind(Utc::now())
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn provider_auto_disabled_at(pool: &MySqlPool, id: u64) -> Result<Option<DateTime<Utc>>> {
    Ok(
        sqlx::query_scalar("select auto_disabled_at from provider_config where id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .flatten(),
    )
}

pub async fn clear_auto_disabled(pool: &MySqlPool, id: u64) -> Result<()> {
    sqlx::query("update provider_config set auto_disabled_at = null where id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    sqlx::query("update provider_health set consecutive_failures = 0 where provider_config_id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_dispute_status<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
    status: Option<&str>,
) -> Result<()> {
    sqlx::query("update payment set dispute_status = ? where id = ?")
        .bind(status)
        .bind(&id[..])
        .execute(ex)
        .await?;
    Ok(())
}

pub async fn find_attempt_by_external_id(
    pool: &MySqlPool,
    provider_id: u64,
    external_id: &str,
) -> Result<Option<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where (watch_provider_id = ? or provider_config_id = ?) and (lookup = ? or external_id = ?) order by id desc limit 1"
    ))
    .bind(provider_id)
    .bind(provider_id)
    .bind(external_id)
    .bind(external_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn refund_by_external_id<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    payment_id: &PaymentId,
    external_id: &str,
) -> Result<Option<RefundRow>> {
    Ok(sqlx::query_as(&format!(
        "select {REFUND_COLS} from refund where payment_id = ? and external_id = ? limit 1"
    ))
    .bind(&payment_id[..])
    .bind(external_id)
    .fetch_optional(ex)
    .await?)
}

pub async fn tenant_auto_disabled(
    pool: &MySqlPool,
    tenant_id: u64,
) -> Result<Vec<(u64, DateTime<Utc>)>> {
    Ok(sqlx::query_as(
        "select id, auto_disabled_at from provider_config where tenant_id = ? and auto_disabled_at is not null",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct ProviderOverviewRow {
    pub id: u64,
    pub tenant_id: u64,
    pub method: String,
    pub provider_type: String,
    pub name: String,
    pub enabled: bool,
    pub auto_disabled_at: Option<DateTime<Utc>>,
    pub successes: Option<u64>,
    pub errors: Option<u64>,
    pub consecutive_failures: Option<u32>,
    pub last_error_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

pub async fn provider_overview(pool: &MySqlPool) -> Result<Vec<ProviderOverviewRow>> {
    Ok(sqlx::query_as(
        "select c.id, c.tenant_id, c.method, c.provider_type, c.name, c.enabled, c.auto_disabled_at, h.successes, h.errors, h.consecutive_failures, h.last_error_at, h.last_error \
         from provider_config c left join provider_health h on h.provider_config_id = c.id order by c.tenant_id, c.id",
    )
    .fetch_all(pool)
    .await?)
}
