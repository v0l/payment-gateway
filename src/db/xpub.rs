use super::*;

#[derive(Clone, Debug, FromRow)]
pub struct XpubAddressRow {
    pub idx: u32,
    pub address: String,
}

pub async fn allocate_xpub_index<F>(
    pool: &MySqlPool,
    provider_id: u64,
    now: DateTime<Utc>,
    derive: F,
) -> Result<(u32, String)>
where
    F: FnOnce(u32) -> Result<String>,
{
    sqlx::query("insert ignore into xpub_state (provider_config_id, next_index) values (?, 0)")
        .bind(provider_id)
        .execute(pool)
        .await?;
    let mut tx = pool.begin().await?;
    let index = sqlx::query(
        "update xpub_state set next_index = last_insert_id(next_index) + 1 where provider_config_id = ?",
    )
    .bind(provider_id)
    .execute(&mut *tx)
    .await?
    .last_insert_id() as u32;
    let address = derive(index)?;
    sqlx::query(
        "insert into xpub_address (provider_config_id, idx, address, issued) values (?, ?, ?, ?)",
    )
    .bind(provider_id)
    .bind(index)
    .bind(&address)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok((index, address))
}

pub async fn xpub_next_index(pool: &MySqlPool, provider_id: u64) -> Result<u32> {
    Ok(
        sqlx::query_scalar("select next_index from xpub_state where provider_config_id = ?")
            .bind(provider_id)
            .fetch_optional(pool)
            .await?
            .unwrap_or(0),
    )
}

pub async fn xpub_watch_addresses(
    pool: &MySqlPool,
    provider_id: u64,
    expired_since: DateTime<Utc>,
) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "select distinct lookup from payment_attempt where watch_provider_id = ? and (status = 'pending' or (status = 'expired' and expires > ?)) order by id limit 2000",
    )
    .bind(provider_id)
    .bind(expired_since)
    .fetch_all(pool)
    .await?)
}

pub async fn request_xpub_rescan(
    pool: &MySqlPool,
    provider_id: u64,
    from: u32,
    count: u32,
) -> Result<()> {
    sqlx::query("insert ignore into xpub_state (provider_config_id, next_index) values (?, 0)")
        .bind(provider_id)
        .execute(pool)
        .await?;
    sqlx::query(
        "update xpub_state set rescan_from = ?, rescan_count = ? where provider_config_id = ?",
    )
    .bind(from)
    .bind(count)
    .bind(provider_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn take_xpub_rescan(pool: &MySqlPool, provider_id: u64) -> Result<Option<(u32, u32)>> {
    let mut tx = pool.begin().await?;
    let row: Option<(Option<u32>, Option<u32>)> = sqlx::query_as(
        "select rescan_from, rescan_count from xpub_state where provider_config_id = ? for update",
    )
    .bind(provider_id)
    .fetch_optional(&mut *tx)
    .await?;
    let Some((Some(from), Some(count))) = row else {
        return Ok(None);
    };
    sqlx::query(
        "update xpub_state set rescan_from = null, rescan_count = null where provider_config_id = ?",
    )
    .bind(provider_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some((from, count)))
}

pub async fn xpub_attempt_addresses(
    pool: &MySqlPool,
    provider_id: u64,
    from: u32,
    count: u32,
) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "select address from xpub_address where provider_config_id = ? and idx >= ? and idx < ? order by idx",
    )
    .bind(provider_id)
    .bind(from)
    .bind(from.saturating_add(count))
    .fetch_all(pool)
    .await?)
}

pub async fn xpub_issued(pool: &MySqlPool, provider_id: u64) -> Result<Vec<XpubAddressRow>> {
    Ok(sqlx::query_as(
        "select idx, address from xpub_address where provider_config_id = ? order by idx desc limit 20",
    )
    .bind(provider_id)
    .fetch_all(pool)
    .await?)
}
