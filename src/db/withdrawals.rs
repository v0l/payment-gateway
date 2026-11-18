use super::*;

const WITHDRAWAL_COLS: &str = "id, tenant_id, amount, currency, purpose, max_fee, fee, destination_kind, destination_id, destination, destination_label, invoice, payment_hash, status, external_id, error, attempts, requested_by, approved_by, auto_approved, created, approved_at, claimed_at, finished_at";
const DESTINATION_COLS: &str = "id, tenant_id, kind, label, value, verified, created_by, usable_from, verified_at, disabled, created";

#[derive(Clone, Debug, FromRow)]
pub struct WithdrawalRow {
    pub id: u64,
    pub tenant_id: u64,
    pub amount: u64,
    pub currency: String,
    pub purpose: String,
    pub max_fee: u64,
    pub fee: Option<u64>,
    pub destination_kind: String,
    pub destination_id: Option<u64>,
    pub destination: Vec<u8>,
    pub destination_label: String,
    pub invoice: Option<Vec<u8>>,
    pub payment_hash: Option<String>,
    pub status: String,
    pub external_id: Option<String>,
    pub error: Option<String>,
    pub attempts: u32,
    pub requested_by: Option<u64>,
    pub approved_by: Option<u64>,
    pub auto_approved: bool,
    pub created: DateTime<Utc>,
    pub approved_at: Option<DateTime<Utc>>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, FromRow)]
pub struct DestinationRow {
    pub id: u64,
    pub tenant_id: u64,
    pub kind: String,
    pub label: String,
    pub value: Vec<u8>,
    pub verified: bool,
    pub created_by: Option<u64>,
    pub usable_from: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
    pub disabled: bool,
    pub created: DateTime<Utc>,
}

pub struct NewDestination<'a> {
    pub tenant_id: u64,
    pub kind: &'a str,
    pub label: &'a str,
    pub sealed_value: &'a [u8],
    pub value_hash: &'a str,
    pub verified: bool,
    pub created_by: Option<u64>,
    pub usable_from: DateTime<Utc>,
}

pub struct NewWithdrawal<'a> {
    pub tenant_id: u64,
    pub amount: u64,
    pub currency: &'a str,
    pub purpose: &'a str,
    pub max_fee: u64,
    pub destination_kind: &'a str,
    pub destination_id: Option<u64>,
    pub sealed_destination: &'a [u8],
    pub destination_label: &'a str,
    pub sealed_invoice: Option<&'a [u8]>,
    pub payment_hash: Option<&'a str>,
    pub status: &'a str,
    pub requested_by: Option<u64>,
    pub approved_by: Option<u64>,
    pub auto_approved: bool,
}

#[derive(Clone, Copy, Debug, Default, FromRow, PartialEq)]
pub struct PayoutLimitRow {
    pub daily_max: Option<u64>,
    pub auto_threshold: Option<u64>,
}

pub async fn insert_destination<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    d: &NewDestination<'_>,
) -> Result<u64> {
    let now = Utc::now();
    let r = sqlx::query(
        "insert into payout_destination (tenant_id, kind, label, value, value_hash, verified, verified_at, created_by, usable_from, created) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(d.tenant_id)
    .bind(d.kind)
    .bind(d.label)
    .bind(d.sealed_value)
    .bind(d.value_hash)
    .bind(d.verified)
    .bind(d.verified.then_some(now))
    .bind(d.created_by)
    .bind(d.usable_from)
    .bind(now)
    .execute(ex)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn list_destinations(pool: &MySqlPool, tenant_id: u64) -> Result<Vec<DestinationRow>> {
    Ok(sqlx::query_as(&format!(
        "select {DESTINATION_COLS} from payout_destination where tenant_id = ? and disabled = 0 order by id"
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_destination<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    id: u64,
) -> Result<Option<DestinationRow>> {
    Ok(sqlx::query_as(&format!(
        "select {DESTINATION_COLS} from payout_destination where tenant_id = ? and id = ? and disabled = 0"
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn disable_destination(pool: &MySqlPool, tenant_id: u64, id: u64) -> Result<bool> {
    let r = sqlx::query(
        "update payout_destination set disabled = 1, value_hash = sha2(concat(id, '-', value_hash), 256) where tenant_id = ? and id = ? and disabled = 0",
    )
    .bind(tenant_id)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn mark_destination_verified<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query("update payout_destination set verified = 1, verified_at = ? where id = ?")
        .bind(now)
        .bind(id)
        .execute(ex)
        .await?;
    Ok(())
}

pub async fn insert_withdrawal<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    w: &NewWithdrawal<'_>,
) -> Result<u64> {
    let now = Utc::now();
    let r = sqlx::query(
        "insert into withdrawal (tenant_id, amount, currency, purpose, max_fee, destination_kind, destination_id, destination, destination_label, invoice, payment_hash, status, requested_by, approved_by, auto_approved, created, approved_at) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(w.tenant_id)
    .bind(w.amount)
    .bind(w.currency)
    .bind(w.purpose)
    .bind(w.max_fee)
    .bind(w.destination_kind)
    .bind(w.destination_id)
    .bind(w.sealed_destination)
    .bind(w.destination_label)
    .bind(w.sealed_invoice)
    .bind(w.payment_hash)
    .bind(w.status)
    .bind(w.requested_by)
    .bind(w.approved_by)
    .bind(w.auto_approved)
    .bind(now)
    .bind((w.status == "approved").then_some(now))
    .execute(ex)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn lock_withdrawal<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<Option<WithdrawalRow>> {
    Ok(sqlx::query_as(&format!(
        "select {WITHDRAWAL_COLS} from withdrawal where id = ? for update"
    ))
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn get_withdrawal(pool: &MySqlPool, id: u64) -> Result<Option<WithdrawalRow>> {
    Ok(sqlx::query_as(&format!(
        "select {WITHDRAWAL_COLS} from withdrawal where id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn withdrawals_for_tenant(
    pool: &MySqlPool,
    tenant_id: u64,
    limit: u32,
) -> Result<Vec<WithdrawalRow>> {
    Ok(sqlx::query_as(&format!(
        "select {WITHDRAWAL_COLS} from withdrawal where tenant_id = ? order by id desc limit ?"
    ))
    .bind(tenant_id)
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await?)
}

pub async fn due_withdrawals(
    pool: &MySqlPool,
    retry_cutoff: DateTime<Utc>,
    limit: u32,
) -> Result<Vec<WithdrawalRow>> {
    Ok(sqlx::query_as(&format!(
        "select {WITHDRAWAL_COLS} from withdrawal where status = 'approved' and (claimed_at is null or claimed_at <= ?) order by id limit ?"
    ))
    .bind(retry_cutoff)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn withdrawals_by_status(
    pool: &MySqlPool,
    status: &str,
    limit: u32,
) -> Result<Vec<WithdrawalRow>> {
    Ok(sqlx::query_as(&format!(
        "select {WITHDRAWAL_COLS} from withdrawal where status = ? order by id limit ?"
    ))
    .bind(status)
    .bind(limit.clamp(1, 500))
    .fetch_all(pool)
    .await?)
}

pub async fn stale_sending(
    pool: &MySqlPool,
    cutoff: DateTime<Utc>,
    limit: u32,
) -> Result<Vec<WithdrawalRow>> {
    Ok(sqlx::query_as(&format!(
        "select {WITHDRAWAL_COLS} from withdrawal where status = 'sending' and claimed_at < ? order by id limit ?"
    ))
    .bind(cutoff)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn sum_recent_withdrawals<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    currency: &str,
    since: DateTime<Utc>,
) -> Result<u64> {
    let v: i64 = sqlx::query_scalar(
        "select cast(coalesce(sum(amount), 0) as signed) from withdrawal where tenant_id = ? and currency = ? and purpose = 'payout' and created >= ? and status in ('requested', 'approved', 'sending', 'completed')",
    )
    .bind(tenant_id)
    .bind(currency)
    .bind(since)
    .fetch_one(ex)
    .await?;
    Ok(v.max(0) as u64)
}

pub async fn approve_withdrawal_row<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    by: Option<u64>,
    auto: bool,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "update withdrawal set status = 'approved', approved_by = ?, auto_approved = ?, approved_at = ? where id = ?",
    )
    .bind(by)
    .bind(auto)
    .bind(now)
    .bind(id)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn claim_withdrawal(pool: &MySqlPool, id: u64, now: DateTime<Utc>) -> Result<bool> {
    let r = sqlx::query(
        "update withdrawal set status = 'sending', claimed_at = ?, attempts = attempts + 1 where id = ? and status = 'approved'",
    )
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn touch_claim(pool: &MySqlPool, id: u64, now: DateTime<Utc>) -> Result<()> {
    sqlx::query("update withdrawal set claimed_at = ? where id = ? and status = 'sending'")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn note_withdrawal_error(pool: &MySqlPool, id: u64, error: &str) -> Result<()> {
    let error: String = error.chars().take(500).collect();
    sqlx::query("update withdrawal set error = ? where id = ? and status = 'sending'")
        .bind(error)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn store_withdrawal_invoice(
    pool: &MySqlPool,
    id: u64,
    sealed_invoice: &[u8],
    payment_hash: &str,
) -> Result<bool> {
    let r = sqlx::query(
        "update withdrawal set invoice = ?, payment_hash = ? where id = ? and status = 'sending'",
    )
    .bind(sealed_invoice)
    .bind(payment_hash)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn requeue_withdrawal(pool: &MySqlPool, id: u64, error: &str) -> Result<bool> {
    let error: String = error.chars().take(500).collect();
    let r = sqlx::query(
        "update withdrawal set status = 'approved', error = ? where id = ? and status = 'sending'",
    )
    .bind(error)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn reclaim_stale(
    pool: &MySqlPool,
    id: u64,
    cutoff: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<bool> {
    let r = sqlx::query(
        "update withdrawal set claimed_at = ?, attempts = attempts + 1 where id = ? and status = 'sending' and claimed_at < ?",
    )
    .bind(now)
    .bind(id)
    .bind(cutoff)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn finish_withdrawal<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    status: &str,
    fee: Option<u64>,
    external_id: Option<&str>,
    error: Option<&str>,
    now: DateTime<Utc>,
) -> Result<()> {
    let error = error.map(|e| e.chars().take(500).collect::<String>());
    sqlx::query(
        "update withdrawal set status = ?, fee = ?, external_id = coalesce(?, external_id), error = ?, finished_at = ? where id = ?",
    )
    .bind(status)
    .bind(fee)
    .bind(external_id)
    .bind(error)
    .bind(now)
    .bind(id)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn get_payout_limits(pool: &MySqlPool, tenant_id: u64) -> Result<PayoutLimitRow> {
    Ok(
        sqlx::query_as("select daily_max, auto_threshold from payout_limit where tenant_id = ?")
            .bind(tenant_id)
            .fetch_optional(pool)
            .await?
            .unwrap_or_default(),
    )
}

pub async fn lock_payout_limits<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
) -> Result<PayoutLimitRow> {
    Ok(
        sqlx::query_as("select daily_max, auto_threshold from payout_limit where tenant_id = ?")
            .bind(tenant_id)
            .fetch_optional(ex)
            .await?
            .unwrap_or_default(),
    )
}

pub async fn set_payout_limits(
    pool: &MySqlPool,
    tenant_id: u64,
    limits: PayoutLimitRow,
) -> Result<()> {
    sqlx::query(
        "insert into payout_limit (tenant_id, daily_max, auto_threshold) values (?, ?, ?) on duplicate key update daily_max = values(daily_max), auto_threshold = values(auto_threshold)",
    )
    .bind(tenant_id)
    .bind(limits.daily_max)
    .bind(limits.auto_threshold)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn withdrawal_counts(pool: &MySqlPool, tenant_id: u64) -> Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as(
        "select status, count(*) from withdrawal where tenant_id = ? group by status order by status",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn oldest_sending(pool: &MySqlPool) -> Result<Option<DateTime<Utc>>> {
    Ok(
        sqlx::query_scalar("select min(claimed_at) from withdrawal where status = 'sending'")
            .fetch_one(pool)
            .await?,
    )
}
