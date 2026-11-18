use super::*;

const PLAN_COLS: &str = "id, tenant_id, name, description, amount, currency, interval_unit, interval_count, trial_days, grace_days, active, created";
const SUB_COLS: &str = "id, tenant_id, customer_ref, plan_id, saved_method_id, status, period_index, anchor, anchor_index, current_period_start, current_period_end, trial_end, next_charge, cancel_at_period_end, adjustment, setup_payment_id, claimed_until, paused_at, cancelled_at, cancel_reason, created";
const PERIOD_COLS: &str = "id, subscription_id, period_index, period_start, period_end, amount, currency, status, attempts, failures, payment_id, created, paid_at";

#[derive(Clone, Debug, FromRow)]
pub struct PlanRow {
    pub id: u64,
    pub tenant_id: u64,
    pub name: String,
    pub description: Option<String>,
    pub amount: u64,
    pub currency: String,
    pub interval_unit: String,
    pub interval_count: u32,
    pub trial_days: u32,
    pub grace_days: u32,
    pub active: bool,
    pub created: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
pub struct SubscriptionRow {
    pub id: u64,
    pub tenant_id: u64,
    pub customer_ref: String,
    pub plan_id: u64,
    pub saved_method_id: Option<u64>,
    pub status: String,
    pub period_index: i32,
    pub anchor: DateTime<Utc>,
    pub anchor_index: i32,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub trial_end: Option<DateTime<Utc>>,
    pub next_charge: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub adjustment: i64,
    pub setup_payment_id: Option<Vec<u8>>,
    pub claimed_until: Option<DateTime<Utc>>,
    pub paused_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<String>,
    pub created: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
pub struct PeriodRow {
    pub id: u64,
    pub subscription_id: u64,
    pub period_index: i32,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub amount: u64,
    pub currency: String,
    pub status: String,
    pub attempts: u32,
    pub failures: u32,
    pub payment_id: Option<Vec<u8>>,
    pub created: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,
}

pub struct NewPlan<'a> {
    pub tenant_id: u64,
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub amount: u64,
    pub currency: &'a str,
    pub interval_unit: &'a str,
    pub interval_count: u32,
    pub trial_days: u32,
    pub grace_days: u32,
    pub created: DateTime<Utc>,
}

pub async fn insert_plan(pool: &MySqlPool, p: &NewPlan<'_>) -> Result<u64> {
    let r = sqlx::query(
        "insert into billing_plan (tenant_id, name, description, amount, currency, interval_unit, interval_count, trial_days, grace_days, created) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(p.tenant_id)
    .bind(p.name)
    .bind(p.description)
    .bind(p.amount)
    .bind(p.currency)
    .bind(p.interval_unit)
    .bind(p.interval_count)
    .bind(p.trial_days)
    .bind(p.grace_days)
    .bind(p.created)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn get_plan<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    id: u64,
) -> Result<Option<PlanRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PLAN_COLS} from billing_plan where tenant_id = ? and id = ?"
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn list_plans(
    pool: &MySqlPool,
    tenant_id: u64,
    after: Option<u64>,
    limit: u32,
) -> Result<Vec<PlanRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PLAN_COLS} from billing_plan where tenant_id = ? and id < ? order by id desc limit ?"
    ))
    .bind(tenant_id)
    .bind(after.unwrap_or(u64::MAX))
    .bind(limit + 1)
    .fetch_all(pool)
    .await?)
}

pub async fn archive_plan(pool: &MySqlPool, tenant_id: u64, id: u64) -> Result<bool> {
    let r = sqlx::query("update billing_plan set active = 0 where tenant_id = ? and id = ?")
        .bind(tenant_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub struct NewSubscription<'a> {
    pub tenant_id: u64,
    pub customer_ref: &'a str,
    pub plan_id: u64,
    pub saved_method_id: Option<u64>,
    pub status: &'a str,
    pub period_index: i32,
    pub anchor: DateTime<Utc>,
    pub anchor_index: i32,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub trial_end: Option<DateTime<Utc>>,
    pub next_charge: Option<DateTime<Utc>>,
    pub setup_payment_id: Option<&'a PaymentId>,
    pub created: DateTime<Utc>,
}

pub async fn insert_subscription<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    s: &NewSubscription<'_>,
) -> Result<u64> {
    let r = sqlx::query(
        "insert into subscription (tenant_id, customer_ref, plan_id, saved_method_id, status, period_index, anchor, anchor_index, current_period_start, current_period_end, trial_end, next_charge, setup_payment_id, created) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(s.tenant_id)
    .bind(s.customer_ref)
    .bind(s.plan_id)
    .bind(s.saved_method_id)
    .bind(s.status)
    .bind(s.period_index)
    .bind(s.anchor)
    .bind(s.anchor_index)
    .bind(s.current_period_start)
    .bind(s.current_period_end)
    .bind(s.trial_end)
    .bind(s.next_charge)
    .bind(s.setup_payment_id.map(|p| p.to_vec()))
    .bind(s.created)
    .execute(ex)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn get_subscription<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    id: u64,
) -> Result<Option<SubscriptionRow>> {
    Ok(sqlx::query_as(&format!(
        "select {SUB_COLS} from subscription where tenant_id = ? and id = ?"
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn lock_subscription<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    id: u64,
) -> Result<Option<SubscriptionRow>> {
    Ok(sqlx::query_as(&format!(
        "select {SUB_COLS} from subscription where tenant_id = ? and id = ? for update"
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn lock_subscription_by_setup_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    payment_id: &PaymentId,
) -> Result<Option<SubscriptionRow>> {
    Ok(sqlx::query_as(&format!(
        "select {SUB_COLS} from subscription where setup_payment_id = ? for update"
    ))
    .bind(&payment_id[..])
    .fetch_optional(ex)
    .await?)
}

pub async fn list_subscriptions(
    pool: &MySqlPool,
    tenant_id: u64,
    after: Option<u64>,
    customer_ref: Option<&str>,
    status: Option<&str>,
    limit: u32,
) -> Result<Vec<SubscriptionRow>> {
    let mut sql = format!("select {SUB_COLS} from subscription where tenant_id = ? and id < ?");
    if customer_ref.is_some() {
        sql.push_str(" and customer_ref = ?");
    }
    if status.is_some() {
        sql.push_str(" and status = ?");
    }
    sql.push_str(" order by id desc limit ?");
    let mut q = sqlx::query_as(&sql)
        .bind(tenant_id)
        .bind(after.unwrap_or(u64::MAX));
    if let Some(c) = customer_ref {
        q = q.bind(c);
    }
    if let Some(s) = status {
        q = q.bind(s);
    }
    Ok(q.bind(limit + 1).fetch_all(pool).await?)
}

pub async fn due_subscriptions(
    pool: &MySqlPool,
    now: DateTime<Utc>,
    limit: u32,
) -> Result<Vec<(u64, u64)>> {
    Ok(sqlx::query_as(
        "select tenant_id, id from subscription where status in ('incomplete', 'trialing', 'active', 'past_due') and next_charge <= ? and (claimed_until is null or claimed_until <= ?) order by next_charge limit ?",
    )
    .bind(now)
    .bind(now)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn save_subscription<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    s: &SubscriptionRow,
) -> Result<()> {
    sqlx::query(
        "update subscription set plan_id = ?, saved_method_id = ?, status = ?, period_index = ?, anchor = ?, anchor_index = ?, current_period_start = ?, current_period_end = ?, trial_end = ?, next_charge = ?, cancel_at_period_end = ?, adjustment = ?, claimed_until = ?, paused_at = ?, cancelled_at = ?, cancel_reason = ? where id = ? and tenant_id = ?",
    )
    .bind(s.plan_id)
    .bind(s.saved_method_id)
    .bind(&s.status)
    .bind(s.period_index)
    .bind(s.anchor)
    .bind(s.anchor_index)
    .bind(s.current_period_start)
    .bind(s.current_period_end)
    .bind(s.trial_end)
    .bind(s.next_charge)
    .bind(s.cancel_at_period_end)
    .bind(s.adjustment)
    .bind(s.claimed_until)
    .bind(s.paused_at)
    .bind(s.cancelled_at)
    .bind(&s.cancel_reason)
    .bind(s.id)
    .bind(s.tenant_id)
    .execute(ex)
    .await?;
    Ok(())
}

pub struct NewPeriod<'a> {
    pub subscription_id: u64,
    pub period_index: i32,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub amount: u64,
    pub currency: &'a str,
    pub status: &'a str,
    pub payment_id: Option<&'a PaymentId>,
    pub paid_at: Option<DateTime<Utc>>,
    pub created: DateTime<Utc>,
}

pub async fn insert_period<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    p: &NewPeriod<'_>,
) -> Result<bool> {
    let r = sqlx::query(
        "insert ignore into subscription_period (subscription_id, period_index, period_start, period_end, amount, currency, status, payment_id, paid_at, created) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(p.subscription_id)
    .bind(p.period_index)
    .bind(p.period_start)
    .bind(p.period_end)
    .bind(p.amount)
    .bind(p.currency)
    .bind(p.status)
    .bind(p.payment_id.map(|p| p.to_vec()))
    .bind(p.paid_at)
    .bind(p.created)
    .execute(ex)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn open_period<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    subscription_id: u64,
) -> Result<Option<PeriodRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PERIOD_COLS} from subscription_period where subscription_id = ? and status = 'open' order by period_index desc limit 1 for update"
    ))
    .bind(subscription_id)
    .fetch_optional(ex)
    .await?)
}

pub async fn period_by_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    payment_id: &PaymentId,
) -> Result<Option<PeriodRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PERIOD_COLS} from subscription_period where payment_id = ? order by id desc limit 1"
    ))
    .bind(&payment_id[..])
    .fetch_optional(ex)
    .await?)
}

pub async fn subscription_of_period<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    subscription_id: u64,
) -> Result<Option<(u64, u64)>> {
    Ok(
        sqlx::query_as("select tenant_id, id from subscription where id = ?")
            .bind(subscription_id)
            .fetch_optional(ex)
            .await?,
    )
}

pub async fn save_period<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    p: &PeriodRow,
) -> Result<()> {
    sqlx::query(
        "update subscription_period set status = ?, attempts = ?, failures = ?, payment_id = ?, paid_at = ?, amount = ? where id = ?",
    )
    .bind(&p.status)
    .bind(p.attempts)
    .bind(p.failures)
    .bind(&p.payment_id)
    .bind(p.paid_at)
    .bind(p.amount)
    .bind(p.id)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn list_periods(
    pool: &MySqlPool,
    tenant_id: u64,
    subscription_id: u64,
) -> Result<Vec<PeriodRow>> {
    Ok(sqlx::query_as(&format!(
        "select {} from subscription_period p join subscription s on s.id = p.subscription_id where s.tenant_id = ? and p.subscription_id = ? order by p.period_index desc limit 100",
        PERIOD_COLS
            .split(", ")
            .map(|c| format!("p.{c}"))
            .collect::<Vec<_>>()
            .join(", ")
    ))
    .bind(tenant_id)
    .bind(subscription_id)
    .fetch_all(pool)
    .await?)
}

pub async fn saved_method_for_attempt(
    ex: &mut sqlx::MySqlConnection,
    tenant_id: u64,
    customer_ref: &str,
) -> Result<Option<u64>> {
    let row: Option<(u64,)> = sqlx::query_as(
        "select id from saved_method where tenant_id = ? and customer_ref = ? and enabled = 1 order by id desc limit 1",
    )
    .bind(tenant_id)
    .bind(customer_ref)
    .fetch_optional(ex)
    .await?;
    Ok(row.map(|r| r.0))
}

pub async fn set_period_payment(
    pool: &MySqlPool,
    period_id: u64,
    payment_id: &PaymentId,
) -> Result<()> {
    sqlx::query("update subscription_period set payment_id = ? where id = ?")
        .bind(&payment_id[..])
        .bind(period_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn lock_period<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<Option<PeriodRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PERIOD_COLS} from subscription_period where id = ? for update"
    ))
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn is_setup_payment(pool: &MySqlPool, payment_id: &PaymentId) -> Result<bool> {
    let row: Option<(u64,)> =
        sqlx::query_as("select id from subscription where setup_payment_id = ? limit 1")
            .bind(&payment_id[..])
            .fetch_optional(pool)
            .await?;
    Ok(row.is_some())
}

pub async fn set_setup_payment(
    pool: &MySqlPool,
    id: u64,
    payment_id: &PaymentId,
    next_charge: DateTime<Utc>,
) -> Result<()> {
    sqlx::query("update subscription set setup_payment_id = ?, next_charge = ? where id = ?")
        .bind(&payment_id[..])
        .bind(next_charge)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_subscription(pool: &MySqlPool, id: u64) -> Result<()> {
    sqlx::query("delete from subscription where id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn release_subscription_claim(pool: &MySqlPool, id: u64) -> Result<()> {
    sqlx::query("update subscription set claimed_until = null where id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn add_adjustment(pool: &MySqlPool, tenant_id: u64, id: u64, delta: i64) -> Result<()> {
    sqlx::query(
        "update subscription set adjustment = adjustment + ? where tenant_id = ? and id = ?",
    )
    .bind(delta)
    .bind(tenant_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}
