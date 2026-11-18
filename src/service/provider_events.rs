use super::*;
use crate::providers::DisputeState;

impl Service {
    async fn lock_for_lookup(
        &self,
        provider_id: u64,
        lookup: &str,
    ) -> Result<
        Option<(
            sqlx::Transaction<'static, sqlx::MySql>,
            PaymentRow,
            AttemptRow,
        )>,
    > {
        let Some(attempt) = db::find_attempt_by_lookup(&self.pool, provider_id, lookup).await?
        else {
            return Ok(None);
        };
        let pid = db::payment_id_of(&attempt.payment_id);
        let mut tx = self.pool.begin().await?;
        let Some(p) = db::lock_payment(&mut *tx, &pid).await? else {
            return Ok(None);
        };
        Ok(Some((tx, p, attempt)))
    }

    pub(super) async fn apply_provider_refund(
        &self,
        provider_id: u64,
        lookup: &str,
        external_id: &str,
        amount: Option<u64>,
    ) -> Result<()> {
        let Some((mut tx, p, attempt)) = self.lock_for_lookup(provider_id, lookup).await? else {
            return Ok(());
        };
        let pid = p.pid();
        let tenant = self.tenant(p.tenant_id).await?;
        if let Some(existing) = db::refund_by_external_id(&mut *tx, &pid, external_id).await? {
            if existing.status == "pending" || existing.status == "failed" {
                db::finish_refund(&mut *tx, existing.id, "completed", Some(external_id), None)
                    .await?;
                self.settle_refund_totals(&mut tx, &tenant, &p, existing.id, existing.amount)
                    .await?;
                tx.commit().await?;
                self.notify(&pid);
            }
            return Ok(());
        }
        if !matches!(p.status.as_str(), "paid" | "partially_refunded") {
            return Ok(());
        }
        let refunds = db::refunds_for_payment(&mut *tx, &pid).await?;
        let committed: u64 = refunds
            .iter()
            .filter(|r| r.status != "failed")
            .map(|r| r.amount)
            .sum();
        let remaining = p.amount.saturating_sub(committed);
        let charge = amount.unwrap_or(attempt.charge_amount);
        let base = if attempt.charge_currency == p.currency {
            charge
        } else {
            (charge as u128 * p.amount as u128 / attempt.charge_amount.max(1) as u128) as u64
        };
        let base = base.min(remaining);
        if base == 0 {
            return Ok(());
        }
        let refund_id = db::insert_refund(
            &mut *tx,
            &pid,
            attempt.id,
            base,
            charge,
            &attempt.charge_currency,
            None,
        )
        .await?;
        db::finish_refund(&mut *tx, refund_id, "completed", Some(external_id), None).await?;
        self.settle_refund_totals(&mut tx, &tenant, &p, refund_id, base)
            .await?;
        tx.commit().await?;
        self.notify(&pid);
        Ok(())
    }

    async fn settle_refund_totals(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
        tenant: &Tenant,
        p: &PaymentRow,
        refund_id: u64,
        amount: u64,
    ) -> Result<()> {
        let pid = p.pid();
        let total: u64 = db::refunds_for_payment(&mut **tx, &pid)
            .await?
            .iter()
            .filter(|r| r.status == "completed")
            .map(|r| r.amount)
            .sum();
        let status = if total >= p.amount {
            "refunded"
        } else {
            "partially_refunded"
        };
        db::set_payment_status(&mut **tx, &pid, &["paid", "partially_refunded"], status).await?;
        self.emit(
            tx,
            tenant,
            p,
            "refund.completed",
            json!({"refund_id": refund_id, "amount": amount}),
        )
        .await
    }

    pub(super) async fn apply_provider_refund_failure(
        &self,
        provider_id: u64,
        lookup: &str,
        external_id: &str,
        reason: &str,
    ) -> Result<()> {
        let Some((mut tx, p, _)) = self.lock_for_lookup(provider_id, lookup).await? else {
            return Ok(());
        };
        let pid = p.pid();
        let Some(existing) = db::refund_by_external_id(&mut *tx, &pid, external_id).await? else {
            return Ok(());
        };
        if existing.status != "pending" && existing.status != "completed" {
            return Ok(());
        }
        db::finish_refund(&mut *tx, existing.id, "failed", Some(external_id), Some(reason))
            .await?;
        let total: u64 = db::refunds_for_payment(&mut *tx, &pid)
            .await?
            .iter()
            .filter(|r| r.status == "completed")
            .map(|r| r.amount)
            .sum();
        let status = match total {
            0 => "paid",
            t if t >= p.amount => "refunded",
            _ => "partially_refunded",
        };
        db::set_payment_status(&mut *tx, &pid, &["paid", "partially_refunded", "refunded"], status)
            .await?;
        let tenant = self.tenant(p.tenant_id).await?;
        self.emit(
            &mut tx,
            &tenant,
            &p,
            "refund.failed",
            json!({"refund_id": existing.id, "error": reason}),
        )
        .await?;
        tx.commit().await?;
        self.notify(&pid);
        self.notify_refund_failed(tenant.id, reason).await;
        Ok(())
    }

    pub(super) async fn apply_dispute(
        &self,
        provider_id: u64,
        lookup: &str,
        state: DisputeState,
        reason: Option<&str>,
    ) -> Result<()> {
        let Some((mut tx, p, attempt)) = self.lock_for_lookup(provider_id, lookup).await? else {
            return Ok(());
        };
        let pid = p.pid();
        if p.dispute_status.as_deref() == Some(state.as_str()) {
            return Ok(());
        }
        if state != DisputeState::Open && p.dispute_status.is_none() {
            return Ok(());
        }
        if p.dispute_status.as_deref().is_some_and(|s| s != "open") {
            return Ok(());
        }
        db::set_dispute_status(&mut *tx, &pid, Some(state.as_str())).await?;
        let tenant = self.tenant(p.tenant_id).await?;
        let event = match state {
            DisputeState::Open => "payment.disputed",
            _ => "payment.dispute_closed",
        };
        self.emit(
            &mut tx,
            &tenant,
            &p,
            event,
            json!({"attempt_id": attempt.id, "dispute": state.as_str(), "reason": reason}),
        )
        .await?;
        tx.commit().await?;
        self.notify(&pid);
        Ok(())
    }
}
