use super::*;

#[derive(Debug, Clone, Default)]
pub struct DeliveryCapture {
    pub status: Option<i32>,
    pub response: Option<String>,
    pub duration_ms: Option<i32>,
}

pub async fn record_delivery_capture(
    pool: &MySqlPool,
    id: u64,
    now: DateTime<Utc>,
    capture: &DeliveryCapture,
) -> Result<()> {
    sqlx::query(
        "update webhook_delivery set last_status = ?, last_response = ?, last_duration_ms = ?, last_attempt_at = ? where id = ?",
    )
    .bind(capture.status)
    .bind(capture.response.as_deref())
    .bind(capture.duration_ms)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_tenant_delivery(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
) -> Result<Option<DeliveryDetail>> {
    Ok(sqlx::query_as(
        "select id, event_id, event_type, attempts, created, next_attempt, delivered_at, abandoned_at, last_error, body, endpoint_id, last_status, last_response, last_duration_ms, last_attempt_at from webhook_delivery where tenant_id = ? and id = ?",
    )
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn replay_abandoned_deliveries(
    pool: &MySqlPool,
    tenant_id: u64,
    now: DateTime<Utc>,
) -> Result<u64> {
    let r = sqlx::query(
        "update webhook_delivery set abandoned_at = null, next_attempt = ? where tenant_id = ? and abandoned_at is not null and delivered_at is null",
    )
    .bind(now)
    .bind(tenant_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected())
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct EndpointHealth {
    pub endpoint_id: Option<u64>,
    pub recent_total: i64,
    pub recent_failed: i64,
    pub last_success: Option<DateTime<Utc>>,
    pub last_failure: Option<DateTime<Utc>>,
    pub avg_duration_ms: Option<f64>,
}

pub async fn endpoint_health(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<EndpointHealth>> {
    Ok(sqlx::query_as(
        "select endpoint_id, cast(count(*) as signed) as recent_total, cast(coalesce(sum(abandoned_at is not null or (delivered_at is null and last_error is not null)), 0) as signed) as recent_failed, max(delivered_at) as last_success, max(case when last_error is not null then last_attempt_at end) as last_failure, avg(last_duration_ms) as avg_duration_ms from webhook_delivery where tenant_id = ? and created >= ? group by endpoint_id",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}
