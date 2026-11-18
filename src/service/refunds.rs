use super::*;

impl Service {
    pub async fn refund(
        &self,
        tenant: &Tenant,
        id: &PaymentId,
        input: RefundInput,
    ) -> Result<RefundView> {
        self.payment_for_tenant(tenant, id).await?;
        let mut tx = self.pool.begin().await?;
        let p = db::lock_payment(&mut *tx, id)
            .await?
            .ok_or(Error::NotFound)?;
        if !matches!(p.status.as_str(), "paid" | "partially_refunded") {
            return Err(Error::Conflict(format!("payment is {}", p.status)));
        }
        let attempts = db::attempts_for_payment(&mut *tx, id).await?;
        let attempt = match input.attempt_id {
            Some(aid) => attempts.iter().find(|a| a.id == aid && a.status == "paid"),
            None => attempts
                .iter()
                .filter(|a| a.status == "paid")
                .min_by_key(|a| a.paid_at),
        }
        .ok_or_else(|| Error::Invalid("no paid attempt to refund".into()))?
        .clone();

        let refunds = db::refunds_for_payment(&mut *tx, id).await?;
        let committed: u64 = refunds
            .iter()
            .filter(|r| r.attempt_id == attempt.id && r.status != "failed")
            .map(|r| r.amount)
            .sum();
        let remaining = p.amount.saturating_sub(committed);
        let amount = input.amount.unwrap_or(remaining);
        if amount == 0 || amount > remaining {
            return Err(Error::Invalid(format!("refundable amount is {remaining}")));
        }
        let charge_committed: u64 = refunds
            .iter()
            .filter(|r| r.attempt_id == attempt.id && r.status != "failed")
            .map(|r| r.charge_amount)
            .sum();
        let gross = (attempt.base_amount + attempt.processing_fee).max(1) as u128;
        let mut charge = (attempt.charge_amount as u128 * amount as u128 / gross) as u64;
        if amount == remaining {
            charge = charge
                .max(
                    ((attempt.charge_amount as u128 * attempt.base_amount as u128 / gross) as u64)
                        .saturating_sub(charge_committed),
                )
                .min(attempt.charge_amount.saturating_sub(charge_committed));
        }
        if charge == 0 {
            return Err(Error::Invalid("refund is too small".into()));
        }
        if attempt.charge_currency == "BTC" {
            charge = (charge / 1000) * 1000;
            if charge == 0 {
                return Err(Error::Invalid("refund is below 1 sat".into()));
            }
        }
        let refund_id = db::insert_refund(
            &mut *tx,
            id,
            attempt.id,
            amount,
            charge,
            &attempt.charge_currency,
            input.destination.as_deref(),
        )
        .await;
        let refund_id = match refund_id {
            Ok(r) => r,
            Err(e) => return Err(e.into()),
        };
        tx.commit().await?;

        let provider_id = if attempt.method == "fixedfloat" {
            attempt.watch_provider_id
        } else {
            attempt.provider_config_id
        };
        let outcome = match self.registry.get(provider_id).await {
            Ok(provider) => {
                provider
                    .refund(
                        &attempt,
                        RefundRequest {
                            amount: charge,
                            currency: attempt.charge_currency.clone(),
                            destination: input.destination.clone(),
                        },
                    )
                    .await
            }
            Err(e) => Err(e),
        };
        let mut tx = self.pool.begin().await?;
        let p = db::lock_payment(&mut *tx, id)
            .await?
            .ok_or(Error::NotFound)?;
        let mut failure = None;
        match outcome {
            Ok(done) => {
                db::finish_refund(
                    &mut *tx,
                    refund_id,
                    "completed",
                    done.external_id.as_deref(),
                    None,
                )
                .await?;
                let total: u64 = db::refunds_for_payment(&mut *tx, id)
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
                db::set_payment_status(&mut *tx, id, &["paid", "partially_refunded"], status)
                    .await?;
                self.emit(
                    &mut tx,
                    tenant,
                    &p,
                    "refund.completed",
                    json!({"refund_id": refund_id, "amount": amount}),
                )
                .await?;
            }
            Err(e) => {
                let msg = format!("{e:#}");
                failure = Some(msg.clone());
                db::finish_refund(&mut *tx, refund_id, "failed", None, Some(&msg)).await?;
                self.emit(
                    &mut tx,
                    tenant,
                    &p,
                    "refund.failed",
                    json!({"refund_id": refund_id, "error": msg}),
                )
                .await?;
            }
        }
        tx.commit().await?;
        self.notify(id);
        if let Some(error) = failure {
            self.notify_refund_failed(tenant.id, &error).await;
        }
        let row = db::get_refund(&self.pool, refund_id)
            .await?
            .ok_or(Error::NotFound)?;
        Ok((&row).into())
    }
}
