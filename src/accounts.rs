use crate::crypto::hash_api_key;
use anyhow::{Result, anyhow, bail};
use argon2::Argon2;
use argon2::password_hash::rand_core::OsRng;
use argon2::password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use chrono::{DateTime, Duration, Utc};
use rand::RngCore;
use serde::Serialize;
use sqlx::{Executor, FromRow, MySql, MySqlPool};

pub const SESSION_HOURS: i64 = 24 * 14;
pub const INVITE_DAYS: i64 = 7;
pub const MIN_PASSWORD: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Admin,
    Owner,
}

impl Role {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "viewer" => Some(Self::Viewer),
            "admin" => Some(Self::Admin),
            "owner" => Some(Self::Owner),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Admin => "admin",
            Self::Owner => "owner",
        }
    }
}

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Ok(Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow!("hashing failed: {e}"))?
        .to_string())
}

pub fn verify_password(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash)
        .map(|h| {
            Argon2::default()
                .verify_password(password.as_bytes(), &h)
                .is_ok()
        })
        .unwrap_or(false)
}

pub fn burn_password_check(password: &str) {
    static DUMMY: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    let hash = DUMMY.get_or_init(|| hash_password("dummy-password-for-timing").unwrap());
    let _ = verify_password(hash, password);
}

pub fn check_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD {
        bail!("password must be at least {MIN_PASSWORD} characters");
    }
    if password.len() > 256 {
        bail!("password too long");
    }
    Ok(())
}

pub fn normalize_email(raw: &str) -> Option<String> {
    let e = raw.trim().to_lowercase();
    let (local, domain) = e.split_once('@')?;
    let ok = !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && e.len() <= 254
        && !e.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then_some(e)
}

pub fn random_token() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

#[derive(Debug, Clone, FromRow)]
pub struct AccountRow {
    pub id: u64,
    pub email: String,
    pub password_hash: String,
    pub is_operator: bool,
    pub disabled: bool,
    pub created: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
    pub notify: bool,
    pub totp_enabled: bool,
}

#[derive(Debug, Clone, FromRow)]
pub struct SessionRow {
    pub id: u64,
    pub account_id: u64,
    pub csrf: String,
    pub expires: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct InviteRow {
    pub id: u64,
    pub email: String,
    pub tenant_id: Option<u64>,
    pub role: Option<String>,
    pub operator: bool,
    pub expires: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MemberRow {
    pub account_id: u64,
    pub email: String,
    pub role: String,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct ProjectRef {
    pub id: u64,
    pub name: String,
    pub role: String,
}

const ACCOUNT_COLS: &str =
    "id, email, password_hash, is_operator, disabled, created, last_login, notify, totp_enabled";

pub async fn insert_account<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    email: &str,
    password_hash: &str,
    operator: bool,
) -> Result<u64> {
    let r = sqlx::query(
        "insert into account (email, password_hash, is_operator, created) values (?, ?, ?, ?)",
    )
    .bind(email)
    .bind(password_hash)
    .bind(operator)
    .bind(Utc::now())
    .execute(ex)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn account_by_email(pool: &MySqlPool, email: &str) -> Result<Option<AccountRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ACCOUNT_COLS} from account where email = ?"
    ))
    .bind(email)
    .fetch_optional(pool)
    .await?)
}

pub async fn account_by_id(pool: &MySqlPool, id: u64) -> Result<Option<AccountRow>> {
    Ok(
        sqlx::query_as(&format!("select {ACCOUNT_COLS} from account where id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn list_accounts(pool: &MySqlPool) -> Result<Vec<AccountRow>> {
    Ok(
        sqlx::query_as(&format!("select {ACCOUNT_COLS} from account order by id"))
            .fetch_all(pool)
            .await?,
    )
}

pub async fn set_account_disabled(pool: &MySqlPool, id: u64, disabled: bool) -> Result<bool> {
    let r = sqlx::query("update account set disabled = ? where id = ?")
        .bind(disabled)
        .bind(id)
        .execute(pool)
        .await?;
    if disabled {
        sqlx::query("delete from account_session where account_id = ?")
            .bind(id)
            .execute(pool)
            .await?;
    }
    Ok(r.rows_affected() > 0)
}

pub async fn set_password(pool: &MySqlPool, id: u64, password_hash: &str) -> Result<()> {
    sqlx::query("update account set password_hash = ? where id = ?")
        .bind(password_hash)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_notify(pool: &MySqlPool, id: u64, notify: bool) -> Result<()> {
    sqlx::query("update account set notify = ? where id = ?")
        .bind(notify)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn set_email(pool: &MySqlPool, id: u64, email: &str) -> Result<()> {
    sqlx::query("update account set email = ? where id = ?")
        .bind(email)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_all_sessions(pool: &MySqlPool, account_id: u64) -> Result<()> {
    sqlx::query("delete from account_session where account_id = ?")
        .bind(account_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn touch_login(pool: &MySqlPool, id: u64) -> Result<()> {
    sqlx::query("update account set last_login = ? where id = ?")
        .bind(Utc::now())
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn create_session(pool: &MySqlPool, account_id: u64) -> Result<(String, String)> {
    let token = random_token();
    let csrf = random_token()[..32].to_string();
    let now = Utc::now();
    sqlx::query(
        "insert into account_session (account_id, token_hash, csrf, created, expires) values (?, ?, ?, ?, ?)",
    )
    .bind(account_id)
    .bind(hash_api_key(&token))
    .bind(&csrf)
    .bind(now)
    .bind(now + Duration::hours(SESSION_HOURS))
    .execute(pool)
    .await?;
    Ok((token, csrf))
}

pub async fn session_by_token(
    pool: &MySqlPool,
    token: &str,
) -> Result<Option<(SessionRow, AccountRow)>> {
    let Some(session): Option<SessionRow> = sqlx::query_as(
        "select id, account_id, csrf, expires from account_session where token_hash = ?",
    )
    .bind(hash_api_key(token))
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    if session.expires <= Utc::now() {
        delete_session(pool, session.id).await?;
        return Ok(None);
    }
    let Some(account) = account_by_id(pool, session.account_id).await? else {
        return Ok(None);
    };
    if account.disabled {
        return Ok(None);
    }
    Ok(Some((session, account)))
}

pub async fn delete_session(pool: &MySqlPool, id: u64) -> Result<()> {
    sqlx::query("delete from account_session where id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_other_sessions(pool: &MySqlPool, account_id: u64, keep: u64) -> Result<()> {
    sqlx::query("delete from account_session where account_id = ? and id <> ?")
        .bind(account_id)
        .bind(keep)
        .execute(pool)
        .await?;
    Ok(())
}

pub struct NewInvite<'a> {
    pub email: &'a str,
    pub tenant_id: Option<u64>,
    pub role: Option<&'a str>,
    pub operator: bool,
    pub created_by: Option<u64>,
}

pub async fn insert_invite(pool: &MySqlPool, i: &NewInvite<'_>) -> Result<String> {
    let token = random_token();
    let now = Utc::now();
    sqlx::query(
        "insert into invite (token_hash, email, tenant_id, role, operator, created_by, created, expires) values (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(hash_api_key(&token))
    .bind(i.email)
    .bind(i.tenant_id)
    .bind(i.role)
    .bind(i.operator)
    .bind(i.created_by)
    .bind(now)
    .bind(now + Duration::days(INVITE_DAYS))
    .execute(pool)
    .await?;
    Ok(token)
}

pub async fn invite_by_token<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    token: &str,
) -> Result<Option<InviteRow>> {
    Ok(sqlx::query_as(
        "select id, email, tenant_id, role, operator, expires, used_at from invite where token_hash = ?",
    )
    .bind(hash_api_key(token))
    .fetch_optional(ex)
    .await?)
}

pub async fn use_invite<'e, E: Executor<'e, Database = MySql>>(ex: E, id: u64) -> Result<bool> {
    let r = sqlx::query(
        "update invite set used_at = ? where id = ? and used_at is null and expires > ?",
    )
    .bind(Utc::now())
    .bind(id)
    .bind(Utc::now())
    .execute(ex)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn add_member<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    account_id: u64,
    role: Role,
) -> Result<()> {
    sqlx::query(
        "insert into project_member (tenant_id, account_id, role, created) values (?, ?, ?, ?) on duplicate key update role = values(role)",
    )
    .bind(tenant_id)
    .bind(account_id)
    .bind(role.as_str())
    .bind(Utc::now())
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn member_role(
    pool: &MySqlPool,
    tenant_id: u64,
    account_id: u64,
) -> Result<Option<Role>> {
    let r: Option<String> = sqlx::query_scalar(
        "select role from project_member where tenant_id = ? and account_id = ?",
    )
    .bind(tenant_id)
    .bind(account_id)
    .fetch_optional(pool)
    .await?;
    Ok(r.and_then(|r| Role::parse(&r)))
}

pub async fn list_members(pool: &MySqlPool, tenant_id: u64) -> Result<Vec<MemberRow>> {
    Ok(sqlx::query_as(
        "select m.account_id, a.email, m.role, m.created from project_member m join account a on a.id = m.account_id where m.tenant_id = ? order by m.created",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn owner_count(pool: &MySqlPool, tenant_id: u64) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "select count(*) from project_member where tenant_id = ? and role = 'owner'",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?)
}

pub async fn remove_member(pool: &MySqlPool, tenant_id: u64, account_id: u64) -> Result<bool> {
    let r = sqlx::query("delete from project_member where tenant_id = ? and account_id = ?")
        .bind(tenant_id)
        .bind(account_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn projects_for_account(
    pool: &MySqlPool,
    account: &AccountRow,
) -> Result<Vec<ProjectRef>> {
    if account.is_operator {
        return Ok(
            sqlx::query_as("select id, name, 'owner' as role from tenant order by id")
                .fetch_all(pool)
                .await?,
        );
    }
    Ok(sqlx::query_as(
        "select t.id, t.name, m.role from project_member m join tenant t on t.id = m.tenant_id where m.account_id = ? order by t.id",
    )
    .bind(account.id)
    .fetch_all(pool)
    .await?)
}

pub async fn insert_project<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    name: &str,
    api_key_hash: &str,
    sealed_webhook_secret: &[u8],
) -> Result<u64> {
    let r = sqlx::query(
        "insert into tenant (name, api_key_hash, webhook_secret, trusted) values (?, ?, ?, 0)",
    )
    .bind(name)
    .bind(api_key_hash)
    .bind(sealed_webhook_secret)
    .execute(ex)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn set_webhook_secret(pool: &MySqlPool, tenant_id: u64, sealed: &[u8]) -> Result<()> {
    sqlx::query("update tenant set webhook_secret = ? where id = ?")
        .bind(sealed)
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passwords_hash_verify_and_reject_short_ones() {
        let h = hash_password("correct horse battery").unwrap();
        assert!(h.starts_with("$argon2"));
        assert!(verify_password(&h, "correct horse battery"));
        assert!(!verify_password(&h, "correct horse batter"));
        assert!(!verify_password("not a hash", "x"));
        assert_ne!(h, hash_password("correct horse battery").unwrap());
        assert!(check_password("short").is_err());
        assert!(check_password("long enough pw").is_ok());
        assert!(check_password(&"x".repeat(300)).is_err());
    }

    #[test]
    fn emails_are_normalised_and_validated() {
        assert_eq!(
            normalize_email("  Kieran@Example.COM ").unwrap(),
            "kieran@example.com"
        );
        for bad in [
            "",
            "a",
            "a@b",
            "@b.com",
            "a b@c.com",
            "a@.com",
            "a@com.",
            "a\n@b.co",
            "a@b\u{0}.co",
        ] {
            assert!(normalize_email(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn roles_order_by_power() {
        assert!(Role::Owner > Role::Admin && Role::Admin > Role::Viewer);
        assert_eq!(Role::parse("admin"), Some(Role::Admin));
        assert_eq!(Role::parse("root"), None);
    }
}
