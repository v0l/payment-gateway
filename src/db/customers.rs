use super::*;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct CustomerSummary {
    pub customer_ref: String,
    pub payments: i64,
    pub paid: i64,
    pub last_payment: DateTime<Utc>,
}

pub async fn list_customers(
    pool: &MySqlPool,
    tenant_id: u64,
    search: Option<&str>,
    limit: u32,
) -> Result<Vec<CustomerSummary>> {
    let mut sql = String::from(
        "select customer_ref, count(*) as payments, cast(coalesce(sum(status in ('paid','partially_refunded','refunded')), 0) as signed) as paid, max(created) as last_payment from payment where tenant_id = ? and customer_ref is not null",
    );
    if search.is_some() {
        sql.push_str(" and customer_ref like ?");
    }
    sql.push_str(" group by customer_ref order by last_payment desc limit ?");
    let mut q = sqlx::query_as(&sql).bind(tenant_id);
    if let Some(s) = search {
        let escaped = s
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        q = q.bind(format!("{escaped}%"));
    }
    Ok(q.bind(limit).fetch_all(pool).await?)
}

pub async fn customer_saved_methods(
    pool: &MySqlPool,
    tenant_id: u64,
    customer_ref: &str,
) -> Result<Vec<SavedMethodRow>> {
    Ok(
        list_saved_methods(pool, tenant_id, Some(customer_ref), None, 200)
            .await?
            .into_iter()
            .take(200)
            .collect(),
    )
}
