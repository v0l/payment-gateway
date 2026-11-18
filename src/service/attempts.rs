use super::*;

impl Service {
    pub async fn create_attempt(
        &self,
        tenant: &Tenant,
        payment_id: &PaymentId,
        input: CreateAttempt,
    ) -> Result<(PaymentRow, AttemptRow)> {
        let p = self.payment_for_tenant(tenant, payment_id).await?;
        let now = Utc::now();
        if p.status != "pending" || p.expires <= now {
            return Err(Error::Conflict(format!("payment is {}", p.status)));
        }
        let method = input.method.trim().to_lowercase();
        if !METHODS.contains(&method.as_str()) {
            return invalid("unknown method");
        }
        if Self::allowed_methods(&p).is_some_and(|a| !a.contains(&method)) {
            return invalid("method not allowed for this payment");
        }
        if !method_supports(&method, &p.currency) {
            return invalid(format!("{method} cannot be used for {}", p.currency));
        }
        let configs = db::list_provider_configs(&self.pool, tenant.id, true).await?;
        let cfg = configs
            .iter()
            .find(|c| c.method == method && input.provider_id.map(|id| id == c.id).unwrap_or(true))
            .ok_or_else(|| Error::Invalid(format!("no {method} provider configured")))?
            .clone();

        let option = input.option.as_ref().map(|o| o.to_uppercase());
        let option_key = option.clone().unwrap_or_default();
        if method == "fixedfloat" {
            let config = self.registry.open_config(&cfg)?;
            let coins = options_from_config("fixedfloat", &config);
            match &option {
                Some(o) if coins.contains(o) => {}
                _ => return invalid(format!("option must be one of {coins:?}")),
            }
        } else if option.is_some() {
            return invalid("this method takes no option");
        }
        if input.save_method && !is_fiat_method(&method) {
            return invalid("only card methods can save a payment method");
        }
        if input.save_method && p.customer_ref.is_none() {
            return invalid("customer_ref is required to save a payment method");
        }

        let existing = db::attempts_for_payment(&self.pool, payment_id).await?;
        if input.saved_method_id.is_none()
            && let Some(a) = existing.iter().find(|a| {
                a.status == "pending"
                    && a.provider_config_id == cfg.id
                    && a.option_key == option_key
                    && a.save_method == input.save_method
                    && a.expires > now
            })
        {
            return Ok((p, a.clone()));
        }
        Self::check_attempt_count(tenant, existing.len())?;

        let (fee, charge, charge_cur, rate) = self.charge_numbers(&cfg, &p).await?;
        let receive_cfg = if method == "fixedfloat" {
            Some(
                configs
                    .iter()
                    .find(|c| c.method == "onchain")
                    .ok_or_else(|| Error::Invalid("fixedfloat needs an onchain provider".into()))?
                    .clone(),
            )
        } else {
            None
        };
        let watch_id = receive_cfg.as_ref().map(|c| c.id).unwrap_or(cfg.id);
        let provider = self
            .registry
            .get(cfg.id)
            .await
            .map_err(|e| Error::Upstream(format!("provider unavailable: {e:#}")))?;
        let receive = match &receive_cfg {
            Some(c) => Some(
                self.registry
                    .get(c.id)
                    .await
                    .map_err(|e| Error::Upstream(format!("provider unavailable: {e:#}")))?,
            ),
            None => None,
        };
        let (mut success_url, cancel_url) = self.return_urls(tenant, &p);
        if cfg.method == "paypal" {
            success_url = format!("{}/return", self.pay_url(payment_id));
        }
        let req = AttemptRequest {
            payment_id: *payment_id,
            amount: charge,
            currency: charge_cur.clone(),
            description: p
                .description
                .clone()
                .unwrap_or_else(|| format!("Payment {}", p.reference)),
            expires: p.expires,
            success_url: Some(success_url),
            cancel_url: Some(cancel_url),
            customer_ref: p.customer_ref.clone(),
            customer_email: p.customer_email.clone(),
            option: option.clone(),
            save_method: input.save_method,
            receive,
            lines: Self::provider_lines(&p, &charge_cur, charge),
        };

        let (created, saved_charge) = match input.saved_method_id {
            Some(sid) => {
                let saved = db::get_saved_method(&self.pool, tenant.id, sid)
                    .await?
                    .ok_or(Error::NotFound)?;
                if saved.provider_config_id != cfg.id {
                    return invalid("saved method belongs to a different provider");
                }
                if p.customer_ref.as_deref() != Some(saved.customer_ref.as_str()) {
                    return invalid("saved method belongs to a different customer");
                }
                let c = provider
                    .charge_saved(&saved, req)
                    .await
                    .map_err(|e| Error::Upstream(format!("charge failed: {e:#}")))?;
                (c, true)
            }
            None => (
                provider
                    .create(req)
                    .await
                    .map_err(|e| Error::Upstream(format!("provider error: {e:#}")))?,
                false,
            ),
        };

        let details =
            serde_json::to_string(&created.details).map_err(|e| Error::Internal(e.into()))?;
        let attempt_expires = created
            .expires
            .map(|e| e.min(p.expires))
            .unwrap_or(p.expires);
        let attempt_id = db::insert_attempt(
            &self.pool,
            &NewAttempt {
                payment_id: *payment_id,
                provider_config_id: cfg.id,
                watch_provider_id: if saved_charge { cfg.id } else { watch_id },
                method: &method,
                option_key: &option_key,
                base_amount: p.amount,
                processing_fee: fee,
                currency: &p.currency,
                charge_amount: charge,
                charge_currency: &charge_cur,
                rate,
                lookup: &created.lookup,
                external_id: created.external_id.as_deref(),
                details: &details,
                created: now,
                expires: attempt_expires,
                save_method: input.save_method,
            },
        )
        .await?;
        if created.completed {
            self.settle_attempt(attempt_id, &created.lookup, None, true)
                .await?;
        }
        self.notify(payment_id);
        let attempt = db::get_attempt(&self.pool, attempt_id)
            .await?
            .ok_or(Error::NotFound)?;
        let p = db::get_payment(&self.pool, payment_id)
            .await?
            .ok_or(Error::NotFound)?;
        Ok((p, attempt))
    }
}
