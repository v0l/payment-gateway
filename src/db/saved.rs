use super::*;

pub struct NewSavedMethod<'a> {
    pub tenant_id: u64,
    pub customer_ref: &'a str,
    pub provider_config_id: u64,
    pub external_customer_id: &'a str,
    pub external_id: &'a str,
    pub brand: Option<&'a str>,
    pub last4: Option<&'a str>,
    pub exp_month: Option<u16>,
    pub exp_year: Option<u16>,
}

pub async fn upsert_saved_method<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    m: &NewSavedMethod<'_>,
) -> Result<()> {
    sqlx::query(
        "insert into saved_method (tenant_id, customer_ref, provider_config_id, external_customer_id, external_id, brand, last4, exp_month, exp_year, created) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) on duplicate key update enabled = 1",
    )
    .bind(m.tenant_id)
    .bind(m.customer_ref)
    .bind(m.provider_config_id)
    .bind(m.external_customer_id)
    .bind(m.external_id)
    .bind(m.brand)
    .bind(m.last4)
    .bind(m.exp_month)
    .bind(m.exp_year)
    .bind(Utc::now())
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn list_saved_methods(
    pool: &MySqlPool,
    tenant_id: u64,
    customer_ref: Option<&str>,
    after: Option<u64>,
    limit: u32,
) -> Result<Vec<SavedMethodRow>> {
    let mut sql = format!(
        "select {SAVED_COLS} from saved_method where tenant_id = ? and enabled = 1 and id > ?"
    );
    if customer_ref.is_some() {
        sql.push_str(" and customer_ref = ?");
    }
    sql.push_str(" order by id limit ?");
    let mut q = sqlx::query_as(&sql)
        .bind(tenant_id)
        .bind(after.unwrap_or(0));
    if let Some(c) = customer_ref {
        q = q.bind(c);
    }
    Ok(q.bind(limit + 1).fetch_all(pool).await?)
}

pub async fn get_saved_method_by_external(
    pool: &MySqlPool,
    provider_config_id: u64,
    external_id: &str,
) -> Result<Option<SavedMethodRow>> {
    Ok(sqlx::query_as(&format!(
        "select {SAVED_COLS} from saved_method where provider_config_id = ? and external_id = ?"
    ))
    .bind(provider_config_id)
    .bind(external_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn get_saved_method(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
) -> Result<Option<SavedMethodRow>> {
    Ok(sqlx::query_as(&format!(
        "select {SAVED_COLS} from saved_method where tenant_id = ? and id = ? and enabled = 1"
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn disable_saved_method(pool: &MySqlPool, tenant_id: u64, id: u64) -> Result<bool> {
    let r = sqlx::query("update saved_method set enabled = 0 where tenant_id = ? and id = ?")
        .bind(tenant_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}
