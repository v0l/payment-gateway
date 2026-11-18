use super::*;

impl Service {
    pub async fn cancel_payment(&self, tenant: &Tenant, id: &PaymentId) -> Result<PaymentRow> {
        let preliminary = self.payment_for_tenant(tenant, id).await?;
        let mut tx = self.pool.begin().await?;
        let p = db::lock_payment(&mut *tx, id)
            .await?
            .ok_or(Error::NotFound)?;
        if p.status != "pending" {
            return Err(Error::Conflict(format!("payment is {}", p.status)));
        }
        if db::has_unconfirmed_receipts(&self.pool, id).await? {
            return Err(Error::Conflict("funds are already on their way".into()));
        }
        db::set_payment_status(&mut *tx, id, &["pending"], "cancelled").await?;
        let mut cancelled = vec![];
        for a in db::attempts_for_payment(&mut *tx, id).await? {
            if a.status == "pending" {
                db::set_attempt_status(&mut *tx, a.id, &["pending"], "cancelled").await?;
                cancelled.push(a);
            }
        }
        self.emit(
            &mut tx,
            tenant,
            &preliminary,
            "payment.cancelled",
            json!({}),
        )
        .await?;
        tx.commit().await?;
        self.notify(id);
        for a in cancelled {
            self.cancel_at_provider(&a).await;
        }
        db::get_payment(&self.pool, id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub async fn expire_due(&self) -> Result<usize> {
        let now = Utc::now();
        let mut count = 0;
        for due in db::due_pending_payments(&self.pool, now).await? {
            let pid = due.pid();
            if db::has_unconfirmed_receipts(&self.pool, &pid).await?
                && now < due.expires + Duration::seconds(UNCONFIRMED_GRACE_SECS)
            {
                continue;
            }
            let tenant = self.tenant(due.tenant_id).await?;
            let mut tx = self.pool.begin().await?;
            let Some(p) = db::lock_payment(&mut *tx, &pid).await? else {
                continue;
            };
            if p.status != "pending" || p.expires > now {
                continue;
            }
            db::set_payment_status(&mut *tx, &pid, &["pending"], "expired").await?;
            let mut expired = vec![];
            for a in db::attempts_for_payment(&mut *tx, &pid).await? {
                if a.status == "pending" {
                    db::set_attempt_status(&mut *tx, a.id, &["pending"], "expired").await?;
                    expired.push(a);
                }
            }
            self.emit(&mut tx, &tenant, &p, "payment.expired", json!({}))
                .await?;
            tx.commit().await?;
            self.notify(&pid);
            for a in expired {
                self.cancel_at_provider(&a).await;
            }
            count += 1;
        }
        for a in db::due_pending_attempts(&self.pool, now).await? {
            if db::set_attempt_status(&self.pool, a.id, &["pending"], "expired").await? {
                self.notify(&db::payment_id_of(&a.payment_id));
            }
        }
        Ok(count)
    }
}
