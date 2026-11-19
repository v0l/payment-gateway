use super::*;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AttentionPayment {
    pub id: String,
    pub reference: String,
    pub description: Option<String>,
    pub customer_ref: Option<String>,
    pub amount: u64,
    pub currency: String,
    pub status: String,
    pub method: Option<String>,
    pub created: DateTime<Utc>,
    pub at: Option<DateTime<Utc>>,
    pub received: Option<i64>,
    pub required: Option<i64>,
}

const ATT_COLS: &str = "lower(hex(p.id)) as id, p.reference, p.description, p.customer_ref, p.amount, p.currency, p.status";

pub async fn open_disputes(pool: &MySqlPool, tenant_id: u64) -> Result<Vec<AttentionPayment>> {
    Ok(sqlx::query_as(&format!(
        "select {ATT_COLS}, (select min(a.method) from payment_attempt a where a.payment_id = p.id and a.status = 'paid') as method, p.created, p.paid_at as at, null as received, null as required from payment p where p.tenant_id = ? and p.dispute_status = 'open' order by p.created limit 50"
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn paid_after_close(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<AttentionPayment>> {
    Ok(sqlx::query_as(&format!(
        "select {ATT_COLS}, a.method, p.created, a.paid_at as at, null as received, null as required from payment p join payment_attempt a on a.payment_id = p.id where p.tenant_id = ? and p.status in ('expired', 'cancelled') and a.status = 'paid' and a.paid_at >= ? order by a.paid_at limit 50"
    ))
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

pub async fn underpaid(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<AttentionPayment>> {
    Ok(sqlx::query_as(&format!(
        "select {ATT_COLS}, a.method, p.created, max(r.created) as at, cast(sum(r.amount) as signed) as received, cast(a.charge_amount as signed) as required from payment p join payment_attempt a on a.payment_id = p.id join attempt_receipt r on r.attempt_id = a.id where p.tenant_id = ? and a.status <> 'paid' and p.status in ('pending', 'expired', 'cancelled') and r.confirmed = 1 and r.created >= ? group by p.id, a.id having received < required order by at limit 50"
    ))
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

pub async fn awaiting_confirmation(
    pool: &MySqlPool,
    tenant_id: u64,
) -> Result<Vec<AttentionPayment>> {
    Ok(sqlx::query_as(&format!(
        "select {ATT_COLS}, a.method, p.created, min(r.created) as at, cast(sum(r.amount) as signed) as received, cast(a.charge_amount as signed) as required from payment p join payment_attempt a on a.payment_id = p.id join attempt_receipt r on r.attempt_id = a.id where p.tenant_id = ? and a.status = 'pending' and r.confirmed = 0 group by p.id, a.id order by at limit 50"
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn declined_unpaid(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<AttentionPayment>> {
    Ok(sqlx::query_as(&format!(
        "select {ATT_COLS}, a.method, p.created, a.created as at, null as received, null as required from payment p join payment_attempt a on a.payment_id = p.id where p.tenant_id = ? and p.status = 'pending' and a.status = 'failed' and a.created >= ? and not exists (select 1 from payment_attempt b where b.payment_id = p.id and b.id > a.id) order by a.created limit 50"
    ))
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AttentionRefund {
    pub refund_id: u64,
    pub id: String,
    pub reference: String,
    pub description: Option<String>,
    pub customer_ref: Option<String>,
    pub amount: u64,
    pub currency: String,
    pub method: String,
    pub error: Option<String>,
    pub at: DateTime<Utc>,
}

pub async fn failed_refunds(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<AttentionRefund>> {
    Ok(sqlx::query_as(
        "select r.id as refund_id, lower(hex(p.id)) as id, p.reference, p.description, p.customer_ref, r.amount, p.currency, a.method, r.error, r.created as at from refund r join payment p on p.id = r.payment_id join payment_attempt a on a.id = r.attempt_id where p.tenant_id = ? and r.status = 'failed' and r.created >= ? and not exists (select 1 from refund s where s.attempt_id = r.attempt_id and s.id > r.id and s.status <> 'failed') order by r.created limit 50",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct FailingEndpoint {
    pub endpoint_id: Option<u64>,
    pub url: Option<String>,
    pub abandoned: i64,
    pub retrying: i64,
    pub last_error: Option<String>,
    pub last_status: Option<i32>,
    pub since: DateTime<Utc>,
}

pub async fn failing_endpoints(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<FailingEndpoint>> {
    Ok(sqlx::query_as(
        "select d.endpoint_id, e.url, cast(sum(d.abandoned_at is not null) as signed) as abandoned, cast(sum(d.abandoned_at is null) as signed) as retrying, substring_index(group_concat(d.last_error order by d.id desc separator '\\n'), '\\n', 1) as last_error, cast(substring_index(group_concat(d.last_status order by d.id desc), ',', 1) as signed) as last_status, min(d.created) as since from webhook_delivery d left join webhook_endpoint e on e.id = d.endpoint_id where d.tenant_id = ? and d.delivered_at is null and d.last_error is not null and d.created >= ? group by d.endpoint_id, e.url order by since limit 20",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AttentionSubscription {
    pub id: u64,
    pub customer_ref: String,
    pub plan_name: Option<String>,
    pub amount: Option<u64>,
    pub currency: Option<String>,
    pub next_charge: Option<DateTime<Utc>>,
}

pub async fn past_due_subscriptions(
    pool: &MySqlPool,
    tenant_id: u64,
) -> Result<Vec<AttentionSubscription>> {
    Ok(sqlx::query_as(
        "select s.id, s.customer_ref, b.name as plan_name, b.amount, b.currency, s.next_charge from subscription s left join billing_plan b on b.id = s.plan_id where s.tenant_id = ? and s.status = 'past_due' order by s.next_charge limit 50",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct AttentionWithdrawal {
    pub id: u64,
    pub amount: u64,
    pub currency: String,
    pub status: String,
    pub destination_label: String,
    pub error: Option<String>,
    pub at: DateTime<Utc>,
}

pub async fn failed_withdrawals(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<AttentionWithdrawal>> {
    Ok(sqlx::query_as(
        "select id, amount, currency, status, destination_label, error, coalesce(finished_at, created) as at from withdrawal where tenant_id = ? and status in ('failed', 'rejected') and coalesce(finished_at, created) >= ? order by at limit 20",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

pub async fn waiting_for_customer(pool: &MySqlPool, tenant_id: u64) -> Result<Vec<CurrencyTotal>> {
    Ok(sqlx::query_as(
        "select currency, count(*) as payments, cast(sum(amount) as signed) as volume from payment p where tenant_id = ? and status = 'pending' and not exists (select 1 from payment_attempt a join attempt_receipt r on r.attempt_id = a.id where a.payment_id = p.id and a.status = 'pending') and not exists (select 1 from payment_attempt f where f.payment_id = p.id and f.status = 'failed' and not exists (select 1 from payment_attempt g where g.payment_id = p.id and g.id > f.id)) group by currency order by currency",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}
