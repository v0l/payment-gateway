use super::*;

impl Service {
    pub async fn mark_paid_manually(
        &self,
        tenant: &Tenant,
        id: &PaymentId,
        reason: &str,
    ) -> Result<PaymentRow> {
        let preliminary = self.payment_for_tenant(tenant, id).await?;
        let mut tx = self.pool.begin().await?;
        let p = db::lock_payment(&mut *tx, id)
            .await?
            .ok_or(Error::NotFound)?;
        if p.status != "pending" && p.status != "expired" {
            return Err(Error::Conflict(format!("payment is {}", p.status)));
        }
        db::mark_payment_paid(&mut *tx, id, Utc::now()).await?;
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
            "payment.paid",
            json!({"manual": true, "reason": reason}),
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
}
