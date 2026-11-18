use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::MySqlPool;

pub async fn notification_recipients(pool: &MySqlPool, tenant_id: u64) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "select a.email from project_member m join account a on a.id = m.account_id where m.tenant_id = ? and m.role = 'owner' and a.notify = 1 and a.disabled = 0 order by a.id",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn operator_emails(pool: &MySqlPool) -> Result<Vec<String>> {
    Ok(sqlx::query_scalar(
        "select email from account where is_operator = 1 and disabled = 0 order by id",
    )
    .fetch_all(pool)
    .await?)
}

pub async fn claim_notification(
    pool: &MySqlPool,
    tenant_id: u64,
    kind: &str,
    now: DateTime<Utc>,
    cutoff: DateTime<Utc>,
) -> Result<bool> {
    let updated = sqlx::query(
        "update notification_throttle set sent_at = ? where tenant_id = ? and kind = ? and sent_at < ?",
    )
    .bind(now)
    .bind(tenant_id)
    .bind(kind)
    .bind(cutoff)
    .execute(pool)
    .await?;
    if updated.rows_affected() > 0 {
        return Ok(true);
    }
    let inserted = sqlx::query(
        "insert ignore into notification_throttle (tenant_id, kind, sent_at) values (?, ?, ?)",
    )
    .bind(tenant_id)
    .bind(kind)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(inserted.rows_affected() > 0)
}
