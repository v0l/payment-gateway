use super::*;
use crate::billing::{self, Unit};
use crate::billing_views::{
    CancelSubscription, ChangePlan, CreatePlan, CreateSubscription, SubscriptionView,
};
use crate::db::{NewPeriod, NewPlan, NewSubscription, PlanRow, SubscriptionRow};

impl Service {
    pub async fn create_plan(&self, tenant: &Tenant, input: CreatePlan) -> Result<PlanRow> {
        let name = input.name.trim().to_string();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return invalid("name must be 1-128 characters");
        }
        if input
            .description
            .as_ref()
            .is_some_and(|d| d.len() > 255 || d.chars().any(char::is_control))
        {
            return invalid("description must be at most 255 characters");
        }
        if input.amount == 0 || input.amount > rates::MAX_AMOUNT {
            return invalid("amount must be positive");
        }
        let currency = input.currency.trim().to_uppercase();
        if rates::unit(&currency).is_err() {
            return invalid(format!("unsupported currency {currency}"));
        }
        let unit = match input.interval.as_deref() {
            None => Unit::Month,
            Some(i) => Unit::parse(i).ok_or_else(|| {
                Error::Invalid("interval must be day, week, month or year".into())
            })?,
        };
        let count = input.interval_count.unwrap_or(1);
        if !(1..=billing::MAX_INTERVAL_COUNT).contains(&count) {
            return invalid(format!(
                "interval_count must be 1-{}",
                billing::MAX_INTERVAL_COUNT
            ));
        }
        let trial = input.trial_days.unwrap_or(0);
        if trial > billing::MAX_TRIAL_DAYS {
            return invalid(format!(
                "trial_days cannot exceed {}",
                billing::MAX_TRIAL_DAYS
            ));
        }
        let grace = input.grace_days.unwrap_or(billing::DEFAULT_GRACE_DAYS);
        if grace > billing::MAX_GRACE_DAYS {
            return invalid(format!(
                "grace_days cannot exceed {}",
                billing::MAX_GRACE_DAYS
            ));
        }
        let id = db::insert_plan(
            &self.pool,
            &NewPlan {
                tenant_id: tenant.id,
                name: &name,
                description: input.description.as_deref(),
                amount: input.amount,
                currency: &currency,
                interval_unit: unit.as_str(),
                interval_count: count,
                trial_days: trial,
                grace_days: grace,
                created: self.clock.now(),
            },
        )
        .await?;
        self.plan_for_tenant(tenant, id).await
    }

    pub async fn plan_for_tenant(&self, tenant: &Tenant, id: u64) -> Result<PlanRow> {
        db::get_plan(&self.pool, tenant.id, id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub async fn archive_plan(&self, tenant: &Tenant, id: u64) -> Result<PlanRow> {
        if !db::archive_plan(&self.pool, tenant.id, id).await? {
            return Err(Error::NotFound);
        }
        self.plan_for_tenant(tenant, id).await
    }

    pub async fn subscription_row(&self, tenant: &Tenant, id: u64) -> Result<SubscriptionRow> {
        db::get_subscription(&self.pool, tenant.id, id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub async fn subscription_view(
        &self,
        tenant: &Tenant,
        s: &SubscriptionRow,
    ) -> Result<SubscriptionView> {
        let periods = db::list_periods(&self.pool, tenant.id, s.id).await?;
        Ok(SubscriptionView::build(s, &periods, &self.public_url))
    }

    pub async fn create_subscription(
        &self,
        tenant: &Tenant,
        input: CreateSubscription,
    ) -> Result<SubscriptionRow> {
        let customer = input.customer_ref.trim().to_string();
        if customer.is_empty() || customer.len() > 128 || customer.chars().any(char::is_control) {
            return invalid("customer_ref must be 1-128 characters");
        }
        let plan = self.plan_for_tenant(tenant, input.plan_id).await?;
        if !plan.active {
            return invalid("plan is archived");
        }
        let saved = match input.saved_method_id {
            None => None,
            Some(sid) => {
                let saved = db::get_saved_method(&self.pool, tenant.id, sid)
                    .await?
                    .ok_or_else(|| Error::Invalid("unknown saved_method_id".into()))?;
                if saved.customer_ref != customer {
                    return invalid("saved method belongs to a different customer");
                }
                Some(saved)
            }
        };
        let unit = Unit::parse(&plan.interval_unit).ok_or(Error::NotFound)?;
        let now = self.clock.now();
        let trial = plan.trial_days > 0;
        let anchor = if trial {
            now + Duration::days(plan.trial_days as i64)
        } else {
            now
        };
        let (start, end) = billing::period_bounds(anchor, 0, unit, plan.interval_count, 0);
        let (status, current, next_charge) = if trial {
            (billing::STATUS_TRIALING, (now, anchor), anchor)
        } else if saved.is_some() {
            (billing::STATUS_ACTIVE, (start, end), now)
        } else {
            (
                billing::STATUS_INCOMPLETE,
                (start, end),
                now + Duration::seconds(billing::SETUP_EXPIRY_SECS),
            )
        };
        let id = db::insert_subscription(
            &self.pool,
            &NewSubscription {
                tenant_id: tenant.id,
                customer_ref: &customer,
                plan_id: plan.id,
                saved_method_id: saved.as_ref().map(|s| s.id),
                status,
                period_index: -1,
                anchor,
                anchor_index: 0,
                current_period_start: current.0,
                current_period_end: current.1,
                trial_end: trial.then_some(anchor),
                next_charge: Some(next_charge),
                setup_payment_id: None,
                created: now,
            },
        )
        .await?;

        let mut setup_payment = None;
        if status == billing::STATUS_INCOMPLETE {
            let created = self
                .create_period_payment(tenant, id, &customer, &plan, 0, plan.amount, 1, true)
                .await;
            match created {
                Ok(p) => {
                    db::insert_period(
                        &self.pool,
                        &NewPeriod {
                            subscription_id: id,
                            period_index: 0,
                            period_start: start,
                            period_end: end,
                            amount: plan.amount,
                            currency: &plan.currency,
                            status: billing::PERIOD_OPEN,
                            payment_id: Some(&p.pid()),
                            paid_at: None,
                            created: now,
                        },
                    )
                    .await?;
                    db::set_setup_payment(&self.pool, id, &p.pid(), next_charge).await?;
                    setup_payment = Some(p);
                }
                Err(e) => {
                    db::delete_subscription(&self.pool, id).await?;
                    return Err(e);
                }
            }
        }

        let sub = self.subscription_row(tenant, id).await?;
        let mut tx = self.pool.begin().await?;
        self.emit_subscription(
            &mut tx,
            tenant,
            &sub,
            setup_payment.as_ref(),
            "subscription.created",
            json!({}),
        )
        .await?;
        tx.commit().await?;
        if status == billing::STATUS_ACTIVE {
            self.bill_subscription(tenant.id, id).await?;
        }
        self.subscription_row(tenant, id).await
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn create_period_payment(
        &self,
        tenant: &Tenant,
        subscription_id: u64,
        customer: &str,
        plan: &PlanRow,
        period_index: i32,
        amount: u64,
        attempt_no: u32,
        off_session_not_possible: bool,
    ) -> Result<PaymentRow> {
        let expires_in = if off_session_not_possible {
            let days = if plan.grace_days == 0 {
                1
            } else {
                plan.grace_days as i64
            };
            (days * 86_400).max(billing::SETUP_EXPIRY_SECS)
        } else {
            DEFAULT_EXPIRY_SECS
        };
        let key = format!("sub-{subscription_id}-{period_index}-{attempt_no}");
        let (payment, _) = self
            .create_payment(
                tenant,
                CreatePayment {
                    reference: Some(format!("sub_{subscription_id}_{period_index}")),
                    amount,
                    currency: plan.currency.clone(),
                    tax: 0,
                    expires_in: Some(expires_in),
                    description: Some(plan.name.clone()),
                    customer_ref: Some(customer.to_string()),
                    customer_email: None,
                    metadata: Some(json!({
                        "subscription_id": subscription_id,
                        "period": period_index,
                    })),
                    methods: None,
                    success_url: None,
                    cancel_url: None,
                },
                Some(&key),
            )
            .await?;
        Ok(payment)
    }

    pub async fn pause_subscription(&self, tenant: &Tenant, id: u64) -> Result<SubscriptionRow> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let mut sub = self.lock_sub(&mut tx, tenant, id).await?;
        if !matches!(
            sub.status.as_str(),
            billing::STATUS_ACTIVE | billing::STATUS_TRIALING
        ) {
            return Err(Error::Conflict(format!("subscription is {}", sub.status)));
        }
        sub.status = billing::STATUS_PAUSED.into();
        sub.paused_at = Some(now);
        sub.next_charge = None;
        db::save_subscription(&mut *tx, &sub).await?;
        self.emit_subscription(
            &mut tx,
            tenant,
            &sub,
            None,
            "subscription.paused",
            json!({}),
        )
        .await?;
        tx.commit().await?;
        Ok(sub)
    }

    pub async fn resume_subscription(&self, tenant: &Tenant, id: u64) -> Result<SubscriptionRow> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let mut sub = self.lock_sub(&mut tx, tenant, id).await?;
        if sub.status != billing::STATUS_PAUSED {
            return Err(Error::Conflict(format!("subscription is {}", sub.status)));
        }
        let trial_left = sub.trial_end.is_some_and(|t| t > now) && sub.period_index < 0;
        sub.status = if trial_left {
            billing::STATUS_TRIALING
        } else {
            billing::STATUS_ACTIVE
        }
        .into();
        sub.paused_at = None;
        if sub.current_period_end > now {
            sub.next_charge = Some(sub.current_period_end);
        } else {
            sub.anchor = now;
            sub.anchor_index = sub.period_index + 1;
            sub.next_charge = Some(now);
        }
        db::save_subscription(&mut *tx, &sub).await?;
        self.emit_subscription(
            &mut tx,
            tenant,
            &sub,
            None,
            "subscription.resumed",
            json!({}),
        )
        .await?;
        tx.commit().await?;
        Ok(sub)
    }

    pub async fn cancel_subscription(
        &self,
        tenant: &Tenant,
        id: u64,
        input: CancelSubscription,
    ) -> Result<SubscriptionRow> {
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let mut sub = self.lock_sub(&mut tx, tenant, id).await?;
        if sub.status == billing::STATUS_CANCELLED {
            return Err(Error::Conflict("subscription is cancelled".into()));
        }
        let mut to_void = vec![];
        if input.at_period_end
            && matches!(
                sub.status.as_str(),
                billing::STATUS_ACTIVE | billing::STATUS_TRIALING | billing::STATUS_PAUSED
            )
        {
            sub.cancel_at_period_end = true;
            if sub.status == billing::STATUS_PAUSED {
                sub.next_charge = None;
            }
            db::save_subscription(&mut *tx, &sub).await?;
        } else {
            let open = db::open_period(&mut *tx, sub.id).await?;
            self.cancel_in_tx(
                &mut tx,
                tenant,
                &mut sub,
                open,
                "requested",
                now,
                &mut to_void,
            )
            .await?;
        }
        tx.commit().await?;
        self.void_payments(tenant, to_void).await;
        Ok(sub)
    }

    pub async fn change_plan(
        &self,
        tenant: &Tenant,
        id: u64,
        input: ChangePlan,
    ) -> Result<SubscriptionRow> {
        let prorate = match input.proration.as_deref() {
            None | Some("none") => false,
            Some("prorate") => true,
            Some(_) => return invalid("proration must be none or prorate"),
        };
        let new = self.plan_for_tenant(tenant, input.plan_id).await?;
        if !new.active {
            return invalid("plan is archived");
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let mut sub = self.lock_sub(&mut tx, tenant, id).await?;
        if !matches!(
            sub.status.as_str(),
            billing::STATUS_ACTIVE | billing::STATUS_TRIALING
        ) {
            return Err(Error::Conflict(format!("subscription is {}", sub.status)));
        }
        let old = self.plan_for_tenant(tenant, sub.plan_id).await?;
        if old.currency != new.currency {
            return invalid("the new plan must use the same currency");
        }
        let paid_period = sub.period_index >= 0 && sub.status == billing::STATUS_ACTIVE;
        let diff = if prorate && paid_period {
            billing::prorated_difference(
                old.amount,
                new.amount,
                sub.current_period_start,
                sub.current_period_end,
                now,
            )
        } else {
            0
        };
        sub.plan_id = new.id;
        sub.anchor = sub.current_period_end;
        sub.anchor_index = sub.period_index + 1;
        let mut charge_now = 0;
        if diff > 0 {
            charge_now = diff as u64;
        } else {
            sub.adjustment += diff;
        }
        db::save_subscription(&mut *tx, &sub).await?;
        self.emit_subscription(
            &mut tx,
            tenant,
            &sub,
            None,
            "subscription.updated",
            json!({"previous_plan_id": old.id, "proration": diff}),
        )
        .await?;
        tx.commit().await?;
        if charge_now > 0 {
            self.charge_proration(tenant, &sub, &new, charge_now, now)
                .await;
        }
        self.subscription_row(tenant, id).await
    }

    async fn charge_proration(
        &self,
        tenant: &Tenant,
        sub: &SubscriptionRow,
        plan: &PlanRow,
        amount: u64,
        now: DateTime<Utc>,
    ) {
        let key = format!(
            "sub-{}-{}-change-{}-{}",
            sub.id,
            sub.period_index,
            plan.id,
            now.timestamp()
        );
        let result = async {
            let payment = self
                .create_period_payment_keyed(tenant, sub, plan, amount, &key)
                .await?;
            if sub.saved_method_id.is_some() {
                self.attempt_saved(tenant, &payment, sub.saved_method_id)
                    .await?;
            }
            Result::Ok(())
        }
        .await;
        if let Err(e) = result {
            log::warn!("proration charge for subscription {} failed: {e:?}", sub.id);
            let _ = db::add_adjustment(&self.pool, sub.tenant_id, sub.id, amount as i64).await;
        }
    }

    async fn create_period_payment_keyed(
        &self,
        tenant: &Tenant,
        sub: &SubscriptionRow,
        plan: &PlanRow,
        amount: u64,
        key: &str,
    ) -> Result<PaymentRow> {
        let (payment, _) = self
            .create_payment(
                tenant,
                CreatePayment {
                    reference: Some(format!("sub_{}_{}_change", sub.id, sub.period_index)),
                    amount,
                    currency: plan.currency.clone(),
                    tax: 0,
                    expires_in: Some(if sub.saved_method_id.is_some() {
                        DEFAULT_EXPIRY_SECS
                    } else {
                        billing::SETUP_EXPIRY_SECS
                    }),
                    description: Some(format!("{} (plan change)", plan.name)),
                    customer_ref: Some(sub.customer_ref.clone()),
                    customer_email: None,
                    metadata: Some(json!({"subscription_id": sub.id})),
                    methods: None,
                    success_url: None,
                    cancel_url: None,
                },
                Some(key),
            )
            .await?;
        Ok(payment)
    }

    pub(super) async fn attempt_saved(
        &self,
        tenant: &Tenant,
        payment: &PaymentRow,
        saved_id: Option<u64>,
    ) -> Result<()> {
        let saved = db::get_saved_method(&self.pool, tenant.id, saved_id.ok_or(Error::NotFound)?)
            .await?
            .ok_or_else(|| Error::Invalid("saved method is gone".into()))?;
        let cfg = db::get_provider_config(&self.pool, saved.provider_config_id)
            .await?
            .filter(|c| c.enabled && c.tenant_id == tenant.id)
            .ok_or_else(|| Error::Invalid("the provider is disabled".into()))?;
        let pid = payment.pid();
        let result = self
            .create_attempt(
                tenant,
                &pid,
                CreateAttempt {
                    method: cfg.method.clone(),
                    provider_id: Some(cfg.id),
                    saved_method_id: Some(saved.id),
                    ..Default::default()
                },
            )
            .await;
        if result.is_err() {
            db::set_payment_status(&self.pool, &pid, &["pending"], "cancelled").await?;
        }
        result.map(|_| ())
    }

    async fn lock_sub(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
        tenant: &Tenant,
        id: u64,
    ) -> Result<SubscriptionRow> {
        db::lock_subscription(&mut **tx, tenant.id, id)
            .await?
            .ok_or(Error::NotFound)
    }
}
