use super::*;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct HourlyVolume {
    pub currency: String,
    pub slot: i64,
    pub payments: i64,
    pub volume: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CurrencyTotal {
    pub currency: String,
    pub payments: i64,
    pub volume: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CreatedOutcome {
    pub currency: String,
    pub paid: i64,
    pub failed: i64,
}

pub async fn hourly_paid(
    pool: &MySqlPool,
    tenant_id: u64,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<HourlyVolume>> {
    Ok(sqlx::query_as(&format!(
        "select currency, cast(floor(timestampdiff(minute, ?, paid_at) / 60) as signed) as slot, count(*) as payments, cast(sum(amount) as signed) as volume from payment where tenant_id = ? and paid_at >= ? and paid_at < ? and status in {PAID_STATES} group by currency, slot order by currency, slot"
    ))
    .bind(start)
    .bind(tenant_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?)
}

pub async fn paid_between(
    pool: &MySqlPool,
    tenant_id: u64,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<CurrencyTotal>> {
    Ok(sqlx::query_as(&format!(
        "select currency, count(*) as payments, cast(sum(amount) as signed) as volume from payment where tenant_id = ? and paid_at >= ? and paid_at < ? and status in {PAID_STATES} group by currency order by currency"
    ))
    .bind(tenant_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?)
}

pub async fn refunded_between(
    pool: &MySqlPool,
    tenant_id: u64,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<CurrencyTotal>> {
    Ok(sqlx::query_as(
        "select p.currency, count(*) as payments, cast(sum(r.amount) as signed) as volume from refund r join payment p on p.id = r.payment_id where p.tenant_id = ? and r.status = 'completed' and r.created >= ? and r.created < ? group by p.currency order by p.currency",
    )
    .bind(tenant_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?)
}

pub async fn outcomes_between(
    pool: &MySqlPool,
    tenant_id: u64,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<CreatedOutcome>> {
    Ok(sqlx::query_as(&format!(
        "select currency, cast(coalesce(sum(status in {PAID_STATES}), 0) as signed) as paid, cast(coalesce(sum(status in ('expired','cancelled')), 0) as signed) as failed from payment where tenant_id = ? and created >= ? and created < ? group by currency order by currency"
    ))
    .bind(tenant_id)
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PendingIncoming {
    pub provider_config_id: u64,
    pub method: String,
    pub platform_managed: bool,
    pub amount: i64,
}

pub async fn unconfirmed_incoming(
    pool: &MySqlPool,
    tenant_id: u64,
) -> Result<Vec<PendingIncoming>> {
    Ok(sqlx::query_as(
        "select a.provider_config_id, a.method, (a.shared_provider_id is not null) as platform_managed, cast(sum(r.amount) as signed) as amount from attempt_receipt r join payment_attempt a on a.id = r.attempt_id join payment p on p.id = a.payment_id where p.tenant_id = ? and r.confirmed = 0 and a.status = 'pending' group by a.provider_config_id, a.method, platform_managed",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, FromRow)]
pub struct EventRow {
    pub id: u64,
    pub event_type: String,
    pub body: String,
    pub created: DateTime<Utc>,
}

pub async fn feed_events(
    pool: &MySqlPool,
    tenant_id: u64,
    before: DateTime<Utc>,
    prefixes: &[&str],
    limit: u32,
) -> Result<Vec<EventRow>> {
    let mut sql = String::from(
        "select id, event_type, body, created from gateway_event where tenant_id = ? and created <= ?",
    );
    if !prefixes.is_empty() {
        let ors: Vec<&str> = prefixes.iter().map(|_| "event_type like ?").collect();
        sql.push_str(&format!(" and ({})", ors.join(" or ")));
    }
    sql.push_str(" order by created desc, id desc limit ?");
    let mut q = sqlx::query_as(&sql).bind(tenant_id).bind(before);
    for p in prefixes {
        q = q.bind(format!("{p}%"));
    }
    Ok(q.bind(limit).fetch_all(pool).await?)
}

pub async fn events_for_payment(
    pool: &MySqlPool,
    tenant_id: u64,
    payment: &PaymentId,
) -> Result<Vec<EventRow>> {
    Ok(sqlx::query_as(
        "select id, event_type, body, created from gateway_event where tenant_id = ? and payment_id = ? order by id",
    )
    .bind(tenant_id)
    .bind(&payment[..])
    .fetch_all(pool)
    .await?)
}

pub async fn feed_created(
    pool: &MySqlPool,
    tenant_id: u64,
    before: DateTime<Utc>,
    limit: u32,
) -> Result<Vec<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where tenant_id = ? and created <= ? order by created desc, id desc limit ?"
    ))
    .bind(tenant_id)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct FailedDelivery {
    pub id: u64,
    pub event_type: String,
    pub payment_id: Option<String>,
    pub attempts: u32,
    pub last_status: Option<i32>,
    pub last_error: Option<String>,
    pub at: DateTime<Utc>,
    pub abandoned: bool,
    pub delivered: bool,
    pub url: Option<String>,
}

pub async fn feed_failed_deliveries(
    pool: &MySqlPool,
    tenant_id: u64,
    before: DateTime<Utc>,
    limit: u32,
) -> Result<Vec<FailedDelivery>> {
    Ok(sqlx::query_as(
        "select d.id, d.event_type, lower(hex(d.payment_id)) as payment_id, d.attempts, d.last_status, d.last_error, coalesce(d.last_attempt_at, d.created) as at, (d.abandoned_at is not null) as abandoned, (d.delivered_at is not null) as delivered, e.url from webhook_delivery d left join webhook_endpoint e on e.id = d.endpoint_id where d.tenant_id = ? and d.last_error is not null and coalesce(d.last_attempt_at, d.created) <= ? order by at desc, d.id desc limit ?",
    )
    .bind(tenant_id)
    .bind(before)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct DeliveryDetailRow {
    pub id: u64,
    pub event_type: String,
    pub attempts: u32,
    pub created: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub abandoned_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub last_status: Option<i32>,
    pub last_duration_ms: Option<i32>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub url: Option<String>,
}

pub async fn payment_deliveries(
    pool: &MySqlPool,
    tenant_id: u64,
    payment: &PaymentId,
) -> Result<Vec<DeliveryDetailRow>> {
    Ok(sqlx::query_as(
        "select d.id, d.event_type, d.attempts, d.created, d.delivered_at, d.abandoned_at, d.last_error, d.last_status, d.last_duration_ms, d.last_attempt_at, e.url from webhook_delivery d left join webhook_endpoint e on e.id = d.endpoint_id where d.tenant_id = ? and d.payment_id = ? order by d.id",
    )
    .bind(tenant_id)
    .bind(&payment[..])
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct WithdrawalSummary {
    pub in_flight: i64,
    pub in_flight_amount: i64,
}

pub async fn withdrawal_summary(pool: &MySqlPool, tenant_id: u64) -> Result<WithdrawalSummary> {
    Ok(sqlx::query_as(
        "select count(*) as in_flight, cast(coalesce(sum(amount), 0) as signed) as in_flight_amount from withdrawal where tenant_id = ? and status in ('requested', 'approved', 'sending') and purpose = 'payout'",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ProviderStatusRow {
    pub id: u64,
    pub method: String,
    pub provider_type: String,
    pub name: String,
    pub enabled: bool,
    pub platform_managed: bool,
    pub auto_disabled_at: Option<DateTime<Utc>>,
    pub successes: Option<u64>,
    pub errors: Option<u64>,
    pub consecutive_failures: Option<u32>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error_at: Option<DateTime<Utc>>,
}

pub async fn tenant_provider_status(
    pool: &MySqlPool,
    tenant_id: u64,
) -> Result<Vec<ProviderStatusRow>> {
    Ok(sqlx::query_as(
        "select c.id, c.method, c.provider_type, c.name, c.enabled, (c.shared_provider_id is not null) as platform_managed, c.auto_disabled_at, h.successes, h.errors, h.consecutive_failures, h.last_success_at, h.last_error_at from provider_config c left join provider_health h on h.provider_config_id = c.id where c.tenant_id = ? order by c.enabled desc, c.id",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}
