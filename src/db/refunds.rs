use super::*;

pub async fn insert_refund<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    payment_id: &PaymentId,
    attempt_id: u64,
    amount: u64,
    charge_amount: u64,
    currency: &str,
    destination: Option<&str>,
) -> Result<u64> {
    let r = sqlx::query(
        "insert into refund (payment_id, attempt_id, amount, charge_amount, currency, destination, status, created) values (?, ?, ?, ?, ?, ?, 'pending', ?)",
    )
    .bind(&payment_id[..])
    .bind(attempt_id)
    .bind(amount)
    .bind(charge_amount)
    .bind(currency)
    .bind(destination)
    .bind(Utc::now())
    .execute(ex)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn finish_refund<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    status: &str,
    external_id: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query("update refund set status = ?, external_id = ?, error = ? where id = ?")
        .bind(status)
        .bind(external_id)
        .bind(error)
        .bind(id)
        .execute(ex)
        .await?;
    Ok(())
}

pub async fn refunds_for_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
) -> Result<Vec<RefundRow>> {
    Ok(sqlx::query_as(&format!(
        "select {REFUND_COLS} from refund where payment_id = ? order by id"
    ))
    .bind(&id[..])
    .fetch_all(ex)
    .await?)
}

pub async fn get_refund<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<Option<RefundRow>> {
    Ok(
        sqlx::query_as(&format!("select {REFUND_COLS} from refund where id = ?"))
            .bind(id)
            .fetch_optional(ex)
            .await?,
    )
}
