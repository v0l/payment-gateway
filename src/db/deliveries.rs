use super::*;

#[allow(clippy::too_many_arguments)]
pub async fn insert_delivery<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    payment_id: Option<&PaymentId>,
    event_id: &str,
    event_type: &str,
    body: &str,
    endpoint_id: Option<u64>,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "insert into webhook_delivery (tenant_id, payment_id, event_id, event_type, body, endpoint_id, created, next_attempt) values (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(tenant_id)
    .bind(payment_id.map(|p| &p[..]))
    .bind(event_id)
    .bind(event_type)
    .bind(body)
    .bind(endpoint_id)
    .bind(now)
    .bind(now)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn due_deliveries(pool: &MySqlPool, now: DateTime<Utc>) -> Result<Vec<DueDelivery>> {
    Ok(sqlx::query_as(
        "select id, tenant_id, event_id, body, attempts, created, endpoint_id from webhook_delivery where delivered_at is null and abandoned_at is null and next_attempt <= ? order by next_attempt limit 50",
    )
    .bind(now)
    .fetch_all(pool)
    .await?)
}

pub async fn delivery_succeeded(pool: &MySqlPool, id: u64, now: DateTime<Utc>) -> Result<()> {
    sqlx::query(
        "update webhook_delivery set delivered_at = ?, attempts = attempts + 1, last_error = null where id = ?",
    )
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delivery_failed(
    pool: &MySqlPool,
    id: u64,
    next: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    error: &str,
) -> Result<()> {
    let error: String = error.chars().take(500).collect();
    match next {
        Some(next) => {
            sqlx::query(
                "update webhook_delivery set attempts = attempts + 1, next_attempt = ?, last_error = ? where id = ?",
            )
            .bind(next)
            .bind(error)
            .bind(id)
            .execute(pool)
            .await?
        }
        None => {
            sqlx::query(
                "update webhook_delivery set attempts = attempts + 1, abandoned_at = ?, last_error = ? where id = ?",
            )
            .bind(now)
            .bind(error)
            .bind(id)
            .execute(pool)
            .await?
        }
    };
    Ok(())
}

pub async fn replay_delivery(pool: &MySqlPool, id: u64, now: DateTime<Utc>) -> Result<bool> {
    let r = sqlx::query(
        "update webhook_delivery set delivered_at = null, abandoned_at = null, next_attempt = ? where id = ?",
    )
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn list_deliveries(
    pool: &MySqlPool,
    tenant_id: Option<u64>,
    undelivered_only: bool,
) -> Result<Vec<DeliveryRow>> {
    let mut sql = String::from(
        "select id, tenant_id, event_id, event_type, attempts, created, next_attempt, delivered_at, abandoned_at, last_error from webhook_delivery where 1=1",
    );
    if tenant_id.is_some() {
        sql.push_str(" and tenant_id = ?");
    }
    if undelivered_only {
        sql.push_str(" and delivered_at is null");
    }
    sql.push_str(" order by id desc limit 200");
    let mut q = sqlx::query_as(&sql);
    if let Some(t) = tenant_id {
        q = q.bind(t);
    }
    Ok(q.fetch_all(pool).await?)
}

pub async fn delivery_tenant(pool: &MySqlPool, id: u64) -> Result<Option<u64>> {
    Ok(
        sqlx::query_scalar("select tenant_id from webhook_delivery where id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn claim_delivery(
    pool: &MySqlPool,
    id: u64,
    now: DateTime<Utc>,
    lease_secs: i64,
) -> Result<bool> {
    let r = sqlx::query(
        "update webhook_delivery set next_attempt = ? where id = ? and delivered_at is null and abandoned_at is null and next_attempt <= ?",
    )
    .bind(now + chrono::Duration::seconds(lease_secs))
    .bind(id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() == 1)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct DeliveryDetail {
    pub id: u64,
    pub event_id: String,
    pub event_type: String,
    pub attempts: u32,
    pub created: DateTime<Utc>,
    pub next_attempt: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub abandoned_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub body: String,
    pub endpoint_id: Option<u64>,
    pub last_status: Option<i32>,
    pub last_response: Option<String>,
    pub last_duration_ms: Option<i32>,
    pub last_attempt_at: Option<DateTime<Utc>>,
}

pub async fn deliveries_for_tenant(
    pool: &MySqlPool,
    tenant_id: u64,
    undelivered_only: bool,
) -> Result<Vec<DeliveryDetail>> {
    let extra = if undelivered_only {
        " and delivered_at is null"
    } else {
        ""
    };
    Ok(sqlx::query_as(&format!(
        "select id, event_id, event_type, attempts, created, next_attempt, delivered_at, abandoned_at, last_error, body, endpoint_id, last_status, last_response, last_duration_ms, last_attempt_at from webhook_delivery where tenant_id = ?{extra} order by id desc limit 100"
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn replay_tenant_delivery(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
    now: DateTime<Utc>,
) -> Result<bool> {
    let r = sqlx::query(
        "update webhook_delivery set delivered_at = null, abandoned_at = null, next_attempt = ? where id = ? and tenant_id = ?",
    )
    .bind(now)
    .bind(id)
    .bind(tenant_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn pending_delivery_count(pool: &MySqlPool, tenant_id: u64) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "select count(*) from webhook_delivery where tenant_id = ? and delivered_at is null and abandoned_at is null",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?)
}

pub async fn pending_delivery_stats(
    pool: &MySqlPool,
    now: DateTime<Utc>,
) -> Result<Option<(i64, i64)>> {
    let row: (i64, Option<DateTime<Utc>>) = sqlx::query_as(
        "select count(*), min(created) from webhook_delivery where delivered_at is null and abandoned_at is null",
    )
    .fetch_one(pool)
    .await?;
    let age = row.1.map(|c| (now - c).num_seconds().max(0)).unwrap_or(0);
    Ok(Some((row.0, age)))
}
