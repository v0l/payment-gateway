use crate::audit::{Action, Actor, Event, Origin};
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::MySqlPool;

#[derive(Clone, Debug, Default)]
pub struct RetentionPolicy {
    pub delivery_body_days: Option<i64>,
    pub unpaid_payment_days: Option<i64>,
    pub session_days: Option<i64>,
}

impl RetentionPolicy {
    pub fn from_env() -> Result<Self> {
        fn days(name: &str) -> Result<Option<i64>> {
            match std::env::var(name) {
                Ok(v) if !v.trim().is_empty() => {
                    let n: i64 = v
                        .trim()
                        .parse()
                        .map_err(|_| anyhow::anyhow!("{name} must be a whole number of days"))?;
                    anyhow::ensure!(n >= 0, "{name} must not be negative");
                    Ok(Some(n))
                }
                _ => Ok(None),
            }
        }
        Ok(Self {
            delivery_body_days: days("GATEWAY_RETAIN_DELIVERY_BODY_DAYS")?,
            unpaid_payment_days: days("GATEWAY_RETAIN_UNPAID_PAYMENT_DAYS")?,
            session_days: days("GATEWAY_RETAIN_SESSION_DAYS")?,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.delivery_body_days.is_none()
            && self.unpaid_payment_days.is_none()
            && self.session_days.is_none()
    }
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct PurgeReport {
    pub delivery_bodies_cleared: u64,
    pub unpaid_payments_deleted: u64,
    pub sessions_deleted: u64,
    pub account_tokens_deleted: u64,
    pub throttle_rows_deleted: u64,
}

const BATCH: i64 = 500;

pub async fn purge(
    pool: &MySqlPool,
    policy: &RetentionPolicy,
    now: DateTime<Utc>,
    actor: Actor,
) -> Result<PurgeReport> {
    let report = purge_tables(pool, policy, now).await?;
    if report != PurgeReport::default() {
        Event::new(Action::RetentionPurged, actor)
            .details(serde_json::json!({
                "delivery_bodies_cleared": report.delivery_bodies_cleared,
                "unpaid_payments_deleted": report.unpaid_payments_deleted,
                "sessions_deleted": report.sessions_deleted,
                "account_tokens_deleted": report.account_tokens_deleted,
                "throttle_rows_deleted": report.throttle_rows_deleted,
            }))
            .record(pool, &Origin::default())
            .await;
    }
    Ok(report)
}

async fn purge_tables(
    pool: &MySqlPool,
    policy: &RetentionPolicy,
    now: DateTime<Utc>,
) -> Result<PurgeReport> {
    let mut report = PurgeReport::default();
    if let Some(days) = policy.unpaid_payment_days {
        report.unpaid_payments_deleted =
            purge_unpaid_payments(pool, now - Duration::days(days)).await?;
    }
    if let Some(days) = policy.delivery_body_days {
        report.delivery_bodies_cleared =
            clear_delivery_bodies(pool, now - Duration::days(days)).await?;
    }
    if let Some(days) = policy.session_days {
        let r = sqlx::query("delete from account_session where expires < ?")
            .bind(now - Duration::days(days))
            .execute(pool)
            .await?;
        report.sessions_deleted = r.rows_affected();
        let cutoff = now - Duration::days(days);
        let r = sqlx::query(
            "delete from account_token where expires < ? or (used_at is not null and used_at < ?)",
        )
        .bind(cutoff)
        .bind(cutoff)
        .execute(pool)
        .await?;
        report.account_tokens_deleted = r.rows_affected();
        let r = sqlx::query("delete from notification_throttle where sent_at < ?")
            .bind(cutoff)
            .execute(pool)
            .await?;
        report.throttle_rows_deleted = r.rows_affected();
    }
    Ok(report)
}

async fn clear_delivery_bodies(pool: &MySqlPool, cutoff: DateTime<Utc>) -> Result<u64> {
    let mut total = 0;
    loop {
        let r = sqlx::query(
            "update webhook_delivery set body = '' where body <> '' and created < ? and (delivered_at is not null or abandoned_at is not null) limit ?",
        )
        .bind(cutoff)
        .bind(BATCH)
        .execute(pool)
        .await?;
        total += r.rows_affected();
        if r.rows_affected() < BATCH as u64 {
            return Ok(total);
        }
    }
}

async fn purge_unpaid_payments(pool: &MySqlPool, cutoff: DateTime<Utc>) -> Result<u64> {
    let mut total = 0;
    loop {
        let ids: Vec<Vec<u8>> = sqlx::query_scalar(
            "select p.id from payment p where p.status in ('expired', 'cancelled') and p.paid_at is null and p.expires < ?
             and not exists (select 1 from payment_attempt a where a.payment_id = p.id and (a.status = 'paid' or a.paid_at is not null))
             and not exists (select 1 from refund r where r.payment_id = p.id)
             and not exists (select 1 from payment_attempt a join attempt_receipt t on t.attempt_id = a.id where a.payment_id = p.id and t.confirmed = 1)
             and not exists (select 1 from webhook_delivery d where d.payment_id = p.id and d.delivered_at is null and d.abandoned_at is null)
             limit ?",
        )
        .bind(cutoff)
        .bind(BATCH)
        .fetch_all(pool)
        .await?;
        if ids.is_empty() {
            return Ok(total);
        }
        for id in &ids {
            let mut tx = pool.begin().await?;
            sqlx::query("delete from attempt_receipt where attempt_id in (select id from payment_attempt where payment_id = ?)")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("delete from payment_attempt where payment_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("delete from webhook_delivery where payment_id = ?")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            let r = sqlx::query("delete from payment where id = ? and status in ('expired', 'cancelled') and paid_at is null")
                .bind(id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            total += r.rows_affected();
        }
    }
}
