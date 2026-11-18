use super::*;
use crate::limits::{LIMIT_CURRENCY, LimitHit, Limits, Usage};

impl Service {
    pub async fn usage(&self, tenant: &Tenant) -> Result<Usage> {
        let now = self.clock.now();
        let since = now - Duration::hours(24);
        let payments_per_day = db::created_since_count(&self.pool, tenant.id, since).await?;
        let pending_payments = db::live_pending_count(&self.pool, tenant.id, now).await?;
        let volume_per_day = self.volume_since(&self.pool, tenant.id, since).await?;
        let providers = db::list_provider_configs(&self.pool, tenant.id, false)
            .await?
            .len() as u64;
        Ok(Usage {
            payments_per_day,
            pending_payments,
            volume_per_day,
            providers,
            webhook_endpoints: db::count_endpoints(&self.pool, tenant.id).await?,
        })
    }

    async fn volume_since<'e, E: sqlx::Executor<'e, Database = sqlx::MySql>>(
        &self,
        ex: E,
        tenant_id: u64,
        since: DateTime<Utc>,
    ) -> Result<u64> {
        let mut total = 0u64;
        for (currency, amount) in db::volume_since_by_currency(ex, tenant_id, since).await? {
            let converted = rates::convert(self.rates.as_ref(), amount, &currency, LIMIT_CURRENCY)
                .await
                .map_err(|e| Error::Upstream(format!("rate lookup failed: {e}")))?;
            total = total.saturating_add(converted);
        }
        Ok(total)
    }

    pub(super) async fn check_new_payment(
        &self,
        tx: &mut sqlx::MySqlConnection,
        tenant: &Tenant,
        amount: u64,
        currency: &str,
    ) -> Result<()> {
        let limits = Limits::for_tenant(tenant);
        let converted = rates::convert(self.rates.as_ref(), amount, currency, LIMIT_CURRENCY)
            .await
            .map_err(|e| Error::Upstream(format!("rate lookup failed: {e}")))?;
        if converted > limits.amount_per_payment {
            return Err(Error::LimitExceeded(LimitHit::new(
                "amount_per_payment",
                converted,
                limits.amount_per_payment,
            )));
        }
        db::lock_tenant(&mut *tx, tenant.id).await?;
        let now = self.clock.now();
        let since = now - Duration::hours(24);
        let today = db::created_since_count(&mut *tx, tenant.id, since).await?;
        if today >= limits.payments_per_day {
            return Err(Error::LimitExceeded(LimitHit::new(
                "payments_per_day",
                today,
                limits.payments_per_day,
            )));
        }
        let pending = db::live_pending_count(&mut *tx, tenant.id, now).await?;
        if pending >= limits.pending_payments {
            return Err(Error::LimitExceeded(LimitHit::new(
                "pending_payments",
                pending,
                limits.pending_payments,
            )));
        }
        let volume = self.volume_since(&mut *tx, tenant.id, since).await?;
        if volume.saturating_add(converted) > limits.volume_per_day {
            return Err(Error::LimitExceeded(LimitHit::new(
                "volume_per_day",
                volume,
                limits.volume_per_day,
            )));
        }
        Ok(())
    }

    pub async fn check_provider_slot(&self, tenant: &Tenant) -> Result<()> {
        let limits = Limits::for_tenant(tenant);
        let count = db::list_provider_configs(&self.pool, tenant.id, false)
            .await?
            .len() as u64;
        if count >= limits.providers {
            return Err(Error::LimitExceeded(LimitHit::new(
                "providers",
                count,
                limits.providers,
            )));
        }
        Ok(())
    }

    pub fn check_webhook_endpoint(&self, tenant: &Tenant, new_url: Option<&str>) -> Result<()> {
        let limits = Limits::for_tenant(tenant);
        if new_url.is_some() && limits.webhook_endpoints < 1 {
            return Err(Error::LimitExceeded(LimitHit::new(
                "webhook_endpoints",
                1,
                limits.webhook_endpoints,
            )));
        }
        Ok(())
    }

    pub async fn check_new_endpoint(&self, tenant: &Tenant) -> Result<()> {
        let max = Limits::for_tenant(tenant).webhook_endpoints;
        let current = self.usage(tenant).await?.webhook_endpoints;
        if current >= max {
            return Err(Error::LimitExceeded(LimitHit::new(
                "webhook_endpoints",
                current + 1,
                max,
            )));
        }
        Ok(())
    }

    pub(super) fn check_attempt_count(tenant: &Tenant, existing: usize) -> Result<()> {
        let max = Limits::for_tenant(tenant).attempts_per_payment;
        if existing as u64 >= max {
            return Err(Error::LimitExceeded(LimitHit::new(
                "attempts_per_payment",
                existing as u64,
                max,
            )));
        }
        Ok(())
    }
}
