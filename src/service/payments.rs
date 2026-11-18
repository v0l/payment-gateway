use super::*;

impl Service {
    pub async fn create_payment(
        &self,
        tenant: &Tenant,
        input: CreatePayment,
        idempotency_key: Option<&str>,
    ) -> Result<(PaymentRow, bool)> {
        let reference = input.reference.trim().to_string();
        if reference.is_empty() || reference.len() > 255 {
            return invalid("reference must be 1-255 characters");
        }
        if input.amount == 0 {
            return invalid("amount must be positive");
        }
        let currency = input.currency.trim().to_uppercase();
        if rates::unit(&currency).is_err() {
            return invalid(format!("unsupported currency {currency}"));
        }
        if input.tax > input.amount {
            return invalid("tax cannot exceed amount");
        }
        let expires_in = input.expires_in.unwrap_or(DEFAULT_EXPIRY_SECS);
        if !(MIN_EXPIRY_SECS..=MAX_EXPIRY_SECS).contains(&expires_in) {
            return invalid(format!(
                "expires_in must be {MIN_EXPIRY_SECS}-{MAX_EXPIRY_SECS} seconds"
            ));
        }
        for u in [&input.success_url, &input.cancel_url]
            .into_iter()
            .flatten()
        {
            if !valid_url(u) {
                return invalid("invalid success_url or cancel_url");
            }
        }
        if let Some(methods) = &input.methods
            && (methods.is_empty() || methods.iter().any(|m| !METHODS.contains(&m.as_str())))
        {
            return invalid("unknown method in methods");
        }
        for (v, max, name) in [
            (&input.description, 255, "description"),
            (&input.customer_ref, 128, "customer_ref"),
            (&input.customer_email, 255, "customer_email"),
        ] {
            if v.as_ref().is_some_and(|s| s.len() > max) {
                return invalid(format!("{name} too long"));
            }
        }
        let metadata = match &input.metadata {
            Some(Value::Null) | None => None,
            Some(v) => {
                let s = serde_json::to_string(v).map_err(|e| Error::Internal(e.into()))?;
                if s.len() > 8192 {
                    return invalid("metadata too large");
                }
                Some(s)
            }
        };
        let methods = input
            .methods
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap());

        if let Some(key) = idempotency_key {
            if key.is_empty() || key.len() > 128 {
                return invalid("Idempotency-Key must be 1-128 characters");
            }
            if let Some(existing) = db::payment_by_idempotency(&self.pool, tenant.id, key).await? {
                return self.same_request(existing, &reference, input.amount, &currency);
            }
        }

        let mut id = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut id);
        let now = Utc::now();
        let new = NewPayment {
            id,
            tenant_id: tenant.id,
            reference: &reference,
            currency: &currency,
            amount: input.amount,
            tax: input.tax,
            created: now,
            expires: now + Duration::seconds(expires_in),
            description: input.description.as_deref(),
            customer_ref: input.customer_ref.as_deref(),
            customer_email: input.customer_email.as_deref(),
            metadata: metadata.as_deref(),
            methods: methods.as_deref(),
            idempotency_key,
            success_url: input.success_url.as_deref(),
            cancel_url: input.cancel_url.as_deref(),
        };
        let mut tx = self.pool.begin().await?;
        self.check_new_payment(&mut tx, tenant, input.amount, &currency)
            .await?;
        let inserted = db::insert_payment(&mut *tx, &new).await;
        let inserted = match inserted {
            Ok(()) => tx.commit().await.map_err(anyhow::Error::from),
            Err(e) => Err(e),
        };
        match inserted {
            Ok(()) => {}
            Err(e) => {
                let e: Error = e.into();
                if let (Error::Conflict(_), Some(key)) = (&e, idempotency_key)
                    && let Some(existing) =
                        db::payment_by_idempotency(&self.pool, tenant.id, key).await?
                {
                    return self.same_request(existing, &reference, input.amount, &currency);
                }
                return Err(e);
            }
        }
        let row = db::get_payment(&self.pool, &id)
            .await?
            .ok_or(Error::NotFound)?;
        Ok((row, true))
    }

    fn same_request(
        &self,
        existing: PaymentRow,
        reference: &str,
        amount: u64,
        currency: &str,
    ) -> Result<(PaymentRow, bool)> {
        if existing.reference != reference
            || existing.amount != amount
            || existing.currency != currency
        {
            return Err(Error::Conflict(
                "Idempotency-Key was used with different parameters".into(),
            ));
        }
        Ok((existing, false))
    }

    pub(super) fn allowed_methods(p: &PaymentRow) -> Option<Vec<String>> {
        p.methods
            .as_deref()
            .and_then(|m| serde_json::from_str(m).ok())
    }

    pub(super) async fn charge_numbers(
        &self,
        cfg: &ProviderConfigRow,
        p: &PaymentRow,
    ) -> Result<(u64, u64, String, f64)> {
        let fee = rates::processing_fee(
            self.rates.as_ref(),
            cfg.fee_rate,
            cfg.fee_base,
            cfg.fee_currency.as_deref(),
            p.amount,
            &p.currency,
        )
        .await
        .map_err(|e| Error::Upstream(format!("fee calculation failed: {e}")))?;
        let total = p.amount.saturating_add(fee);
        let charge_cur =
            charge_currency(&cfg.method, &p.currency).map_err(|e| Error::Invalid(e.to_string()))?;
        let mut charge = rates::convert(self.rates.as_ref(), total, &p.currency, &charge_cur)
            .await
            .map_err(|e| Error::Upstream(format!("rate lookup failed: {e}")))?;
        if charge_cur == "BTC" {
            charge = charge.div_ceil(1000) * 1000;
        }
        let rate = if total == 0 {
            1.0
        } else {
            charge as f64 / total as f64
        };
        Ok((fee, charge, charge_cur, rate))
    }

    pub async fn available_methods(
        &self,
        tenant: &Tenant,
        p: &PaymentRow,
    ) -> Result<Vec<MethodChoice>> {
        let configs = self.usable_configs(tenant).await?;
        let allowed = Self::allowed_methods(p);
        let has_onchain = configs.iter().any(|c| c.method == "onchain");
        let mut out = vec![];
        for cfg in &configs {
            if !method_supports(&cfg.method, &p.currency) {
                continue;
            }
            if allowed.as_ref().is_some_and(|a| !a.contains(&cfg.method)) {
                continue;
            }
            if cfg.method == "fixedfloat" && !has_onchain {
                continue;
            }
            let options = if cfg.method == "fixedfloat" {
                self.registry
                    .open_config(cfg)
                    .map(|c| options_from_config(&cfg.method, &c))
                    .unwrap_or_default()
            } else {
                vec![]
            };
            let fee = rates::processing_fee(
                self.rates.as_ref(),
                cfg.fee_rate,
                cfg.fee_base,
                cfg.fee_currency.as_deref(),
                p.amount,
                &p.currency,
            )
            .await
            .unwrap_or(0);
            out.push(MethodChoice {
                provider_id: cfg.id,
                method: cfg.method.clone(),
                name: cfg.name.clone(),
                options,
                fee,
                total: p.amount.saturating_add(fee),
                currency: p.currency.clone(),
                platform_managed: cfg.shared_provider_id.is_some(),
            });
        }
        Ok(out)
    }

    pub(super) fn return_urls(&self, tenant: &Tenant, p: &PaymentRow) -> (String, String) {
        let fallback = self.pay_url(&p.pid());
        (
            p.success_url
                .clone()
                .or_else(|| tenant.default_success_url.clone())
                .unwrap_or_else(|| fallback.clone()),
            p.cancel_url
                .clone()
                .or_else(|| tenant.default_cancel_url.clone())
                .unwrap_or(fallback),
        )
    }

    pub fn signed_return_url(&self, tenant: &Tenant, p: &PaymentRow) -> Option<String> {
        let (success, cancel) = self.return_urls(tenant, p);
        let (base, status) = match p.status.as_str() {
            "paid" | "refunded" | "partially_refunded" => (success, "paid"),
            "expired" => (cancel, "expired"),
            "cancelled" => (cancel, "cancelled"),
            _ => return None,
        };
        let secret = self.sealer.open(&tenant.webhook_secret).ok()?;
        let secret = String::from_utf8(secret).ok()?;
        let id = hex::encode(&p.id);
        let sig = sign_return(&secret, &id, &p.reference, status);
        let mut u = url::Url::parse(&base).ok()?;
        u.query_pairs_mut()
            .append_pair("payment_id", &id)
            .append_pair("reference", &p.reference)
            .append_pair("status", status)
            .append_pair("sig", &sig);
        Some(u.to_string())
    }
}
