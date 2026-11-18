use super::*;

pub(super) struct OutgoingEvent<'a> {
    pub payment_id: Option<&'a PaymentId>,
    pub event_id: &'a str,
    pub event_type: &'a str,
    pub body: &'a str,
    pub now: DateTime<Utc>,
}

impl Service {
    pub(super) async fn emit(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
        tenant: &Tenant,
        p: &PaymentRow,
        event_type: &str,
        extra: Value,
    ) -> Result<()> {
        let id = p.pid();
        let attempts = db::attempts_for_payment(&mut **tx, &id).await?;
        let refunds = db::refunds_for_payment(&mut **tx, &id).await?;
        let fresh = db::get_payment(&mut **tx, &id)
            .await?
            .unwrap_or_else(|| p.clone());
        let view = PaymentView::build(&fresh, &self.public_url, &attempts, &refunds);
        let now = self.clock.now();
        let event_id = random_hex(16);
        let body = json!({
            "id": event_id,
            "type": event_type,
            "created": now.timestamp(),
            "data": { "payment": view, "detail": extra },
        });
        let body = body.to_string();
        let event = OutgoingEvent {
            payment_id: Some(&id),
            event_id: &event_id,
            event_type,
            body: &body,
            now,
        };
        self.fan_out(tx, tenant, &event).await
    }

    pub(super) async fn fan_out(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
        tenant: &Tenant,
        e: &OutgoingEvent<'_>,
    ) -> Result<()> {
        db::insert_event(
            &mut **tx,
            tenant.id,
            e.payment_id,
            e.event_id,
            e.event_type,
            e.body,
            e.now,
        )
        .await?;
        for endpoint in db::list_endpoints(&mut **tx, tenant.id).await? {
            if endpoint.enabled && endpoint_wants(&endpoint, e.event_type) {
                db::insert_delivery(
                    &mut **tx,
                    tenant.id,
                    e.payment_id,
                    e.event_id,
                    e.event_type,
                    e.body,
                    Some(endpoint.id),
                    e.now,
                )
                .await?;
            }
        }
        Ok(())
    }

    pub async fn apply_event(&self, provider_id: u64, event: ProviderEvent) -> Result<()> {
        match event {
            ProviderEvent::Paid {
                lookup,
                reference,
                amount,
                confirmed,
                saved,
            } => {
                let Some(attempt) =
                    db::find_attempt_by_lookup(&self.pool, provider_id, &lookup).await?
                else {
                    log::debug!(
                        "provider {provider_id}: payment for unknown lookup {}",
                        crate::logsafe::clean(&lookup)
                    );
                    return Ok(());
                };
                if let Some(s) = saved {
                    let payment =
                        db::get_payment(&self.pool, &db::payment_id_of(&attempt.payment_id))
                            .await?
                            .ok_or(Error::NotFound)?;
                    if let Some(customer) = &payment.customer_ref {
                        db::upsert_saved_method(
                            &self.pool,
                            &NewSavedMethod {
                                tenant_id: payment.tenant_id,
                                customer_ref: customer,
                                provider_config_id: attempt.provider_config_id,
                                external_customer_id: &s.customer_id,
                                external_id: &s.method_id,
                                brand: s.brand.as_deref(),
                                last4: s.last4.as_deref(),
                                exp_month: s.exp_month,
                                exp_year: s.exp_year,
                            },
                        )
                        .await?;
                    }
                }
                self.settle_attempt(attempt.id, &reference, amount, confirmed)
                    .await
            }
            ProviderEvent::Failed { lookup, reason } => {
                let Some(attempt) =
                    db::find_attempt_by_lookup(&self.pool, provider_id, &lookup).await?
                else {
                    return Ok(());
                };
                let pid = db::payment_id_of(&attempt.payment_id);
                let mut tx = self.pool.begin().await?;
                let Some(p) = db::lock_payment(&mut *tx, &pid).await? else {
                    return Ok(());
                };
                if db::set_attempt_status(&mut *tx, attempt.id, &["pending"], "failed").await? {
                    let tenant = self.tenant(p.tenant_id).await?;
                    self.emit(
                        &mut tx,
                        &tenant,
                        &p,
                        "attempt.failed",
                        json!({"attempt_id": attempt.id, "reason": reason}),
                    )
                    .await?;
                }
                tx.commit().await?;
                self.notify(&pid);
                Ok(())
            }
            ProviderEvent::Refunded {
                lookup,
                external_id,
                amount,
            } => {
                self.apply_provider_refund(provider_id, &lookup, &external_id, amount)
                    .await
            }
            ProviderEvent::RefundFailed {
                lookup,
                external_id,
                reason,
            } => {
                self.apply_provider_refund_failure(provider_id, &lookup, &external_id, &reason)
                    .await
            }
            ProviderEvent::Dispute {
                lookup,
                state,
                reason,
            } => {
                self.apply_dispute(provider_id, &lookup, state, reason.as_deref())
                    .await
            }
        }
    }

    pub async fn settle_attempt(
        &self,
        attempt_id: u64,
        reference: &str,
        amount: Option<u64>,
        confirmed: bool,
    ) -> Result<()> {
        super::retry_deadlock(|| self.settle_attempt_once(attempt_id, reference, amount, confirmed))
            .await
    }

    async fn settle_attempt_once(
        &self,
        attempt_id: u64,
        reference: &str,
        amount: Option<u64>,
        confirmed: bool,
    ) -> Result<()> {
        let settle_started = std::time::Instant::now();
        let probe = db::get_attempt(&self.pool, attempt_id)
            .await?
            .ok_or(Error::NotFound)?;
        let pid = db::payment_id_of(&probe.payment_id);
        let preliminary = db::get_payment(&self.pool, &pid)
            .await?
            .ok_or(Error::NotFound)?;
        let tenant = self.tenant(preliminary.tenant_id).await?;

        let mut tx = self.pool.begin().await?;
        let p = db::lock_payment(&mut *tx, &pid)
            .await?
            .ok_or(Error::NotFound)?;
        let attempt = db::lock_attempt(&mut *tx, attempt_id)
            .await?
            .ok_or(Error::NotFound)?;
        if attempt.status == "paid" {
            tx.commit().await?;
            return Ok(());
        }
        let required = required_receipt(&attempt);
        db::upsert_receipt(
            &mut *tx,
            attempt.id,
            reference,
            amount.unwrap_or(required),
            confirmed,
            self.clock.now(),
        )
        .await?;
        let received: u64 = db::receipts_for_attempt(&mut *tx, attempt.id)
            .await?
            .iter()
            .filter(|r| r.confirmed)
            .map(|r| r.amount)
            .sum();
        if received < required {
            tx.commit().await?;
            self.notify(&pid);
            return Ok(());
        }

        let now = self.clock.now();
        db::mark_attempt_paid(&mut *tx, attempt.id, now).await?;
        let split = self
            .post_settlement(&mut tx, tenant.id, &attempt, received)
            .await?;
        let detail = match split {
            Some(sp) => {
                json!({"attempt_id": attempt.id, "received": received, "platform_fee": sp.fee, "net_credited": sp.net})
            }
            None => json!({"attempt_id": attempt.id, "received": received}),
        };
        let mut to_cancel = vec![];
        match p.status.as_str() {
            "pending" | "expired" => {
                db::mark_payment_paid(&mut *tx, &pid, now).await?;
                for other in db::attempts_for_payment(&mut *tx, &pid).await? {
                    if other.id != attempt.id && other.status == "pending" {
                        db::set_attempt_status(&mut *tx, other.id, &["pending"], "cancelled")
                            .await?;
                        to_cancel.push(other);
                    }
                }
                self.emit(&mut tx, &tenant, &p, "payment.paid", detail)
                    .await?;
            }
            _ => {
                self.emit(&mut tx, &tenant, &p, "payment.late_payment", detail)
                    .await?;
            }
        }
        tx.commit().await?;
        self.metrics.inc("gateway_settlements_total", &[]);
        self.metrics.observe(
            "gateway_settlement_duration_seconds",
            &[],
            settle_started.elapsed().as_secs_f64(),
        );
        self.notify(&pid);
        for other in to_cancel {
            self.cancel_at_provider(&other).await;
        }
        if let Err(e) = self.after_payment_paid(&pid).await {
            log::warn!("subscription update after payment failed: {e:?}");
        }
        Ok(())
    }

    pub(super) async fn cancel_at_provider(&self, a: &AttemptRow) {
        if matches!(a.method.as_str(), "onchain" | "fixedfloat") {
            return;
        }
        match self.registry.get(a.provider_config_id).await {
            Ok(p) => {
                if let Err(e) = p.cancel(a).await {
                    log::warn!("cancel attempt {} at provider failed: {e:#}", a.id);
                }
            }
            Err(e) => log::warn!("cancel attempt {}: provider unavailable: {e:#}", a.id),
        }
    }

    pub async fn reconcile_provider(&self, provider_id: u64) -> Result<usize> {
        let provider = self
            .registry
            .get(provider_id)
            .await
            .map_err(|e| Error::Upstream(format!("{e:#}")))?;
        let mut settled = 0;
        for attempt in db::pending_attempts_of_provider(&self.pool, provider_id).await? {
            match provider.reconcile(&attempt).await {
                Ok(Some(event)) => {
                    self.apply_event(provider_id, event).await?;
                    settled += 1;
                }
                Ok(None) => {}
                Err(e) => log::warn!("reconcile attempt {} failed: {e:#}", attempt.id),
            }
        }
        Ok(settled)
    }
}
