use super::*;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct StatRow {
    pub status: String,
    pub currency: String,
    pub count: i64,
    pub total: Option<i64>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct DayRow {
    pub day: String,
    pub currency: String,
    pub count: i64,
    pub total: Option<i64>,
}

pub async fn list_payments(
    pool: &MySqlPool,
    tenant_id: u64,
    status: Option<&str>,
    query: Option<&str>,
    before: Option<(DateTime<Utc>, Vec<u8>)>,
    limit: u32,
) -> Result<Vec<PaymentRow>> {
    let mut sql = format!("select {PAYMENT_COLS} from payment where tenant_id = ?");
    if status.is_some() {
        sql.push_str(" and status = ?");
    }
    if query.is_some() {
        sql.push_str(" and (reference like ? or hex(id) like ?)");
    }
    if before.is_some() {
        sql.push_str(" and (created < ? or (created = ? and id < ?))");
    }
    sql.push_str(" order by created desc, id desc limit ?");
    let mut q = sqlx::query_as(&sql).bind(tenant_id);
    if let Some(s) = status {
        q = q.bind(s);
    }
    if let Some(text) = query {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        q = q
            .bind(format!("{escaped}%"))
            .bind(format!("{}%", escaped.to_uppercase()));
    }
    if let Some((at, id)) = before {
        q = q.bind(at).bind(at).bind(id);
    }
    Ok(q.bind(limit).fetch_all(pool).await?)
}

pub async fn payment_stats(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<StatRow>> {
    Ok(sqlx::query_as(
        "select status, currency, count(*) as count, cast(sum(amount) as signed) as total from payment where tenant_id = ? and created >= ? group by status, currency order by status, currency",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

pub async fn paid_by_day(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<DayRow>> {
    Ok(sqlx::query_as(
        "select cast(date(paid_at) as char) as day, currency, count(*) as count, cast(sum(amount) as signed) as total from payment where tenant_id = ? and paid_at >= ? and status in ('paid','partially_refunded','refunded') group by date(paid_at), currency order by date(paid_at)",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

pub async fn method_counts(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as(
        "select a.method, count(*) from payment_attempt a join payment p on p.id = a.payment_id where p.tenant_id = ? and a.status = 'paid' and a.paid_at >= ? group by a.method order by 2 desc",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

pub async fn count_all<'e, E: Executor<'e, Database = MySql>>(ex: E, table: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(&format!("select count(*) from {table}"))
        .fetch_one(ex)
        .await?)
}
