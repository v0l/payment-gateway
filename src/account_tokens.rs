use crate::accounts::random_token;
use crate::crypto::hash_api_key;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sqlx::{Executor, FromRow, MySql, MySqlPool};
use subtle::ConstantTimeEq;

pub const TOKEN_MINUTES: i64 = 60;
pub const RESET: &str = "reset";
pub const EMAIL_CHANGE: &str = "email";

#[derive(Debug, Clone, FromRow)]
pub struct TokenRow {
    pub id: u64,
    pub account_id: u64,
    pub token_hash: String,
    pub new_email: Option<String>,
    pub expires: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

pub async fn issue(
    pool: &MySqlPool,
    account_id: u64,
    kind: &str,
    new_email: Option<&str>,
) -> Result<String> {
    let token = random_token();
    let now = Utc::now();
    sqlx::query(
        "insert into account_token (account_id, kind, token_hash, new_email, created, expires) values (?, ?, ?, ?, ?, ?)",
    )
    .bind(account_id)
    .bind(kind)
    .bind(hash_api_key(&token))
    .bind(new_email)
    .bind(now)
    .bind(now + Duration::minutes(TOKEN_MINUTES))
    .execute(pool)
    .await?;
    Ok(token)
}

pub async fn find_valid<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    kind: &str,
    token: &str,
) -> Result<Option<TokenRow>> {
    let presented = hash_api_key(token);
    let row: Option<TokenRow> = sqlx::query_as(
        "select id, account_id, token_hash, new_email, expires, used_at from account_token where token_hash = ? and kind = ?",
    )
    .bind(&presented)
    .bind(kind)
    .fetch_optional(ex)
    .await?;
    Ok(row.filter(|r| {
        bool::from(r.token_hash.as_bytes().ct_eq(presented.as_bytes()))
            && r.used_at.is_none()
            && r.expires > Utc::now()
    }))
}

pub async fn consume<'e, E: Executor<'e, Database = MySql>>(ex: E, id: u64) -> Result<bool> {
    let r = sqlx::query("update account_token set used_at = ? where id = ? and used_at is null")
        .bind(Utc::now())
        .bind(id)
        .execute(ex)
        .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn revoke_all<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    account_id: u64,
    kind: &str,
) -> Result<()> {
    sqlx::query(
        "update account_token set used_at = ? where account_id = ? and kind = ? and used_at is null",
    )
    .bind(Utc::now())
    .bind(account_id)
    .bind(kind)
    .execute(ex)
    .await?;
    Ok(())
}
