use super::*;
use crate::db::{PlatformInvoiceRow, PlatformPlanInput, PlatformPlanRow};
use crate::limits::LimitOverrides;
use crate::platform::{self, InvoiceLine, Metered};

pub struct PlanDefinition {
    pub name: String,
    pub live: bool,
    pub monthly_price: u64,
    pub currency: String,
    pub included_volume: u64,
    pub fee_bps: u32,
    pub fee_base: u64,
    pub limits: LimitOverrides,
    pub active: bool,
}

const MAX_FEE_BPS: u32 = 10_000;

fn clean_reason(reason: &str) -> Result<String> {
    let reason = reason.trim();
    if reason.is_empty() || reason.len() > 255 || reason.chars().any(char::is_control) {
        return invalid("a reason of 1-255 characters is required");
    }
    Ok(reason.to_string())
}

impl Service {
    pub async fn check_standing(&self, tenant_id: u64) -> Result<()> {
        if self
            .platform_standing(tenant_id)
            .await
            .blocks_new_payments()
        {
            return Err(Error::Conflict(
                "project billing is past due: new payments are paused until the platform invoice is paid"
                    .into(),
            ));
        }
        Ok(())
    }

    pub(super) async fn check_attempt_standing(&self, tenant_id: u64) -> Result<()> {
        if self
            .platform_standing(tenant_id)
            .await
            .blocks_new_attempts()
        {
            return Err(Error::Conflict(
                "project billing is past due: new payments are disabled until the platform invoice is paid"
                    .into(),
            ));
        }
        Ok(())
    }

    pub async fn define_platform_plan(
        &self,
        id: Option<u64>,
        def: PlanDefinition,
    ) -> Result<PlatformPlanRow> {
        let name = def.name.trim().to_string();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return invalid("name must be 1-128 characters");
        }
        let currency = def.currency.trim().to_uppercase();
        if rates::unit(&currency).is_err() {
            return invalid(format!("unsupported currency {currency}"));
        }
        if def.fee_bps > MAX_FEE_BPS {
            return invalid("fee_bps cannot exceed 10000");
        }
        if def.monthly_price > rates::MAX_AMOUNT || def.fee_base > rates::MAX_AMOUNT {
            return invalid("amount out of range");
        }
        let input = PlatformPlanInput {
            name: &name,
            live: def.live,
            monthly_price: def.monthly_price,
            currency: &currency,
            included_volume: def.included_volume,
            fee_bps: def.fee_bps,
            fee_base: def.fee_base,
            limits: def.limits,
            active: def.active,
        };
        let id = match id {
            Some(id) => {
                if !db::update_platform_plan(&self.pool, id, &input).await? {
                    return Err(Error::NotFound);
                }
                id
            }
            None => db::insert_platform_plan(&self.pool, &input, self.clock.now()).await?,
        };
        self.platform_plan(id).await
    }

    pub async fn set_platform_tenant(&self, tenant_id: Option<u64>) -> Result<()> {
        if let Some(id) = tenant_id {
            self.tenant(id).await?;
        }
        db::set_platform_tenant(&self.pool, tenant_id).await?;
        Ok(())
    }

    pub async fn change_platform_plan(
        &self,
        tenant_id: u64,
        plan_id: u64,
        reason: &str,
    ) -> Result<(PlatformPlanRow, PlatformPlanRow, i64)> {
        let reason = clean_reason(reason)?;
        let tenant = self.tenant(tenant_id).await?;
        let new = self.platform_plan(plan_id).await?;
        if !new.active {
            return invalid("plan is archived");
        }
        let account = self.ensure_platform_account(&tenant).await?;
        let old = self.platform_plan(account.plan_id).await?;
        if old.currency != new.currency {
            return invalid("the new plan must use the same currency");
        }
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let mut account = db::lock_platform_account(&mut *tx, tenant_id)
            .await?
            .ok_or(Error::NotFound)?;
        let diff = crate::billing::prorated_difference(
            account.period_fee,
            new.monthly_price,
            account.period_start,
            account.period_end,
            now,
        );
        if diff > 0 {
            let lines = vec![InvoiceLine::new(
                format!("Upgrade to {}, prorated: {}", new.name, reason),
                1,
                diff as u64,
            )];
            let key = format!("upgrade-{tenant_id}-{}", now.timestamp());
            self.insert_invoice(
                &mut tx,
                &tenant,
                &new,
                platform::KIND_UPGRADE,
                &key,
                (now, account.period_end),
                Metered::default(),
                lines,
                0,
            )
            .await?;
        } else {
            account.credit += (-diff) as u64;
        }
        account.plan_id = new.id;
        account.period_fee = new.monthly_price;
        db::save_platform_account(&mut *tx, &account).await?;
        tx.commit().await?;
        Ok((old, new, diff))
    }

    pub async fn grant_platform_credit(
        &self,
        tenant_id: u64,
        amount: u64,
        reason: &str,
    ) -> Result<u64> {
        clean_reason(reason)?;
        if amount == 0 || amount > rates::MAX_AMOUNT {
            return invalid("amount must be positive");
        }
        let tenant = self.tenant(tenant_id).await?;
        self.ensure_platform_account(&tenant).await?;
        let mut tx = self.pool.begin().await?;
        let mut account = db::lock_platform_account(&mut *tx, tenant_id)
            .await?
            .ok_or(Error::NotFound)?;
        account.credit = account.credit.saturating_add(amount);
        db::save_platform_account(&mut *tx, &account).await?;
        tx.commit().await?;
        Ok(account.credit)
    }

    pub async fn waive_platform_invoice(
        &self,
        tenant_id: u64,
        invoice_id: u64,
        reason: &str,
    ) -> Result<PlatformInvoiceRow> {
        let reason = clean_reason(reason)?;
        let tenant = self.tenant(tenant_id).await?;
        let now = self.clock.now();
        let mut tx = self.pool.begin().await?;
        let inv = db::get_platform_invoice(&mut *tx, tenant_id, invoice_id)
            .await?
            .ok_or(Error::NotFound)?;
        let locked = db::lock_platform_invoice(&mut *tx, inv.id)
            .await?
            .ok_or(Error::NotFound)?;
        if locked.status != platform::INVOICE_OPEN {
            return Err(Error::Conflict(format!("invoice is {}", locked.status)));
        }
        db::settle_platform_invoice(
            &mut *tx,
            inv.id,
            platform::INVOICE_WAIVED,
            Some(&reason),
            now,
        )
        .await?;
        self.emit_platform_invoice(&mut tx, &tenant, inv.id, "platform.invoice.waived")
            .await?;
        tx.commit().await?;
        if let Some(p) = &inv.payment_id
            && let Ok(pay) = self.payment_for_tenant_id(p).await
            && pay.status == "pending"
            && let Some(platform_tenant) = db::platform_tenant(&self.pool).await?
        {
            let platform_tenant = self.tenant(platform_tenant).await?;
            let _ = self
                .cancel_payment(&platform_tenant, &db::payment_id_of(p))
                .await;
        }
        self.review_platform_standing(tenant_id).await?;
        db::get_platform_invoice(&self.pool, tenant_id, invoice_id)
            .await?
            .ok_or(Error::NotFound)
    }

    async fn payment_for_tenant_id(&self, p: &[u8]) -> Result<PaymentRow> {
        db::get_payment(&self.pool, &db::payment_id_of(p))
            .await?
            .ok_or(Error::NotFound)
    }
}
