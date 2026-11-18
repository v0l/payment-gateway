use super::*;

#[derive(Default, Clone, Debug)]
pub struct SearchFilter {
    pub status: Option<String>,
    pub query: Option<String>,
    pub customer: Option<String>,
    pub method: Option<String>,
    pub created_from: Option<DateTime<Utc>>,
    pub created_to: Option<DateTime<Utc>>,
    pub min_amount: Option<u64>,
    pub max_amount: Option<u64>,
    pub currency: Option<String>,
    pub before: Option<(DateTime<Utc>, Vec<u8>)>,
}

fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

pub async fn search_payments(
    pool: &MySqlPool,
    tenant_id: u64,
    f: &SearchFilter,
    limit: u32,
) -> Result<Vec<PaymentRow>> {
    let mut sql = format!("select {PAYMENT_COLS} from payment where tenant_id = ?");
    if f.status.is_some() {
        sql.push_str(" and status = ?");
    }
    if f.query.is_some() {
        sql.push_str(" and (reference like ? or hex(id) like ?)");
    }
    if f.customer.is_some() {
        sql.push_str(" and (customer_ref = ? or customer_email = ?)");
    }
    if f.method.is_some() {
        sql.push_str(
            " and exists (select 1 from payment_attempt a where a.payment_id = payment.id and a.method = ?)",
        );
    }
    if f.created_from.is_some() {
        sql.push_str(" and created >= ?");
    }
    if f.created_to.is_some() {
        sql.push_str(" and created < ?");
    }
    if f.min_amount.is_some() {
        sql.push_str(" and amount >= ?");
    }
    if f.max_amount.is_some() {
        sql.push_str(" and amount <= ?");
    }
    if f.currency.is_some() {
        sql.push_str(" and currency = ?");
    }
    if f.before.is_some() {
        sql.push_str(" and (created < ? or (created = ? and id < ?))");
    }
    sql.push_str(" order by created desc, id desc limit ?");
    let mut q = sqlx::query_as(&sql).bind(tenant_id);
    if let Some(v) = &f.status {
        q = q.bind(v.clone());
    }
    if let Some(text) = &f.query {
        let escaped = escape_like(text);
        q = q
            .bind(format!("{escaped}%"))
            .bind(format!("{}%", escaped.to_uppercase()));
    }
    if let Some(v) = &f.customer {
        q = q.bind(v.clone()).bind(v.clone());
    }
    if let Some(v) = &f.method {
        q = q.bind(v.clone());
    }
    if let Some(v) = f.created_from {
        q = q.bind(v);
    }
    if let Some(v) = f.created_to {
        q = q.bind(v);
    }
    if let Some(v) = f.min_amount {
        q = q.bind(v);
    }
    if let Some(v) = f.max_amount {
        q = q.bind(v);
    }
    if let Some(v) = &f.currency {
        q = q.bind(v.clone());
    }
    if let Some((at, id)) = &f.before {
        q = q.bind(*at).bind(*at).bind(id.clone());
    }
    Ok(q.bind(limit).fetch_all(pool).await?)
}

pub async fn stale_pending_payments(
    pool: &MySqlPool,
    tenant_id: u64,
    older_than: DateTime<Utc>,
    limit: u32,
) -> Result<Vec<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where tenant_id = ? and status = 'pending' and created < ? order by created limit ?"
    ))
    .bind(tenant_id)
    .bind(older_than)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct NoteRow {
    pub id: u64,
    pub account_id: u64,
    pub body: String,
    pub created: DateTime<Utc>,
}

pub async fn add_payment_note(
    pool: &MySqlPool,
    tenant_id: u64,
    payment: &PaymentId,
    account_id: u64,
    body: &str,
) -> Result<u64> {
    let r = sqlx::query(
        "insert into payment_note (tenant_id, payment_id, account_id, body, created) values (?, ?, ?, ?, ?)",
    )
    .bind(tenant_id)
    .bind(&payment[..])
    .bind(account_id)
    .bind(body)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn payment_notes(
    pool: &MySqlPool,
    tenant_id: u64,
    payment: &PaymentId,
) -> Result<Vec<NoteRow>> {
    Ok(sqlx::query_as(
        "select id, account_id, body, created from payment_note where tenant_id = ? and payment_id = ? order by id",
    )
    .bind(tenant_id)
    .bind(&payment[..])
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct SavedViewRow {
    pub id: u64,
    pub name: String,
    pub filter: String,
}

pub async fn list_saved_views(
    pool: &MySqlPool,
    tenant_id: u64,
    account_id: u64,
) -> Result<Vec<SavedViewRow>> {
    Ok(sqlx::query_as(
        "select id, name, filter from saved_view where tenant_id = ? and account_id = ? order by name",
    )
    .bind(tenant_id)
    .bind(account_id)
    .fetch_all(pool)
    .await?)
}

pub async fn upsert_saved_view(
    pool: &MySqlPool,
    tenant_id: u64,
    account_id: u64,
    name: &str,
    filter: &str,
) -> Result<()> {
    sqlx::query(
        "insert into saved_view (tenant_id, account_id, name, filter, created) values (?, ?, ?, ?, ?) on duplicate key update filter = values(filter)",
    )
    .bind(tenant_id)
    .bind(account_id)
    .bind(name)
    .bind(filter)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn count_saved_views(pool: &MySqlPool, tenant_id: u64, account_id: u64) -> Result<i64> {
    Ok(
        sqlx::query_scalar(
            "select count(*) from saved_view where tenant_id = ? and account_id = ?",
        )
        .bind(tenant_id)
        .bind(account_id)
        .fetch_one(pool)
        .await?,
    )
}

pub async fn delete_saved_view(
    pool: &MySqlPool,
    tenant_id: u64,
    account_id: u64,
    id: u64,
) -> Result<bool> {
    let r = sqlx::query("delete from saved_view where tenant_id = ? and account_id = ? and id = ?")
        .bind(tenant_id)
        .bind(account_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn manual_paid_allowed(pool: &MySqlPool, tenant_id: u64) -> Result<bool> {
    let v: Option<i64> = sqlx::query_scalar(
        "select cast(allow_manual_paid as signed) from project_config where tenant_id = ?",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(v.unwrap_or(0) != 0)
}

pub async fn set_manual_paid_allowed(
    pool: &MySqlPool,
    tenant_id: u64,
    allowed: bool,
) -> Result<()> {
    sqlx::query(
        "insert into project_config (tenant_id, allow_manual_paid) values (?, ?) on duplicate key update allow_manual_paid = values(allow_manual_paid)",
    )
    .bind(tenant_id)
    .bind(allowed)
    .execute(pool)
    .await?;
    Ok(())
}
