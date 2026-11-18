use super::*;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ProjectSettings {
    pub legal_name: Option<String>,
    pub legal_address: Option<String>,
    pub legal_vat_id: Option<String>,
    pub legal_email: Option<String>,
    pub notify_webhook_abandoned: bool,
    pub notify_provider_failing: bool,
    pub notify_refund_failed: bool,
    pub allow_manual_paid: bool,
    pub delete_requested_at: Option<DateTime<Utc>>,
}

pub async fn project_settings(pool: &MySqlPool, tenant_id: u64) -> Result<ProjectSettings> {
    let row: Option<ProjectSettings> = sqlx::query_as(
        "select legal_name, legal_address, legal_vat_id, legal_email, notify_webhook_abandoned, notify_provider_failing, notify_refund_failed, allow_manual_paid, delete_requested_at from project_config where tenant_id = ? and deleted_at is null",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.unwrap_or(ProjectSettings {
        legal_name: None,
        legal_address: None,
        legal_vat_id: None,
        legal_email: None,
        notify_webhook_abandoned: true,
        notify_provider_failing: true,
        notify_refund_failed: true,
        allow_manual_paid: false,
        delete_requested_at: None,
    }))
}

pub async fn set_legal_details(
    pool: &MySqlPool,
    tenant_id: u64,
    name: Option<&str>,
    address: Option<&str>,
    vat_id: Option<&str>,
    email: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "insert into project_config (tenant_id, legal_name, legal_address, legal_vat_id, legal_email) values (?, ?, ?, ?, ?) on duplicate key update legal_name = values(legal_name), legal_address = values(legal_address), legal_vat_id = values(legal_vat_id), legal_email = values(legal_email)",
    )
    .bind(tenant_id)
    .bind(name)
    .bind(address)
    .bind(vat_id)
    .bind(email)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn set_notification_prefs(
    pool: &MySqlPool,
    tenant_id: u64,
    webhook_abandoned: bool,
    provider_failing: bool,
    refund_failed: bool,
) -> Result<()> {
    sqlx::query(
        "insert into project_config (tenant_id, notify_webhook_abandoned, notify_provider_failing, notify_refund_failed) values (?, ?, ?, ?) on duplicate key update notify_webhook_abandoned = values(notify_webhook_abandoned), notify_provider_failing = values(notify_provider_failing), notify_refund_failed = values(notify_refund_failed)",
    )
    .bind(tenant_id)
    .bind(webhook_abandoned)
    .bind(provider_failing)
    .bind(refund_failed)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn notification_enabled(pool: &MySqlPool, tenant_id: u64, kind: &str) -> Result<bool> {
    let column = match kind {
        "webhook_abandoned" => "notify_webhook_abandoned",
        "provider_failing" => "notify_provider_failing",
        "refund_failed" => "notify_refund_failed",
        _ => return Ok(true),
    };
    let v: Option<i64> = sqlx::query_scalar(&format!(
        "select cast({column} as signed) from project_config where tenant_id = ?"
    ))
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(v.unwrap_or(1) != 0)
}

pub async fn store_delete_request(
    pool: &MySqlPool,
    tenant_id: u64,
    token_hash: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "insert into project_config (tenant_id, delete_token_hash, delete_requested_at) values (?, ?, ?) on duplicate key update delete_token_hash = values(delete_token_hash), delete_requested_at = values(delete_requested_at)",
    )
    .bind(tenant_id)
    .bind(token_hash)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn clear_delete_request(pool: &MySqlPool, tenant_id: u64) -> Result<()> {
    sqlx::query(
        "update project_config set delete_token_hash = null, delete_requested_at = null where tenant_id = ?",
    )
    .bind(tenant_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete_request(
    pool: &MySqlPool,
    tenant_id: u64,
) -> Result<Option<(String, DateTime<Utc>)>> {
    let row: Option<(Option<String>, Option<DateTime<Utc>>)> = sqlx::query_as(
        "select delete_token_hash, delete_requested_at from project_config where tenant_id = ? and deleted_at is null",
    )
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|(h, t)| Some((h?, t?))))
}

pub async fn nonzero_balance(pool: &MySqlPool, tenant_id: u64) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from ledger_account where tenant_id = ? and balance <> 0",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}

pub async fn finish_project_delete(pool: &MySqlPool, tenant_id: u64, now: DateTime<Utc>) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("update tenant set enabled = 0 where id = ?")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("update api_key set revoked_at = ? where tenant_id = ? and revoked_at is null")
        .bind(now)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("update webhook_endpoint set enabled = 0 where tenant_id = ?")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("delete from project_member where tenant_id = ?")
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "update project_config set deleted_at = ?, delete_token_hash = null where tenant_id = ?",
    )
    .bind(now)
    .bind(tenant_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}
