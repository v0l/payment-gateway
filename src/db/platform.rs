use super::*;

const PLAN_COLS: &str = "id, name, live, monthly_price, currency, included_volume, fee_bps, fee_base, limit_payments_per_day, limit_pending_payments, limit_amount_per_payment, limit_volume_per_day, limit_providers, limit_webhook_endpoints, limit_attempts_per_payment, active, created";
const ACCOUNT_COLS: &str = "tenant_id, plan_id, status, status_since, period_start, period_end, period_fee, credit, created";
const INVOICE_COLS: &str = "id, tenant_id, uniq_key, kind, plan_id, period_start, period_end, currency, usage_count, usage_volume, line_items, subtotal, credit_applied, total, status, reason, payment_id, pay_attempts, past_due_notified, due_at, paid_at, created";

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct PlatformPlanRow {
    pub id: u64,
    pub name: String,
    pub live: bool,
    pub monthly_price: u64,
    pub currency: String,
    pub included_volume: u64,
    pub fee_bps: u32,
    pub fee_base: u64,
    #[sqlx(flatten)]
    pub limits: crate::limits::LimitOverrides,
    pub active: bool,
    pub created: DateTime<Utc>,
}

impl PlatformPlanRow {
    pub fn fees(&self) -> crate::platform::Fees {
        crate::platform::Fees {
            monthly_price: self.monthly_price,
            included_volume: self.included_volume,
            fee_bps: self.fee_bps,
            fee_base: self.fee_base,
        }
    }
}

#[derive(Clone, Debug, FromRow)]
pub struct PlatformAccountRow {
    pub tenant_id: u64,
    pub plan_id: u64,
    pub status: String,
    pub status_since: Option<DateTime<Utc>>,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub period_fee: u64,
    pub credit: u64,
    pub created: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
pub struct PlatformInvoiceRow {
    pub id: u64,
    pub tenant_id: u64,
    pub uniq_key: String,
    pub kind: String,
    pub plan_id: u64,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub currency: String,
    pub usage_count: u64,
    pub usage_volume: u64,
    pub line_items: String,
    pub subtotal: u64,
    pub credit_applied: u64,
    pub total: u64,
    pub status: String,
    pub reason: Option<String>,
    pub payment_id: Option<Vec<u8>>,
    pub pay_attempts: u32,
    pub past_due_notified: bool,
    pub due_at: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,
    pub created: DateTime<Utc>,
}

pub struct PlatformPlanInput<'a> {
    pub name: &'a str,
    pub live: bool,
    pub monthly_price: u64,
    pub currency: &'a str,
    pub included_volume: u64,
    pub fee_bps: u32,
    pub fee_base: u64,
    pub limits: crate::limits::LimitOverrides,
    pub active: bool,
}

pub async fn list_platform_plans(pool: &MySqlPool) -> Result<Vec<PlatformPlanRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PLAN_COLS} from platform_plan order by id"
    ))
    .fetch_all(pool)
    .await?)
}

pub async fn get_platform_plan<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<Option<PlatformPlanRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PLAN_COLS} from platform_plan where id = ?"
    ))
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn default_platform_plan(
    pool: &MySqlPool,
    live: bool,
) -> Result<Option<PlatformPlanRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PLAN_COLS} from platform_plan where live = ? and active = 1 order by id limit 1"
    ))
    .bind(live)
    .fetch_optional(pool)
    .await?)
}

pub async fn insert_platform_plan(
    pool: &MySqlPool,
    p: &PlatformPlanInput<'_>,
    now: DateTime<Utc>,
) -> Result<u64> {
    let r = sqlx::query(
        "insert into platform_plan (name, live, monthly_price, currency, included_volume, fee_bps, fee_base, limit_payments_per_day, limit_pending_payments, limit_amount_per_payment, limit_volume_per_day, limit_providers, limit_webhook_endpoints, limit_attempts_per_payment, active, created) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(p.name)
    .bind(p.live)
    .bind(p.monthly_price)
    .bind(p.currency)
    .bind(p.included_volume)
    .bind(p.fee_bps)
    .bind(p.fee_base)
    .bind(p.limits.limit_payments_per_day)
    .bind(p.limits.limit_pending_payments)
    .bind(p.limits.limit_amount_per_payment)
    .bind(p.limits.limit_volume_per_day)
    .bind(p.limits.limit_providers)
    .bind(p.limits.limit_webhook_endpoints)
    .bind(p.limits.limit_attempts_per_payment)
    .bind(p.active)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn update_platform_plan(
    pool: &MySqlPool,
    id: u64,
    p: &PlatformPlanInput<'_>,
) -> Result<bool> {
    let r = sqlx::query(
        "update platform_plan set name = ?, live = ?, monthly_price = ?, currency = ?, included_volume = ?, fee_bps = ?, fee_base = ?, limit_payments_per_day = ?, limit_pending_payments = ?, limit_amount_per_payment = ?, limit_volume_per_day = ?, limit_providers = ?, limit_webhook_endpoints = ?, limit_attempts_per_payment = ?, active = ? where id = ?",
    )
    .bind(p.name)
    .bind(p.live)
    .bind(p.monthly_price)
    .bind(p.currency)
    .bind(p.included_volume)
    .bind(p.fee_bps)
    .bind(p.fee_base)
    .bind(p.limits.limit_payments_per_day)
    .bind(p.limits.limit_pending_payments)
    .bind(p.limits.limit_amount_per_payment)
    .bind(p.limits.limit_volume_per_day)
    .bind(p.limits.limit_providers)
    .bind(p.limits.limit_webhook_endpoints)
    .bind(p.limits.limit_attempts_per_payment)
    .bind(p.active)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn platform_tenant(pool: &MySqlPool) -> Result<Option<u64>> {
    let id: Option<Option<u64>> =
        sqlx::query_scalar("select tenant_id from platform_setting where id = 1")
            .fetch_optional(pool)
            .await?;
    Ok(id.flatten())
}

pub async fn set_platform_tenant(pool: &MySqlPool, tenant_id: Option<u64>) -> Result<()> {
    sqlx::query("update platform_setting set tenant_id = ? where id = 1")
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_platform_account<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
) -> Result<Option<PlatformAccountRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ACCOUNT_COLS} from platform_account where tenant_id = ?"
    ))
    .bind(tenant_id)
    .fetch_optional(ex)
    .await?)
}

pub async fn lock_platform_account<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
) -> Result<Option<PlatformAccountRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ACCOUNT_COLS} from platform_account where tenant_id = ? for update"
    ))
    .bind(tenant_id)
    .fetch_optional(ex)
    .await?)
}

pub async fn insert_platform_account<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    a: &PlatformAccountRow,
) -> Result<bool> {
    let r = sqlx::query(
        "insert ignore into platform_account (tenant_id, plan_id, status, period_start, period_end, period_fee, credit, created) values (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(a.tenant_id)
    .bind(a.plan_id)
    .bind(&a.status)
    .bind(a.period_start)
    .bind(a.period_end)
    .bind(a.period_fee)
    .bind(a.credit)
    .bind(a.created)
    .execute(ex)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn save_platform_account<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    a: &PlatformAccountRow,
) -> Result<()> {
    sqlx::query(
        "update platform_account set plan_id = ?, status = ?, status_since = ?, period_start = ?, period_end = ?, period_fee = ?, credit = ? where tenant_id = ?",
    )
    .bind(a.plan_id)
    .bind(&a.status)
    .bind(a.status_since)
    .bind(a.period_start)
    .bind(a.period_end)
    .bind(a.period_fee)
    .bind(a.credit)
    .bind(a.tenant_id)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn platform_accounts_due(pool: &MySqlPool, now: DateTime<Utc>) -> Result<Vec<u64>> {
    Ok(sqlx::query_scalar(
        "select tenant_id from platform_account where period_end <= ? order by tenant_id",
    )
    .bind(now)
    .fetch_all(pool)
    .await?)
}

pub async fn platform_standing<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("select status from platform_account where tenant_id = ?")
            .bind(tenant_id)
            .fetch_optional(ex)
            .await?,
    )
}

pub async fn tenants_to_review(pool: &MySqlPool) -> Result<Vec<u64>> {
    Ok(sqlx::query_scalar(
        "select distinct tenant_id from platform_invoice where status = 'open' union select tenant_id from platform_account where status <> 'current'",
    )
    .fetch_all(pool)
    .await?)
}

pub struct NewPlatformInvoice<'a> {
    pub tenant_id: u64,
    pub uniq_key: &'a str,
    pub kind: &'a str,
    pub plan_id: u64,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub currency: &'a str,
    pub usage_count: u64,
    pub usage_volume: u64,
    pub lines: &'a str,
    pub subtotal: u64,
    pub credit_applied: u64,
    pub total: u64,
    pub status: &'a str,
    pub due_at: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,
    pub created: DateTime<Utc>,
}

pub async fn insert_platform_invoice<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    i: &NewPlatformInvoice<'_>,
) -> Result<Option<u64>> {
    let r = sqlx::query(
        "insert ignore into platform_invoice (tenant_id, uniq_key, kind, plan_id, period_start, period_end, currency, usage_count, usage_volume, line_items, subtotal, credit_applied, total, status, due_at, paid_at, created) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(i.tenant_id)
    .bind(i.uniq_key)
    .bind(i.kind)
    .bind(i.plan_id)
    .bind(i.period_start)
    .bind(i.period_end)
    .bind(i.currency)
    .bind(i.usage_count)
    .bind(i.usage_volume)
    .bind(i.lines)
    .bind(i.subtotal)
    .bind(i.credit_applied)
    .bind(i.total)
    .bind(i.status)
    .bind(i.due_at)
    .bind(i.paid_at)
    .bind(i.created)
    .execute(ex)
    .await?;
    Ok((r.rows_affected() > 0).then(|| r.last_insert_id()))
}

pub async fn get_platform_invoice<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    id: u64,
) -> Result<Option<PlatformInvoiceRow>> {
    Ok(sqlx::query_as(&format!(
        "select {INVOICE_COLS} from platform_invoice where tenant_id = ? and id = ?"
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn lock_platform_invoice<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<Option<PlatformInvoiceRow>> {
    Ok(sqlx::query_as(&format!(
        "select {INVOICE_COLS} from platform_invoice where id = ? for update"
    ))
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn list_platform_invoices(
    pool: &MySqlPool,
    tenant_id: u64,
    limit: u32,
) -> Result<Vec<PlatformInvoiceRow>> {
    Ok(sqlx::query_as(&format!(
        "select {INVOICE_COLS} from platform_invoice where tenant_id = ? order by id desc limit ?"
    ))
    .bind(tenant_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn open_platform_invoices<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
) -> Result<Vec<PlatformInvoiceRow>> {
    Ok(sqlx::query_as(&format!(
        "select {INVOICE_COLS} from platform_invoice where tenant_id = ? and status = 'open' order by due_at, id"
    ))
    .bind(tenant_id)
    .fetch_all(ex)
    .await?)
}

pub async fn all_open_platform_invoices(pool: &MySqlPool) -> Result<Vec<PlatformInvoiceRow>> {
    Ok(sqlx::query_as(&format!(
        "select {INVOICE_COLS} from platform_invoice where status = 'open' order by id limit 500"
    ))
    .fetch_all(pool)
    .await?)
}

pub async fn tenants_without_platform_account(pool: &MySqlPool) -> Result<Vec<u64>> {
    Ok(sqlx::query_scalar(
        "select t.id from tenant t left join platform_account a on a.tenant_id = t.id where a.tenant_id is null order by t.id",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn platform_invoice_by_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    payment_id: &PaymentId,
) -> Result<Option<PlatformInvoiceRow>> {
    Ok(sqlx::query_as(&format!(
        "select {INVOICE_COLS} from platform_invoice where payment_id = ?"
    ))
    .bind(&payment_id[..])
    .fetch_optional(ex)
    .await?)
}

pub async fn set_platform_invoice_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    payment_id: &PaymentId,
    expected_attempts: u32,
) -> Result<bool> {
    let r = sqlx::query(
        "update platform_invoice set payment_id = ?, pay_attempts = pay_attempts + 1 where id = ? and status = 'open' and pay_attempts = ?",
    )
    .bind(&payment_id[..])
    .bind(id)
    .bind(expected_attempts)
    .execute(ex)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn settle_platform_invoice<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    status: &str,
    reason: Option<&str>,
    now: DateTime<Utc>,
) -> Result<bool> {
    let r = sqlx::query(
        "update platform_invoice set status = ?, reason = ?, paid_at = ? where id = ? and status = 'open'",
    )
    .bind(status)
    .bind(reason)
    .bind(now)
    .bind(id)
    .execute(ex)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn mark_invoice_past_due_notified<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<bool> {
    let r = sqlx::query(
        "update platform_invoice set past_due_notified = 1 where id = ? and past_due_notified = 0",
    )
    .bind(id)
    .execute(ex)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn metered_paid<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Result<Vec<(String, u64, u64)>> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "select currency, cast(coalesce(sum(amount), 0) as signed), cast(count(*) as signed) from payment where tenant_id = ? and status = 'paid' and paid_at >= ? and paid_at < ? group by currency",
    )
    .bind(tenant_id)
    .bind(start)
    .bind(end)
    .fetch_all(ex)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(c, v, n)| (c, v as u64, n as u64))
        .collect())
}
