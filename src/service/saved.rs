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
}
