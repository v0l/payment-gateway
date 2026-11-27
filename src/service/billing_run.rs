use super::*;
use crate::billing::{self, Unit};
use crate::billing_views::SubscriptionView;
use crate::db::{NewPeriod, PeriodRow, PlanRow, SubscriptionRow};

type Tx<'a> = sqlx::Transaction<'a, sqlx::MySql>;

impl Service {
    pub(super) async fn emit_subscription(
        &self,
        tx: &mut Tx<'_>,
        tenant: &Tenant,
        sub: &SubscriptionRow,
        payment: Option<&PaymentRow>,
        event_type: &str,
        extra: Value,
    ) -> Result<()> {
        let now = self.clock.now();
        let payment_view = match payment {
            None => Value::Null,
            Some(p) => {
                let id = p.pid();
                let attempts = db::attempts_for_payment(&mut **tx, &id).await?;
                let refunds = db::refunds_for_payment(&mut **tx, &id).await?;
                let fresh = db::get_payment(&mut **tx, &id)
                    .await?
                    .unwrap_or_else(|| p.clone());
                serde_json::to_value(PaymentView::build(
                    &fresh,
                    &self.public_url,
                    &attempts,
                    &refunds,
                ))
                .map_err(|e| Error::Internal(e.into()))?
            }
        };
        let subscription = SubscriptionView::build(sub, &[], &self.public_url);
        let event_id = random_hex(16);
        let body = json!({
            "id": event_id,
            "type": event_type,
            "created": now.timestamp(),
            "data": {
                "subscription": subscription,
                "payment": payment_view,
                "detail": extra,
            },
        })
        .to_string();
        let pid = payment.map(|p| p.pid());
        let event = super::settlement::OutgoingEvent {
            payment_id: pid.as_ref(),
            event_id: &event_id,
            event_type,
            body: &body,
            now,
        };
        self.fan_out(tx, tenant, &event).await
    }

    pub async fn bill_due(&self) -> Result<usize> {
        let now = self.clock.now();
        let due = db::due_subscriptions(&self.pool, now, 50).await?;
        let mut done = 0;
        let before = self.billing_tally();
        for (tenant_id, id) in due {
            match self.bill_subscription(tenant_id, id).await {
                Ok(()) => done += 1,
                Err(e) => log::warn!("billing subscription {id} failed: {e:?}"),
            }
        }
        self.metrics.inc("gateway_billing_runs_total", &[]);
        let after = self.billing_tally();
        if after != before {
            log::info!(
                "billing run: {} subscriptions due, payments created {}, charged {}, failed {}, cancelled {}",
                done,
                after[0] - before[0],
                after[1] - before[1],
                after[2] - before[2],
                after[3] - before[3],
            );
        }
        Ok(done)
    }

    fn billing_tally(&self) -> [u64; 4] {
        ["created", "charged", "failed", "cancelled"].map(|o| {
            self.metrics
                .counter("gateway_billing_charges_total", &[("outcome", o)])
        })
    }

    fn count_charge(&self, outcome: &str) {
        self.metrics
            .inc("gateway_billing_charges_total", &[("outcome", outcome)]);
    }

    pub async fn bill_subscription(&self, tenant_id: u64, id: u64) -> Result<()> {
        let now = self.clock.now();
        let tenant = self.tenant(tenant_id).await?;
        let mut tx = self.pool.begin().await?;
        let Some(mut sub) = db::lock_subscription(&mut *tx, tenant_id, id).await? else {
            return Ok(());
        };
        let due = matches!(
            sub.status.as_str(),
            billing::STATUS_INCOMPLETE
                | billing::STATUS_TRIALING
                | billing::STATUS_ACTIVE
                | billing::STATUS_PAST_DUE
        ) && sub.next_charge.is_some_and(|n| n <= now)
            && sub.claimed_until.is_none_or(|c| c <= now);
        if !due {
            return Ok(());
        }
        sub.claimed_until = Some(now + Duration::seconds(billing::CLAIM_SECS));
        db::save_subscription(&mut *tx, &sub).await?;
        tx.commit().await?;
        let result = self.bill_claimed(&tenant, sub, now).await;
        if result.is_err() {
            db::release_subscription_claim(&self.pool, id).await?;
        }
        result
    }

    async fn bill_claimed(
        &self,
        tenant: &Tenant,
        sub: SubscriptionRow,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let plan = self.plan_for_tenant(tenant, sub.plan_id).await?;
        let open = db::open_period(&self.pool, sub.id).await?;
        match (sub.status.as_str(), open) {
            (billing::STATUS_INCOMPLETE, open) => {
                self.cancel_claimed(tenant, sub.id, open, "setup_expired", now)
                    .await
            }
            (billing::STATUS_PAST_DUE, open) => {
                self.cancel_claimed(tenant, sub.id, open, "payment_failed", now)
                    .await
            }
            (_, Some(period)) => {
                self.settle_open_period(tenant, sub, plan, period, now)
                    .await
            }
            (_, None) => self.start_period(tenant, sub, plan, now).await,
        }
    }

    async fn cancel_claimed(
        &self,
        tenant: &Tenant,
        id: u64,
        open: Option<PeriodRow>,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let mut to_void = vec![];
        let mut tx = self.pool.begin().await?;
        let mut sub = db::lock_subscription(&mut *tx, tenant.id, id)
            .await?
            .ok_or(Error::NotFound)?;
        let open = match open {
            Some(o) => db::lock_period(&mut *tx, o.id).await?,
            None => None,
        };
        let open = open.filter(|o| o.status == billing::PERIOD_OPEN);
        self.cancel_in_tx(&mut tx, tenant, &mut sub, open, reason, now, &mut to_void)
            .await?;
        tx.commit().await?;
        self.count_charge("cancelled");
        self.void_payments(tenant, to_void).await;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn cancel_in_tx(
        &self,
        tx: &mut Tx<'_>,
        tenant: &Tenant,
        sub: &mut SubscriptionRow,
        open: Option<PeriodRow>,
        reason: &str,
        now: DateTime<Utc>,
        to_void: &mut Vec<PaymentId>,
    ) -> Result<()> {
        let mut payment = None;
        if let Some(mut period) = open {
            period.status = billing::PERIOD_VOID.into();
            if let Some(p) = &period.payment_id {
                let pid = db::payment_id_of(p);
                payment = db::get_payment(&mut **tx, &pid).await?;
                to_void.push(pid);
            }
            db::save_period(&mut **tx, &period).await?;
        }
        sub.status = billing::STATUS_CANCELLED.into();
        sub.cancelled_at = Some(now);
        sub.cancel_reason = Some(reason.to_string());
        sub.cancel_at_period_end = false;
        sub.next_charge = None;
        sub.claimed_until = None;
        db::save_subscription(&mut **tx, sub).await?;
        self.emit_subscription(
            tx,
            tenant,
            sub,
            payment.as_ref(),
            "subscription.cancelled",
            json!({"reason": reason}),
        )
        .await
    }

    pub(super) async fn void_payments(&self, tenant: &Tenant, ids: Vec<PaymentId>) {
        for id in ids {
            if let Err(e) = self.cancel_payment(tenant, &id).await {
                log::debug!("void payment: {e:?}");
            }
        }
    }

    async fn start_period(
        &self,
        tenant: &Tenant,
        sub: SubscriptionRow,
        plan: PlanRow,
        now: DateTime<Utc>,
    ) -> Result<()> {
        if sub.cancel_at_period_end {
            return self
                .cancel_claimed(tenant, sub.id, None, "requested", now)
                .await;
        }
        let unit = Unit::parse(&plan.interval_unit).ok_or(Error::NotFound)?;
        let index = sub.period_index + 1;
        let (start, end) = billing::period_bounds(
            sub.anchor,
            sub.anchor_index,
            unit,
            plan.interval_count,
            index,
        );
        let owed = plan.amount as i64 + sub.adjustment;
        let amount = owed.max(0) as u64;

        let mut tx = self.pool.begin().await?;
        let mut locked = db::lock_subscription(&mut *tx, tenant.id, sub.id)
            .await?
            .ok_or(Error::NotFound)?;
        if locked.period_index != sub.period_index {
            tx.commit().await?;
            return Ok(());
        }
        let free = amount == 0;
        let inserted = db::insert_period(
            &mut *tx,
            &NewPeriod {
                subscription_id: sub.id,
                period_index: index,
                period_start: start,
                period_end: end,
                amount,
                currency: &plan.currency,
                status: if free {
                    billing::PERIOD_PAID
                } else {
                    billing::PERIOD_OPEN
                },
                payment_id: None,
                paid_at: free.then_some(now),
                created: now,
            },
        )
        .await?;
        if !inserted {
            tx.commit().await?;
            return Ok(());
        }
        locked.period_index = index;
        locked.current_period_start = start;
        locked.current_period_end = end;
        locked.adjustment = owed.min(0);
        locked.status = billing::STATUS_ACTIVE.into();
        if free {
            locked.next_charge = Some(end);
            locked.claimed_until = None;
        }
        db::save_subscription(&mut *tx, &locked).await?;
        if free {
            self.emit_subscription(
                &mut tx,
                tenant,
                &locked,
                None,
                "invoice.paid",
                json!({"period": index, "amount": 0, "credit_applied": plan.amount}),
            )
            .await?;
        }
        tx.commit().await?;
        if free {
            return Ok(());
        }
        let period = db::open_period(&self.pool, sub.id)
            .await?
            .ok_or(Error::NotFound)?;
        self.charge_period(tenant, &locked, &plan, period, now)
            .await
    }

    async fn charge_period(
        &self,
        tenant: &Tenant,
        sub: &SubscriptionRow,
        plan: &PlanRow,
        mut period: PeriodRow,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let attempt_no = period.attempts + 1;
        let saved = sub.saved_method_id;
        let payment = self
            .create_period_payment(
                tenant,
                sub.id,
                &sub.customer_ref,
                plan,
                period.period_index,
                period.amount,
                attempt_no,
                saved.is_none(),
            )
            .await?;
        self.count_charge("created");
        let pid = payment.pid();
        period.attempts = attempt_no;
        period.payment_id = Some(pid.to_vec());
        db::save_period(&self.pool, &period).await?;

        if saved.is_none() {
            let deadline = now + Duration::days(plan.grace_days as i64);
            return self
                .finish_unsettled(
                    tenant,
                    sub.id,
                    period.id,
                    deadline,
                    Some((
                        "invoice.payment_required",
                        json!({
                            "period": period.period_index,
                            "pay_url": self.pay_url(&pid),
                            "grace_ends": deadline.timestamp(),
                        }),
                        payment,
                    )),
                )
                .await;
        }
        match self.attempt_saved(tenant, &payment, saved).await {
            Ok(()) => {
                let fresh = db::get_payment(&self.pool, &pid)
                    .await?
                    .ok_or(Error::NotFound)?;
                if fresh.status == "paid" {
                    self.count_charge("charged");
                    self.after_payment_paid(&pid).await?;
                    db::release_subscription_claim(&self.pool, sub.id).await?;
                    Ok(())
                } else {
                    let again = now + Duration::seconds(billing::IN_FLIGHT_RECHECK_SECS);
                    self.finish_unsettled(tenant, sub.id, period.id, again, None)
                        .await
                }
            }
            Err(e) => {
                self.record_failure(tenant, sub.id, &format!("{e:?}"), now)
                    .await
            }
        }
    }

    async fn finish_unsettled(
        &self,
        tenant: &Tenant,
        sub_id: u64,
        period_id: u64,
        next: DateTime<Utc>,
        event: Option<(&str, Value, PaymentRow)>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let mut sub = db::lock_subscription(&mut *tx, tenant.id, sub_id)
            .await?
            .ok_or(Error::NotFound)?;
        let period = db::lock_period(&mut *tx, period_id).await?;
        let still_open = period.is_some_and(|p| p.status == billing::PERIOD_OPEN);
        if still_open && sub.status != billing::STATUS_CANCELLED {
            sub.next_charge = Some(next);
            if let Some((kind, detail, payment)) = &event {
                self.emit_subscription(&mut tx, tenant, &sub, Some(payment), kind, detail.clone())
                    .await?;
            }
        }
        sub.claimed_until = None;
        db::save_subscription(&mut *tx, &sub).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn settle_open_period(
        &self,
        tenant: &Tenant,
        sub: SubscriptionRow,
        plan: PlanRow,
        period: PeriodRow,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let payment = match &period.payment_id {
            Some(p) => db::get_payment(&self.pool, &db::payment_id_of(p)).await?,
            None => None,
        };
        let Some(payment) = payment else {
            return self.charge_period(tenant, &sub, &plan, period, now).await;
        };
        if period.failures >= period.attempts {
            return self.charge_period(tenant, &sub, &plan, period, now).await;
        }
        let pid = payment.pid();
        match payment.status.as_str() {
            "paid" => {
                self.count_charge("charged");
                self.after_payment_paid(&pid).await?;
                db::release_subscription_claim(&self.pool, sub.id).await?;
                Ok(())
            }
            "pending" if sub.saved_method_id.is_some() && payment.expires > now => {
                let again = now + Duration::seconds(billing::IN_FLIGHT_RECHECK_SECS);
                self.finish_unsettled(tenant, sub.id, period.id, again, None)
                    .await
            }
            "pending" => match self.cancel_payment(tenant, &pid).await {
                Ok(_) => {
                    self.record_failure(tenant, sub.id, "grace_expired", now)
                        .await
                }
                Err(Error::Conflict(_)) => {
                    let again = now + Duration::seconds(billing::IN_FLIGHT_RECHECK_SECS);
                    self.finish_unsettled(tenant, sub.id, period.id, again, None)
                        .await
                }
                Err(e) => Err(e),
            },
            other => {
                let reason = format!("payment_{other}");
                self.record_failure(tenant, sub.id, &reason, now).await
            }
        }
    }

    async fn record_failure(
        &self,
        tenant: &Tenant,
        sub_id: u64,
        reason: &str,
        now: DateTime<Utc>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let mut sub = db::lock_subscription(&mut *tx, tenant.id, sub_id)
            .await?
            .ok_or(Error::NotFound)?;
        let Some(mut period) = db::open_period(&mut *tx, sub_id).await? else {
            sub.claimed_until = None;
            db::save_subscription(&mut *tx, &sub).await?;
            tx.commit().await?;
            return Ok(());
        };
        if sub.status == billing::STATUS_CANCELLED {
            tx.commit().await?;
            return Ok(());
        }
        let payment = match &period.payment_id {
            Some(p) => db::get_payment(&mut *tx, &db::payment_id_of(p)).await?,
            None => None,
        };
        period.failures += 1;
        let retry = sub
            .saved_method_id
            .and_then(|_| billing::retry_at(period.period_start, period.failures));
        db::save_period(&mut *tx, &period).await?;
        sub.claimed_until = None;
        match retry {
            Some(at) => {
                let at = at.max(now + Duration::hours(1));
                sub.next_charge = Some(at);
                db::save_subscription(&mut *tx, &sub).await?;
                self.emit_subscription(
                    &mut tx,
                    tenant,
                    &sub,
                    payment.as_ref(),
                    "invoice.failed",
                    json!({
                        "period": period.period_index,
                        "failures": period.failures,
                        "reason": reason,
                        "next_retry": at.timestamp(),
                    }),
                )
                .await?;
            }
            None => {
                sub.status = billing::STATUS_PAST_DUE.into();
                sub.next_charge = Some(now + Duration::days(billing::PAST_DUE_CANCEL_DAYS));
                db::save_subscription(&mut *tx, &sub).await?;
                self.emit_subscription(
                    &mut tx,
                    tenant,
                    &sub,
                    payment.as_ref(),
                    "invoice.failed",
                    json!({
                        "period": period.period_index,
                        "failures": period.failures,
                        "reason": reason,
                        "next_retry": null,
                    }),
                )
                .await?;
                self.emit_subscription(
                    &mut tx,
                    tenant,
                    &sub,
                    payment.as_ref(),
                    "subscription.past_due",
                    json!({"period": period.period_index}),
                )
                .await?;
            }
        }
        tx.commit().await?;
        self.count_charge("failed");
        Ok(())
    }

    pub(super) async fn after_payment_paid(&self, pid: &PaymentId) -> Result<()> {
        let Some(period) = db::period_by_payment(&self.pool, pid).await? else {
            return Ok(());
        };
        let Some((tenant_id, sub_id)) =
            db::subscription_of_period(&self.pool, period.subscription_id).await?
        else {
            return Ok(());
        };
        let tenant = self.tenant(tenant_id).await?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let mut sub = db::lock_subscription(&mut *tx, tenant_id, sub_id)
            .await?
            .ok_or(Error::NotFound)?;
        let Some(mut period) = db::lock_period(&mut *tx, period.id).await? else {
            return Ok(());
        };
        if period.status != billing::PERIOD_OPEN {
            tx.commit().await?;
            return Ok(());
        }
        period.status = billing::PERIOD_PAID.into();
        period.paid_at = Some(now);
        db::save_period(&mut *tx, &period).await?;
        if sub.status != billing::STATUS_CANCELLED {
            if sub.status == billing::STATUS_INCOMPLETE {
                sub.period_index = period.period_index;
                sub.current_period_start = period.period_start;
                sub.current_period_end = period.period_end;
                if let Some(id) =
                    db::saved_method_for_attempt(&mut tx, tenant_id, &sub.customer_ref).await?
                {
                    sub.saved_method_id = Some(id);
                }
            }
            sub.status = billing::STATUS_ACTIVE.into();
            sub.claimed_until = None;
            sub.next_charge = Some(period.period_end.max(now));
            db::save_subscription(&mut *tx, &sub).await?;
        }
        let payment = db::get_payment(&mut *tx, pid).await?;
        self.emit_subscription(
            &mut tx,
            &tenant,
            &sub,
            payment.as_ref(),
            "invoice.paid",
            json!({"period": period.period_index, "amount": period.amount}),
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
}
