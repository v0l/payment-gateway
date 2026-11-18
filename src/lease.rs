use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use sqlx::MySqlPool;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub const WORKERS_LEASE: &str = "workers";

pub trait Clock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone, Debug, PartialEq, sqlx::FromRow)]
pub struct LeaseRow {
    pub name: String,
    pub holder: String,
    pub acquired: DateTime<Utc>,
    pub expires: DateTime<Utc>,
}

pub struct Lease {
    pool: MySqlPool,
    name: String,
    holder: String,
    ttl: Duration,
    clock: Arc<dyn Clock>,
    held: AtomicBool,
}

impl Lease {
    pub fn new(
        pool: MySqlPool,
        name: &str,
        holder: &str,
        ttl: std::time::Duration,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            pool,
            name: name.to_string(),
            holder: holder.to_string(),
            ttl: Duration::from_std(ttl).expect("lease ttl"),
            clock,
            held: AtomicBool::new(false),
        }
    }

    pub fn holder(&self) -> &str {
        &self.holder
    }

    pub fn ttl(&self) -> std::time::Duration {
        self.ttl.to_std().expect("lease ttl")
    }

    pub fn is_held(&self) -> bool {
        self.held.load(Ordering::SeqCst)
    }

    pub async fn acquire_or_renew(&self) -> Result<bool> {
        let now = self.clock.now();
        let expires = now + self.ttl;
        sqlx::query(
            "insert ignore into worker_lease (name, holder, acquired, expires) values (?, '', ?, ?)",
        )
        .bind(&self.name)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?;
        let mut tx = self.pool.begin().await?;
        let current: Option<LeaseRow> = sqlx::query_as(
            "select name, holder, acquired, expires from worker_lease where name = ? for update",
        )
        .bind(&self.name)
        .fetch_optional(&mut *tx)
        .await?;
        let won = match current {
            None => false,
            Some(row) if row.holder == self.holder => {
                sqlx::query("update worker_lease set expires = ? where name = ?")
                    .bind(expires)
                    .bind(&self.name)
                    .execute(&mut *tx)
                    .await?;
                true
            }
            Some(row) if row.expires <= now => {
                sqlx::query(
                    "update worker_lease set holder = ?, acquired = ?, expires = ? where name = ?",
                )
                .bind(&self.holder)
                .bind(now)
                .bind(expires)
                .bind(&self.name)
                .execute(&mut *tx)
                .await?;
                true
            }
            Some(_) => false,
        };
        tx.commit().await?;
        self.held.store(won, Ordering::SeqCst);
        Ok(won)
    }

    pub async fn release(&self) -> Result<()> {
        self.held.store(false, Ordering::SeqCst);
        sqlx::query("delete from worker_lease where name = ? and holder = ?")
            .bind(&self.name)
            .bind(&self.holder)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn current(&self) -> Result<Option<LeaseRow>> {
        current(&self.pool, &self.name).await
    }
}

pub async fn current(pool: &MySqlPool, name: &str) -> Result<Option<LeaseRow>> {
    Ok(
        sqlx::query_as("select name, holder, acquired, expires from worker_lease where name = ?")
            .bind(name)
            .fetch_optional(pool)
            .await?,
    )
}
