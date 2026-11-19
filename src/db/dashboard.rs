use super::*;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct VolumeRow {
    pub currency: String,
    pub method: String,
    pub payments: i64,
    pub volume: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct DailyVolume {
    pub day: String,
    pub currency: String,
    pub payments: i64,
    pub volume: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TopCustomer {
    pub customer_ref: String,
    pub payments: i64,
    pub currency: String,
    pub volume: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct PendingValue {
    pub currency: String,
    pub payments: i64,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DashboardStats {
    pub volume: Vec<VolumeRow>,
    pub daily: Vec<DailyVolume>,
    pub paid: i64,
    pub failed: i64,
    pub success_rate: Option<f64>,
    pub avg_settlement_seconds: Option<f64>,
    pub pending: Vec<PendingValue>,
    pub failed_webhooks: i64,
    pub top_customers: Vec<TopCustomer>,
}

pub async fn dashboard_stats(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<DashboardStats> {
    let volume: Vec<VolumeRow> = sqlx::query_as(&format!(
        "select p.currency, coalesce((select min(a.method) from payment_attempt a where a.payment_id = p.id and a.status = 'paid'), 'manual') as method, count(*) as payments, cast(sum(p.amount) as signed) as volume from payment p where p.tenant_id = ? and p.created >= ? and p.status in {PAID_STATES} group by p.currency, method order by p.currency, method"
    ))
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?;
    let daily: Vec<DailyVolume> = sqlx::query_as(&format!(
        "select date_format(created, '%Y-%m-%d') as day, currency, count(*) as payments, cast(sum(amount) as signed) as volume from payment where tenant_id = ? and created >= ? and status in {PAID_STATES} group by day, currency order by day, currency"
    ))
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?;
    let (paid, failed): (i64, i64) = sqlx::query_as(&format!(
        "select cast(coalesce(sum(status in {PAID_STATES}), 0) as signed), cast(coalesce(sum(status in ('expired','cancelled')), 0) as signed) from payment where tenant_id = ? and created >= ?"
    ))
    .bind(tenant_id)
    .bind(since)
    .fetch_one(pool)
    .await?;
    let success_rate = (paid + failed > 0).then(|| paid as f64 / (paid + failed) as f64);
    let avg: Option<f64> = sqlx::query_scalar(&format!(
        "select cast(avg(timestampdiff(second, created, paid_at)) as double) from payment where tenant_id = ? and created >= ? and paid_at is not null and status in {PAID_STATES}"
    ))
    .bind(tenant_id)
    .bind(since)
    .fetch_one(pool)
    .await?;
    let pending: Vec<PendingValue> = sqlx::query_as(
        "select currency, count(*) as payments, cast(sum(amount) as signed) as value from payment where tenant_id = ? and status = 'pending' group by currency order by currency",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    let failed_webhooks: i64 = sqlx::query_scalar(
        "select count(*) from webhook_delivery where tenant_id = ? and abandoned_at is not null and created >= ?",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_one(pool)
    .await?;
    let top_customers: Vec<TopCustomer> = sqlx::query_as(&format!(
        "select customer_ref, count(*) as payments, currency, cast(sum(amount) as signed) as volume from payment where tenant_id = ? and created >= ? and customer_ref is not null and status in {PAID_STATES} group by customer_ref, currency order by volume desc limit 5"
    ))
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(DashboardStats {
        volume,
        daily,
        paid,
        failed,
        success_rate,
        avg_settlement_seconds: avg,
        pending,
        failed_webhooks,
        top_customers,
    })
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TenantOverview {
    pub id: u64,
    pub name: String,
    pub enabled: bool,
    pub trusted: bool,
    pub payments: i64,
    pub volume_30d: i64,
    pub last_payment: Option<DateTime<Utc>>,
    pub limit_payments_per_day: Option<u64>,
    pub limit_pending_payments: Option<u64>,
    pub limit_volume_per_day: Option<u64>,
}

pub async fn tenant_overviews(
    pool: &MySqlPool,
    since: DateTime<Utc>,
) -> Result<Vec<TenantOverview>> {
    Ok(sqlx::query_as(&format!(
        "select t.id, t.name, t.enabled, t.trusted, (select count(*) from payment p where p.tenant_id = t.id) as payments, (select cast(coalesce(sum(p.amount), 0) as signed) from payment p where p.tenant_id = t.id and p.created >= ? and p.status in {PAID_STATES}) as volume_30d, (select max(p.created) from payment p where p.tenant_id = t.id) as last_payment, t.limit_payments_per_day, t.limit_pending_payments, t.limit_volume_per_day from tenant t order by t.id"
    ))
    .bind(since)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SearchHit {
    pub tenant_id: u64,
    pub payment_id: String,
    pub reference: String,
    pub status: String,
    pub matched: String,
}

pub async fn global_search(pool: &MySqlPool, term: &str, limit: u32) -> Result<Vec<SearchHit>> {
    let escaped = term
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let prefix = format!("{}%", escaped.to_uppercase());
    Ok(sqlx::query_as(
        "select tenant_id, lower(hex(id)) as payment_id, reference, status, matched from (
           select tenant_id, id, reference, status, 'id' as matched from payment where hex(id) like ?
           union
           select tenant_id, id, reference, status, 'reference' from payment where reference = ?
           union
           select p.tenant_id, p.id, p.reference, p.status, 'invoice' from payment p join payment_attempt a on a.payment_id = p.id where a.lookup = ?
           union
           select p.tenant_id, p.id, p.reference, p.status, 'txid' from payment p join payment_attempt a on a.payment_id = p.id join attempt_receipt r on r.attempt_id = a.id where r.ref = ?
         ) hits limit ?",
    )
    .bind(prefix)
    .bind(term)
    .bind(term)
    .bind(term)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn tenant_has_pending(pool: &MySqlPool, tenant_id: u64) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from payment where tenant_id = ? and status = 'pending'",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}
