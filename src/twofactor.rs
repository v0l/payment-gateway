use crate::accounts::random_token;
use crate::crypto::{Sealer, hash_api_key};
use crate::totp;
use anyhow::Result;
use chrono::{Duration, Utc};
use sqlx::MySqlPool;

pub const CHALLENGE_MINUTES: i64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Factor {
    Totp,
    Recovery,
}

pub async fn begin_enrolment(
    pool: &MySqlPool,
    sealer: &Sealer,
    account_id: u64,
    secret: &[u8],
) -> Result<bool> {
    let r = sqlx::query(
        "update account set totp_secret = ?, totp_last_step = null where id = ? and totp_enabled = 0",
    )
    .bind(sealer.seal(secret)?)
    .bind(account_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() == 1)
}

async fn load_secret(
    pool: &MySqlPool,
    sealer: &Sealer,
    account_id: u64,
    enabled: bool,
) -> Result<Option<Vec<u8>>> {
    let sealed: Option<Option<Vec<u8>>> =
        sqlx::query_scalar("select totp_secret from account where id = ? and totp_enabled = ?")
            .bind(account_id)
            .bind(enabled)
            .fetch_optional(pool)
            .await?;
    match sealed.flatten() {
        Some(s) => Ok(Some(sealer.open(&s)?)),
        None => Ok(None),
    }
}

pub async fn pending_secret(
    pool: &MySqlPool,
    sealer: &Sealer,
    account_id: u64,
) -> Result<Option<Vec<u8>>> {
    load_secret(pool, sealer, account_id, false).await
}

async fn claim_step(pool: &MySqlPool, account_id: u64, step: u64) -> Result<bool> {
    let r = sqlx::query(
        "update account set totp_last_step = ? where id = ? and (totp_last_step is null or totp_last_step < ?)",
    )
    .bind(step)
    .bind(account_id)
    .bind(step)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn check_totp(
    pool: &MySqlPool,
    sealer: &Sealer,
    account_id: u64,
    enabled: bool,
    code: &str,
    now: u64,
) -> Result<bool> {
    let Some(secret) = load_secret(pool, sealer, account_id, enabled).await? else {
        return Ok(false);
    };
    match totp::matching_step(&secret, code, now) {
        Some(step) => claim_step(pool, account_id, step).await,
        None => Ok(false),
    }
}

pub async fn activate(
    pool: &MySqlPool,
    sealer: &Sealer,
    account_id: u64,
    code: &str,
) -> Result<Option<Vec<String>>> {
    let now = Utc::now().timestamp() as u64;
    if !check_totp(pool, sealer, account_id, false, code, now).await? {
        return Ok(None);
    }
    let r = sqlx::query("update account set totp_enabled = 1 where id = ? and totp_enabled = 0")
        .bind(account_id)
        .execute(pool)
        .await?;
    if r.rows_affected() != 1 {
        return Ok(None);
    }
    Ok(Some(replace_recovery_codes(pool, account_id).await?))
}

pub async fn replace_recovery_codes(pool: &MySqlPool, account_id: u64) -> Result<Vec<String>> {
    let mut tx = pool.begin().await?;
    sqlx::query("delete from account_recovery_code where account_id = ?")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;
    let mut codes = Vec::with_capacity(totp::RECOVERY_CODES);
    for _ in 0..totp::RECOVERY_CODES {
        let code = totp::new_recovery_code();
        sqlx::query(
            "insert into account_recovery_code (account_id, code_hash, created) values (?, ?, ?)",
        )
        .bind(account_id)
        .bind(hash_api_key(&totp::normalize_recovery_code(&code)))
        .bind(Utc::now())
        .execute(&mut *tx)
        .await?;
        codes.push(code);
    }
    tx.commit().await?;
    Ok(codes)
}

pub async fn use_recovery_code(pool: &MySqlPool, account_id: u64, code: &str) -> Result<bool> {
    let r = sqlx::query(
        "update account_recovery_code set used_at = ? where account_id = ? and code_hash = ? and used_at is null",
    )
    .bind(Utc::now())
    .bind(account_id)
    .bind(hash_api_key(&totp::normalize_recovery_code(code)))
    .execute(pool)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn recovery_codes_remaining(pool: &MySqlPool, account_id: u64) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "select count(*) from account_recovery_code where account_id = ? and used_at is null",
    )
    .bind(account_id)
    .fetch_one(pool)
    .await?)
}

pub async fn disable(pool: &MySqlPool, account_id: u64) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "update account set totp_secret = null, totp_enabled = 0, totp_last_step = null where id = ?",
    )
    .bind(account_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query("delete from account_recovery_code where account_id = ?")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("delete from login_challenge where account_id = ?")
        .bind(account_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn create_challenge(pool: &MySqlPool, account_id: u64) -> Result<String> {
    let token = random_token();
    let now = Utc::now();
    sqlx::query(
        "insert into login_challenge (account_id, token_hash, created, expires) values (?, ?, ?, ?)",
    )
    .bind(account_id)
    .bind(hash_api_key(&token))
    .bind(now)
    .bind(now + Duration::minutes(CHALLENGE_MINUTES))
    .execute(pool)
    .await?;
    sqlx::query("delete from login_challenge where expires < ?")
        .bind(now)
        .execute(pool)
        .await?;
    Ok(token)
}

pub async fn challenge_account(pool: &MySqlPool, token: &str) -> Result<Option<u64>> {
    Ok(sqlx::query_scalar(
        "select account_id from login_challenge where token_hash = ? and expires > ?",
    )
    .bind(hash_api_key(token))
    .bind(Utc::now())
    .fetch_optional(pool)
    .await?)
}

pub async fn consume_challenge(pool: &MySqlPool, token: &str) -> Result<bool> {
    let r = sqlx::query("delete from login_challenge where token_hash = ? and expires > ?")
        .bind(hash_api_key(token))
        .bind(Utc::now())
        .execute(pool)
        .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn verify_factor(
    pool: &MySqlPool,
    sealer: &Sealer,
    account_id: u64,
    totp_code: Option<&str>,
    recovery_code: Option<&str>,
) -> Result<Option<Factor>> {
    let now = Utc::now().timestamp() as u64;
    if let Some(code) = totp_code
        && check_totp(pool, sealer, account_id, true, code, now).await?
    {
        return Ok(Some(Factor::Totp));
    }
    if let Some(code) = recovery_code
        && use_recovery_code(pool, account_id, code).await?
    {
        return Ok(Some(Factor::Recovery));
    }
    Ok(None)
}
