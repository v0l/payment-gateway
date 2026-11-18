use super::*;

#[derive(Clone, Debug, FromRow)]
pub struct EndpointRow {
    pub id: u64,
    pub tenant_id: u64,
    pub url: String,
    pub secret: Vec<u8>,
    pub description: Option<String>,
    pub events: Option<String>,
    pub enabled: bool,
    pub is_default: bool,
    pub created: DateTime<Utc>,
}

const ENDPOINT_COLS: &str =
    "id, tenant_id, url, secret, description, events, enabled, is_default, created";

pub async fn insert_endpoint(
    pool: &MySqlPool,
    tenant_id: u64,
    url: &str,
    sealed_secret: &[u8],
    description: Option<&str>,
    events: Option<&str>,
) -> Result<u64> {
    let r = sqlx::query(
        "insert into webhook_endpoint (tenant_id, url, secret, description, events) values (?, ?, ?, ?, ?)",
    )
    .bind(tenant_id)
    .bind(url)
    .bind(sealed_secret)
    .bind(description)
    .bind(events)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn set_default_endpoint_url(
    pool: &MySqlPool,
    tenant_id: u64,
    url: Option<&str>,
) -> Result<()> {
    let existing: Option<u64> = sqlx::query_scalar(
        "select id from webhook_endpoint where tenant_id = ? and is_default = 1 limit 1",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    match (existing, url) {
        (Some(id), Some(url)) => {
            sqlx::query("update webhook_endpoint set url = ? where id = ?")
                .bind(url)
                .bind(id)
                .execute(pool)
                .await?;
        }
        (Some(id), None) => {
            sqlx::query("delete from webhook_endpoint where id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        }
        (None, Some(url)) => {
            sqlx::query(
                "insert into webhook_endpoint (tenant_id, url, secret, is_default) select id, ?, webhook_secret, 1 from tenant where id = ?",
            )
            .bind(url)
            .bind(tenant_id)
            .execute(pool)
            .await?;
        }
        (None, None) => {}
    }
    Ok(())
}

pub async fn set_default_endpoint_secret(
    pool: &MySqlPool,
    tenant_id: u64,
    sealed: &[u8],
) -> Result<()> {
    sqlx::query("update webhook_endpoint set secret = ? where tenant_id = ? and is_default = 1")
        .bind(sealed)
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_endpoints<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
) -> Result<Vec<EndpointRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ENDPOINT_COLS} from webhook_endpoint where tenant_id = ? order by id"
    ))
    .bind(tenant_id)
    .fetch_all(ex)
    .await?)
}

pub async fn get_endpoint(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
) -> Result<Option<EndpointRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ENDPOINT_COLS} from webhook_endpoint where tenant_id = ? and id = ?"
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn get_endpoint_any_tenant(pool: &MySqlPool, id: u64) -> Result<Option<EndpointRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ENDPOINT_COLS} from webhook_endpoint where id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn update_endpoint(pool: &MySqlPool, tenant_id: u64, e: &EndpointRow) -> Result<bool> {
    let r = sqlx::query(
        "update webhook_endpoint set url = ?, description = ?, events = ?, enabled = ? where tenant_id = ? and id = ?",
    )
    .bind(&e.url)
    .bind(&e.description)
    .bind(&e.events)
    .bind(e.enabled)
    .bind(tenant_id)
    .bind(e.id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn set_endpoint_secret(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
    sealed: &[u8],
) -> Result<bool> {
    let r = sqlx::query("update webhook_endpoint set secret = ? where tenant_id = ? and id = ?")
        .bind(sealed)
        .bind(tenant_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn delete_endpoint(pool: &MySqlPool, tenant_id: u64, id: u64) -> Result<bool> {
    let r = sqlx::query("delete from webhook_endpoint where tenant_id = ? and id = ?")
        .bind(tenant_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn count_endpoints(pool: &MySqlPool, tenant_id: u64) -> Result<u64> {
    let n: i64 = sqlx::query_scalar("select count(*) from webhook_endpoint where tenant_id = ?")
        .bind(tenant_id)
        .fetch_one(pool)
        .await?;
    Ok(n as u64)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct EndpointStats {
    pub endpoint_id: Option<u64>,
    pub delivered: i64,
    pub failed: i64,
    pub pending: i64,
}

pub async fn endpoint_stats(pool: &MySqlPool, tenant_id: u64) -> Result<Vec<EndpointStats>> {
    Ok(sqlx::query_as(
        "select endpoint_id, cast(coalesce(sum(delivered_at is not null), 0) as signed) as delivered, cast(coalesce(sum(abandoned_at is not null), 0) as signed) as failed, cast(coalesce(sum(delivered_at is null and abandoned_at is null), 0) as signed) as pending from webhook_delivery where tenant_id = ? group by endpoint_id",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn insert_event<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    payment_id: &PaymentId,
    event_id: &str,
    event_type: &str,
    body: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "insert into gateway_event (tenant_id, payment_id, event_id, event_type, body, created) values (?, ?, ?, ?, ?, ?)",
    )
    .bind(tenant_id)
    .bind(&payment_id[..])
    .bind(event_id)
    .bind(event_type)
    .bind(body)
    .bind(now)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn events_after(
    pool: &MySqlPool,
    tenant_id: u64,
    after: u64,
    event_type: Option<&str>,
    limit: u32,
) -> Result<Vec<(u64, String)>> {
    let mut sql = String::from("select id, body from gateway_event where tenant_id = ? and id > ?");
    if event_type.is_some() {
        sql.push_str(" and event_type = ?");
    }
    sql.push_str(" order by id limit ?");
    let mut q = sqlx::query_as(&sql).bind(tenant_id).bind(after);
    if let Some(t) = event_type {
        q = q.bind(t);
    }
    Ok(q.bind(limit).fetch_all(pool).await?)
}
