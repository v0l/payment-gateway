use super::*;

impl Service {
    pub async fn charge_saved(
        &self,
        tenant: &Tenant,
        saved_id: u64,
        mut input: CreatePayment,
        idempotency_key: Option<&str>,
    ) -> Result<(PaymentRow, bool)> {
        let saved = db::get_saved_method(&self.pool, tenant.id, saved_id)
            .await?
            .ok_or(Error::NotFound)?;
        let cfg = db::get_provider_config(&self.pool, saved.provider_config_id)
            .await?
            .filter(|c| c.enabled && c.tenant_id == tenant.id)
            .ok_or_else(|| {
                Error::Invalid("the provider for this saved method is disabled".into())
            })?;
        input.customer_ref = Some(saved.customer_ref.clone());
        input.methods = Some(vec![cfg.method.clone()]);
        let (payment, created) = self.create_payment(tenant, input, idempotency_key).await?;
        if !created {
            return Ok((payment, false));
        }
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
        match result {
            Ok((p, _)) => Ok((p, true)),
            Err(e) => {
                db::set_payment_status(&self.pool, &pid, &["pending"], "cancelled").await?;
                Err(e)
            }
        }
    }

    pub async fn import_saved(
        &self,
        tenant: &Tenant,
        input: ImportSaved,
    ) -> Result<db::SavedMethodRow> {
        let customer_ref = input.customer_ref.trim();
        if customer_ref.is_empty() || customer_ref.len() > 128 {
            return invalid_field("customer_ref", "customer_ref must be 1-128 characters");
        }
        for (field, v) in [
            ("external_customer_id", &input.external_customer_id),
            ("external_id", &input.external_id),
        ] {
            if v.is_empty() || v.len() > 255 || v.chars().any(char::is_control) {
                return invalid_field(field, format!("{field} must be 1-255 characters"));
            }
        }
        if input.brand.as_ref().is_some_and(|b| b.len() > 32) {
            return invalid_field("brand", "brand is at most 32 characters");
        }
        if input.last4.as_ref().is_some_and(|l| l.len() > 8) {
            return invalid_field("last4", "last4 is at most 8 characters");
        }
        if input.exp_month.is_some_and(|m| !(1..=12).contains(&m)) {
            return invalid_field("exp_month", "exp_month must be 1 to 12");
        }
        if input.exp_year.is_some_and(|y| !(2000..=2200).contains(&y)) {
            return invalid_field("exp_year", "exp_year must be a four digit year");
        }
        let cfg = db::get_provider_config(&self.pool, input.provider_id)
            .await?
            .filter(|c| c.tenant_id == tenant.id)
            .ok_or(Error::NotFound)?;
        if !cfg.enabled || !is_fiat_method(&cfg.method) {
            return invalid_field("provider_id", "saved methods need an enabled card provider");
        }
        if let Some(existing) =
            db::get_saved_method_by_external(&self.pool, cfg.id, &input.external_id).await?
            && existing.customer_ref != customer_ref
        {
            return invalid_field(
                "external_id",
                "this method is already saved for another customer",
            );
        }
        db::upsert_saved_method(
            &self.pool,
            &NewSavedMethod {
                tenant_id: tenant.id,
                customer_ref,
                provider_config_id: cfg.id,
                external_customer_id: &input.external_customer_id,
                external_id: &input.external_id,
                brand: input.brand.as_deref(),
                last4: input.last4.as_deref(),
                exp_month: input.exp_month,
                exp_year: input.exp_year,
            },
        )
        .await?;
        db::get_saved_method_by_external(&self.pool, cfg.id, &input.external_id)
            .await?
            .ok_or(Error::NotFound)
    }
}

#[derive(Debug, serde::Deserialize)]
pub struct ImportSaved {
    pub customer_ref: String,
    pub provider_id: u64,
    pub external_customer_id: String,
    pub external_id: String,
    pub brand: Option<String>,
    pub last4: Option<String>,
    pub exp_month: Option<u16>,
    pub exp_year: Option<u16>,
}
