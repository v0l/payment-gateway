use super::*;

pub struct NewAttempt<'a> {
    pub payment_id: PaymentId,
    pub provider_config_id: u64,
    pub watch_provider_id: u64,
    pub method: &'a str,
    pub option_key: &'a str,
    pub base_amount: u64,
    pub processing_fee: u64,
    pub currency: &'a str,
    pub charge_amount: u64,
    pub charge_currency: &'a str,
    pub rate: f64,
    pub lookup: &'a str,
    pub external_id: Option<&'a str>,
    pub details: &'a str,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub save_method: bool,
}

pub async fn insert_attempt<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    a: &NewAttempt<'_>,
) -> Result<u64> {
    let r = sqlx::query(
        "insert into payment_attempt (payment_id, provider_config_id, watch_provider_id, method, option_key, base_amount, processing_fee, currency, charge_amount, charge_currency, rate, lookup, external_id, details, status, created, expires, save_method) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
    )
    .bind(&a.payment_id[..])
    .bind(a.provider_config_id)
    .bind(a.watch_provider_id)
    .bind(a.method)
    .bind(a.option_key)
    .bind(a.base_amount)
    .bind(a.processing_fee)
    .bind(a.currency)
    .bind(a.charge_amount)
    .bind(a.charge_currency)
    .bind(a.rate)
    .bind(a.lookup)
    .bind(a.external_id)
    .bind(a.details)
    .bind(a.created)
    .bind(a.expires)
    .bind(a.save_method)
    .execute(ex)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn get_attempt<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<Option<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where id = ?"
    ))
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn lock_attempt<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<Option<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where id = ? for update"
    ))
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn attempts_for_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
) -> Result<Vec<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where payment_id = ? order by id"
    ))
    .bind(&id[..])
    .fetch_all(ex)
    .await?)
}

pub async fn find_attempt_by_lookup(
    pool: &MySqlPool,
    watch_provider_id: u64,
    lookup: &str,
) -> Result<Option<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where watch_provider_id = ? and lookup = ? order by id desc limit 1"
    ))
    .bind(watch_provider_id)
    .bind(lookup)
    .fetch_optional(pool)
    .await?)
}

pub async fn set_attempt_status<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    from: &[&str],
    to: &str,
) -> Result<bool> {
    let marks = vec!["?"; from.len()].join(",");
    let sql = format!("update payment_attempt set status = ? where id = ? and status in ({marks})");
    let mut q = sqlx::query(&sql).bind(to).bind(id);
    for f in from {
        q = q.bind(*f);
    }
    Ok(q.execute(ex).await?.rows_affected() > 0)
}

pub async fn mark_attempt_paid<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    at: DateTime<Utc>,
) -> Result<()> {
    sqlx::query("update payment_attempt set status = 'paid', paid_at = ? where id = ?")
        .bind(at)
        .bind(id)
        .execute(ex)
        .await?;
    Ok(())
}

pub async fn due_pending_attempts(pool: &MySqlPool, now: DateTime<Utc>) -> Result<Vec<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where status = 'pending' and expires <= ? order by expires limit 200"
    ))
    .bind(now)
    .fetch_all(pool)
    .await?)
}

pub async fn upsert_receipt<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    attempt_id: u64,
    reference: &str,
    amount: u64,
    confirmed: bool,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "insert into attempt_receipt (attempt_id, ref, amount, confirmed, created) values (?, ?, ?, ?, ?) on duplicate key update confirmed = if(values(confirmed) = 1, 1, confirmed), amount = values(amount)",
    )
    .bind(attempt_id)
    .bind(reference)
    .bind(amount)
    .bind(confirmed)
    .bind(now)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn receipts_for_attempt<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    attempt_id: u64,
) -> Result<Vec<ReceiptRow>> {
    Ok(sqlx::query_as(
        "select id, attempt_id, ref, amount, confirmed from attempt_receipt where attempt_id = ? order by id",
    )
    .bind(attempt_id)
    .fetch_all(ex)
    .await?)
}

pub async fn get_cursor(pool: &MySqlPool, provider_id: u64) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("select cursor_value from provider_cursor where provider_config_id = ?")
            .bind(provider_id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn set_cursor(pool: &MySqlPool, provider_id: u64, value: &str) -> Result<()> {
    sqlx::query(
        "insert into provider_cursor (provider_config_id, cursor_value, updated) values (?, ?, ?) on duplicate key update cursor_value = values(cursor_value), updated = values(updated)",
    )
    .bind(provider_id)
    .bind(value)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn pending_attempts_of_provider(
    pool: &MySqlPool,
    provider_id: u64,
) -> Result<Vec<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where provider_config_id = ? and status = 'pending' order by id limit 500"
    ))
    .bind(provider_id)
    .fetch_all(pool)
    .await?)
}

pub async fn has_unconfirmed_receipts<'e, E: Executor<'e, Database = MySql>>(
    pool: E,
    id: &PaymentId,
) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from attempt_receipt r join payment_attempt a on a.id = r.attempt_id where a.payment_id = ? and r.confirmed = 0 and a.status = 'pending'",
    )
    .bind(&id[..])
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}
