use super::*;
use crate::audit::{Action, Actor, Event, Origin};
use crate::billing::{self, Unit};
use crate::db::{NewPlatformInvoice, PlatformAccountRow, PlatformInvoiceRow, PlatformPlanRow};
use crate::platform::{self, InvoiceLine, Metered, Standing};
use crate::platform_views::PlatformInvoiceView;

type Tx<'a> = sqlx::Transaction<'a, sqlx::MySql>;

const MAX_CATCH_UP_PERIODS: usize = 24;

fn period_after(start: DateTime<Utc>) -> DateTime<Utc> {
    billing::add_intervals(start, Unit::Month, 1, 1)
}

impl Service {
    pub async fn platform_plan(&self, id: u64) -> Result<PlatformPlanRow> {
        db::get_platform_plan(&self.pool, id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub async fn ensure_platform_account(&self, tenant: &Tenant) -> Result<PlatformAccountRow> {
        if let Some(a) = db::get_platform_account(&self.pool, tenant.id).await? {
            return Ok(a);
        }
        let plan = db::default_platform_plan(&self.pool, tenant.trusted)
            .await?
            .ok_or_else(|| Error::Conflict("no platform plan is configured".into()))?;
        let now = self.clock.now();
        let account = PlatformAccountRow {
            tenant_id: tenant.id,
            plan_id: plan.id,
            status: platform::STANDING_CURRENT.into(),
            status_since: None,
            period_start: now,
            period_end: period_after(now),
            period_fee: plan.monthly_price,
            credit: 0,
            created: now,
        };
        let mut tx = self.pool.begin().await?;
        if db::insert_platform_account(&mut *tx, &account).await? && plan.monthly_price > 0 {
            let lines = vec![InvoiceLine::new(
                format!("{} plan, monthly", plan.name),
                1,
                plan.monthly_price,
            )];
            let key = format!("plan-{}-{}", tenant.id, now.timestamp());
            self.insert_invoice(
                &mut tx,
                tenant,
                &plan,
                platform::KIND_PLAN,
                &key,
                (account.period_start, account.period_end),
                Metered::default(),
                lines,
                0,
            )
            .await?;
        }
        tx.commit().await?;
        db::get_platform_account(&self.pool, tenant.id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub async fn metered_usage(
        &self,
        tenant_id: u64,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        currency: &str,
    ) -> Result<Metered> {
        let mut used = Metered::default();
        for (cur, volume, count) in db::metered_paid(&self.pool, tenant_id, start, end).await? {
            let converted = rates::convert(self.rates.as_ref(), volume, &cur, currency)
                .await
                .map_err(|e| Error::Upstream(format!("rate lookup failed: {e}")))?;
            used.count += count;
            used.volume = used.volume.saturating_add(converted);
        }
        Ok(used)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn insert_invoice(
        &self,
        tx: &mut Tx<'_>,
        tenant: &Tenant,
        plan: &PlatformPlanRow,
        kind: &str,
        key: &str,
        period: (DateTime<Utc>, DateTime<Utc>),
        used: Metered,
        lines: Vec<InvoiceLine>,
        credit_available: u64,
    ) -> Result<Option<(u64, u64)>> {
        let subtotal = platform::subtotal(&lines);
        if subtotal == 0 {
            return Ok(None);
        }
        let now = self.clock.now();
        let credit_applied = credit_available.min(subtotal);
        let total = subtotal - credit_applied;
        let status = if total == 0 {
            platform::INVOICE_PAID
        } else {
            platform::INVOICE_OPEN
        };
        let lines_json = serde_json::to_string(&lines).map_err(|e| Error::Internal(e.into()))?;
        let id = db::insert_platform_invoice(
            &mut **tx,
            &NewPlatformInvoice {
                tenant_id: tenant.id,
                uniq_key: key,
                kind,
                plan_id: plan.id,
                period_start: period.0,
                period_end: period.1,
                currency: &plan.currency,
                usage_count: used.count,
                usage_volume: used.volume,
                lines: &lines_json,
                subtotal,
                credit_applied,
                total,
                status,
                due_at: now + Duration::days(platform::INVOICE_DUE_DAYS),
                paid_at: (total == 0).then_some(now),
                created: now,
            },
        )
        .await?;
        let Some(id) = id else {
            return Ok(None);
        };
        self.emit_platform_invoice(tx, tenant, id, "platform.invoice.created")
            .await?;
        if total == 0 {
            self.emit_platform_invoice(tx, tenant, id, "platform.invoice.paid")
                .await?;
        }
        Ok(Some((id, credit_applied)))
    }

    pub async fn invoice_view(&self, row: &PlatformInvoiceRow) -> Result<PlatformInvoiceView> {
        let method = match &row.payment_id {
            Some(p) => {
                let attempts = db::attempts_for_payment(&self.pool, &db::payment_id_of(p)).await?;
                attempts
                    .iter()
                    .find(|a| a.status == "paid")
                    .map(|a| a.method.clone())
            }
            None => None,
        };
        Ok(PlatformInvoiceView::build(
            row,
            &self.public_url,
            method,
            self.clock.now(),
        ))
    }

    pub(super) async fn emit_platform_invoice(
        &self,
        tx: &mut Tx<'_>,
        tenant: &Tenant,
        invoice_id: u64,
        event_type: &str,
    ) -> Result<()> {
        let row = db::get_platform_invoice(&mut **tx, tenant.id, invoice_id)
            .await?
            .ok_or(Error::NotFound)?;
        let now = self.clock.now();
        let view = PlatformInvoiceView::build(&row, &self.public_url, None, now);
        let event_id = random_hex(16);
        let body = json!({
            "id": event_id,
            "type": event_type,
            "created": now.timestamp(),
            "data": {"invoice": view},
        })
        .to_string();
        let event = super::settlement::OutgoingEvent {
            payment_id: None,
            event_id: &event_id,
            event_type,
            body: &body,
            now,
        };
        self.fan_out(tx, tenant, &event).await
    }

    pub async fn close_platform_period(&self, tenant_id: u64) -> Result<bool> {
        let tenant = self.tenant(tenant_id).await?;
        let Some(account) = db::get_platform_account(&self.pool, tenant_id).await? else {
            return Ok(false);
        };
        let now = self.clock.now();
        if account.period_end > now {
            return Ok(false);
        }
        let plan = self.platform_plan(account.plan_id).await?;
        let used = self
            .metered_usage(
                tenant_id,
                account.period_start,
                account.period_end,
                &plan.currency,
            )
            .await?;
        let mut tx = self.pool.begin().await?;
        let Some(mut locked) = db::lock_platform_account(&mut *tx, tenant_id).await? else {
            return Ok(false);
        };
        if locked.period_start != account.period_start || locked.plan_id != account.plan_id {
            return Ok(false);
        }
        let mut lines = platform::usage_lines(&plan.fees(), used);
        if plan.monthly_price > 0 {
            lines.push(InvoiceLine::new(
                format!("{} plan, next month", plan.name),
                1,
                plan.monthly_price,
            ));
        }
        let key = format!("period-{}-{}", tenant_id, locked.period_start.timestamp());
        let applied = self
            .insert_invoice(
                &mut tx,
                &tenant,
                &plan,
                platform::KIND_PERIOD,
                &key,
                (locked.period_start, locked.period_end),
                used,
                lines,
                locked.credit,
            )
            .await?
            .map(|(_, applied)| applied)
            .unwrap_or(0);
        locked.credit -= applied;
        locked.period_start = account.period_end;
        locked.period_end = period_after(account.period_end);
        locked.period_fee = plan.monthly_price;
        db::save_platform_account(&mut *tx, &locked).await?;
        tx.commit().await?;
        Ok(true)
    }

    pub async fn ensure_invoice_payment(&self, inv: &PlatformInvoiceRow) -> Result<()> {
        if inv.status != platform::INVOICE_OPEN {
            return Ok(());
        }
        if let Some(p) = &inv.payment_id {
            let id = db::payment_id_of(p);
            match db::get_payment(&self.pool, &id).await? {
                Some(pay) if pay.status == "pending" && pay.expires > self.clock.now() => {
                    return Ok(());
                }
                Some(pay) if pay.status == "paid" => return self.platform_payment_paid(&id).await,
                _ => {}
            }
        }
        let Some(platform_tenant_id) = db::platform_tenant(&self.pool).await? else {
            return Ok(());
        };
        let platform_tenant = self.tenant(platform_tenant_id).await?;
        let key = format!("pinv-{}-{}", inv.id, inv.pay_attempts);
        let (payment, _) = self
            .create_payment(
                &platform_tenant,
                CreatePayment {
                    reference: Some(format!("pinv_{}_{}", inv.id, inv.pay_attempts)),
                    amount: inv.total,
                    currency: inv.currency.clone(),
                    tax: 0,
                    expires_in: Some(platform::PAYMENT_EXPIRY_DAYS * 86_400),
                    description: Some(format!("Platform invoice {}", inv.id)),
                    customer_ref: Some(format!("project_{}", inv.tenant_id)),
                    customer_email: None,
                    metadata: Some(json!({
                        "platform_invoice": inv.id,
                        "project": inv.tenant_id,
                    })),
                    methods: None,
                    success_url: None,
                    cancel_url: None,
                    line_items: None,
                    discount: None,
                },
                Some(&key),
            )
            .await?;
        let _ =
            db::set_platform_invoice_payment(&self.pool, inv.id, &payment.pid(), inv.pay_attempts)
                .await?;
        Ok(())
    }

    pub(super) async fn platform_payment_paid(&self, pid: &PaymentId) -> Result<()> {
        let Some(inv) = db::platform_invoice_by_payment(&self.pool, pid).await? else {
            return Ok(());
        };
        let tenant = self.tenant(inv.tenant_id).await?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let Some(locked) = db::lock_platform_invoice(&mut *tx, inv.id).await? else {
            return Ok(());
        };
        if locked.status != platform::INVOICE_OPEN {
            tx.commit().await?;
            return Ok(());
        }
        db::settle_platform_invoice(&mut *tx, inv.id, platform::INVOICE_PAID, None, now).await?;
        self.emit_platform_invoice(&mut tx, &tenant, inv.id, "platform.invoice.paid")
            .await?;
        tx.commit().await?;
        self.review_platform_standing(inv.tenant_id).await
    }

    pub async fn review_platform_standing(&self, tenant_id: u64) -> Result<()> {
        let tenant = self.tenant(tenant_id).await?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let Some(mut account) = db::lock_platform_account(&mut *tx, tenant_id).await? else {
            return Ok(());
        };
        let open = db::open_platform_invoices(&mut *tx, tenant_id).await?;
        for inv in open
            .iter()
            .filter(|i| i.due_at <= now && !i.past_due_notified)
        {
            if db::mark_invoice_past_due_notified(&mut *tx, inv.id).await? {
                self.emit_platform_invoice(&mut tx, &tenant, inv.id, "platform.invoice.past_due")
                    .await?;
            }
        }
        let before = Standing::parse(&account.status);
        let after = platform::standing_for(open.first().map(|i| i.due_at), now);
        if before == after {
            tx.commit().await?;
            return Ok(());
        }
        account.status = after.as_str().into();
        account.status_since = (after != Standing::Current).then_some(now);
        db::save_platform_account(&mut *tx, &account).await?;
        tx.commit().await?;
        Event::new(Action::PlatformStandingChanged, Actor::System)
            .tenant(tenant_id)
            .target("project", tenant_id)
            .details(json!({"from": before.as_str(), "to": after.as_str()}))
            .record(&self.pool, &Origin::default())
            .await;
        self.mail_standing(tenant_id, after).await;
        Ok(())
    }

    async fn mail_standing(&self, tenant_id: u64, standing: Standing) {
        let (subject, detail) = match standing {
            Standing::Current => (
                "Billing is up to date",
                "Your platform invoices are paid. Normal service is restored.".to_string(),
            ),
            Standing::Warned => (
                "Platform invoice past due",
                format!(
                    "A platform invoice is past due. Pay it within {} days to avoid restrictions.",
                    platform::RESTRICT_AFTER_DAYS
                ),
            ),
            Standing::Restricted => (
                "Project restricted for unpaid invoice",
                "Your project can only process refunds and withdrawals until the overdue invoice is paid. Withdrawing your own funds is never blocked."
                    .to_string(),
            ),
            Standing::Disabled => (
                "New payments disabled for unpaid invoice",
                "New payments and attempts are disabled until the overdue invoice is paid. Refunds and withdrawals still work."
                    .to_string(),
            ),
        };
        let kind = format!("platform_{}", standing.as_str());
        self.notify_quietly(tenant_id, &kind, subject, &detail)
            .await;
    }

    pub async fn run_platform_billing(&self) -> Result<()> {
        let platform_tenant = db::platform_tenant(&self.pool).await?;
        for id in db::tenants_without_platform_account(&self.pool).await? {
            if Some(id) == platform_tenant {
                continue;
            }
            let tenant = self.tenant(id).await?;
            self.ensure_platform_account(&tenant).await?;
        }
        let now = self.clock.now();
        for id in db::platform_accounts_due(&self.pool, now).await? {
            for _ in 0..MAX_CATCH_UP_PERIODS {
                if !self.close_platform_period(id).await? {
                    break;
                }
            }
        }
        for inv in db::all_open_platform_invoices(&self.pool).await? {
            if let Err(e) = self.ensure_invoice_payment(&inv).await {
                log::warn!("platform invoice {} payment failed: {e:?}", inv.id);
            }
        }
        for id in db::tenants_to_review(&self.pool).await? {
            self.review_platform_standing(id).await?;
        }
        Ok(())
    }

    pub async fn platform_standing(&self, tenant_id: u64) -> Standing {
        db::platform_standing(&self.pool, tenant_id)
            .await
            .ok()
            .flatten()
            .map(|s| Standing::parse(&s))
            .unwrap_or(Standing::Current)
    }
}
