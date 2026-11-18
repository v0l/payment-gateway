use super::*;

pub struct NewPayment<'a> {
    pub id: PaymentId,
    pub tenant_id: u64,
    pub reference: &'a str,
    pub currency: &'a str,
    pub amount: u64,
    pub tax: u64,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub description: Option<&'a str>,
    pub customer_ref: Option<&'a str>,
    pub customer_email: Option<&'a str>,
    pub metadata: Option<&'a str>,
    pub methods: Option<&'a str>,
    pub idempotency_key: Option<&'a str>,
    pub success_url: Option<&'a str>,
    pub cancel_url: Option<&'a str>,
}

pub async fn insert_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    p: &NewPayment<'_>,
) -> Result<()> {
    sqlx::query(
        "insert into payment (id, tenant_id, reference, currency, amount, tax, status, created, expires, description, customer_ref, customer_email, metadata, methods, idempotency_key, success_url, cancel_url) values (?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&p.id[..])
    .bind(p.tenant_id)
    .bind(p.reference)
    .bind(p.currency)
    .bind(p.amount)
    .bind(p.tax)
    .bind(p.created)
    .bind(p.expires)
    .bind(p.description)
    .bind(p.customer_ref)
    .bind(p.customer_email)
    .bind(p.metadata)
    .bind(p.methods)
    .bind(p.idempotency_key)
    .bind(p.success_url)
    .bind(p.cancel_url)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn get_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
) -> Result<Option<PaymentRow>> {
    Ok(
        sqlx::query_as(&format!("select {PAYMENT_COLS} from payment where id = ?"))
            .bind(&id[..])
            .fetch_optional(ex)
            .await?,
    )
}

pub async fn lock_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
) -> Result<Option<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where id = ? for update"
    ))
    .bind(&id[..])
    .fetch_optional(ex)
    .await?)
}

pub async fn payment_by_idempotency(
    pool: &MySqlPool,
    tenant_id: u64,
    key: &str,
) -> Result<Option<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where tenant_id = ? and idempotency_key = ?"
    ))
    .bind(tenant_id)
    .bind(key)
    .fetch_optional(pool)
    .await?)
}

pub async fn payments_by_reference(
    pool: &MySqlPool,
    tenant_id: u64,
    reference: &str,
) -> Result<Vec<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where tenant_id = ? and reference = ? order by created desc limit 100"
    ))
    .bind(tenant_id)
    .bind(reference)
    .fetch_all(pool)
    .await?)
}

pub async fn set_payment_status<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
    from: &[&str],
    to: &str,
) -> Result<bool> {
    let marks = vec!["?"; from.len()].join(",");
    let sql = format!("update payment set status = ? where id = ? and status in ({marks})");
    let mut q = sqlx::query(&sql).bind(to).bind(&id[..]);
    for f in from {
        q = q.bind(*f);
    }
    Ok(q.execute(ex).await?.rows_affected() > 0)
}

pub async fn mark_payment_paid<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
    at: DateTime<Utc>,
) -> Result<()> {
    sqlx::query("update payment set status = 'paid', paid_at = ? where id = ?")
        .bind(at)
        .bind(&id[..])
        .execute(ex)
        .await?;
    Ok(())
}

pub async fn due_pending_payments(pool: &MySqlPool, now: DateTime<Utc>) -> Result<Vec<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where status = 'pending' and expires <= ? order by expires limit 200"
    ))
    .bind(now)
    .fetch_all(pool)
    .await?)
}

pub async fn recent_payments(
    pool: &MySqlPool,
    tenant_id: u64,
    limit: u32,
) -> Result<Vec<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where tenant_id = ? order by created desc limit ?"
    ))
    .bind(tenant_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn live_pending_count<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    now: DateTime<Utc>,
) -> Result<u64> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from payment where tenant_id = ? and status = 'pending' and expires > ?",
    )
    .bind(tenant_id)
    .bind(now)
    .fetch_one(ex)
    .await?;
    Ok(n as u64)
}

pub async fn created_since_count<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<u64> {
    let n: i64 =
        sqlx::query_scalar("select count(*) from payment where tenant_id = ? and created > ?")
            .bind(tenant_id)
            .bind(since)
            .fetch_one(ex)
            .await?;
    Ok(n as u64)
}

pub async fn volume_since_by_currency<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<(String, u64)>> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "select currency, cast(coalesce(sum(amount), 0) as signed) from payment where tenant_id = ? and created > ? group by currency",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(ex)
    .await?;
    Ok(rows.into_iter().map(|(c, n)| (c, n as u64)).collect())
}
