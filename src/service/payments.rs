use super::*;

fn invoice_field(message: &str) -> &'static str {
    if message.contains("discount") {
        "discount"
    } else {
        "line_items"
    }
}

impl Service {
    pub async fn create_payment(
        &self,
        tenant: &Tenant,
        input: CreatePayment,
        idempotency_key: Option<&str>,
    ) -> Result<(PaymentRow, bool)> {
        let supplied = input.reference.as_ref().map(|r| r.trim().to_string());
        let reference = supplied
            .clone()
            .unwrap_or_else(|| format!("pay_{}", random_hex(12)));
        if reference.is_empty() || reference.len() > 255 {
            return invalid("reference must be 1-255 characters");
        }
        if reference.chars().any(char::is_control) {
            return invalid("reference must not contain control characters");
        }
        let breakdown = match (&input.line_items, &input.discount) {
            (None, Some(_)) => return invalid_field("discount", "discount requires line_items"),
            (None, None) => None,
            (Some(items), discount) => {
                let b = crate::invoice::compute(items, discount.as_ref())
                    .map_err(|m| Error::InvalidField(invoice_field(&m), m))?;
                if input.amount != 0 && input.amount != b.total {
                    return invalid_field(
                        "amount",
                        format!(
                            "amount {} does not match the line items total {}",
                            input.amount, b.total
                        ),
                    );
                }
                if input.tax != 0 && input.tax != b.tax {
                    return invalid_field(
                        "tax",
                        format!(
                            "tax {} does not match the line items tax {}",
                            input.tax, b.tax
                        ),
                    );
                }
                Some(b)
            }
        };
        let (amount, tax) = match &breakdown {
            Some(b) => (b.total, b.tax),
            None => (input.amount, input.tax),
        };
        if amount == 0 {
            return invalid("amount must be positive");
        }
        let currency = input.currency.trim().to_uppercase();
        if rates::unit(&currency).is_err() {
            return invalid(format!("unsupported currency {currency}"));
        }
        if amount > rates::MAX_AMOUNT {
            return invalid(format!("amount cannot exceed {}", rates::MAX_AMOUNT));
        }
        if tax > amount {
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
            if !crate::model::url_in_origins(u, &tenant.origins()) {
                return invalid(
                    "success_url and cancel_url must be on one of the project's allowed_origins",
                );
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
            if v.as_ref().is_some_and(|s| s.chars().any(char::is_control)) {
                return invalid(format!("{name} must not contain control characters"));
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
        let line_items = breakdown
            .as_ref()
            .map(|b| serde_json::to_string(b).map_err(|e| Error::Internal(e.into())))
            .transpose()?;
        let methods = input
            .methods
            .as_ref()
            .map(|m| serde_json::to_string(m).unwrap());

        if let Some(key) = idempotency_key {
            if key.is_empty() || key.len() > 128 {
                return invalid("Idempotency-Key must be 1-128 characters");
            }
            if let Some(existing) = db::payment_by_idempotency(&self.pool, tenant.id, key).await? {
                return self.same_request(existing, supplied.as_deref(), amount, &currency);
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
            amount,
            tax,
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
            line_items: line_items.as_deref(),
        };
        let mut tx = self.pool.begin().await?;
        self.check_new_payment(&mut tx, tenant, amount, &currency)
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
                    return self.same_request(existing, supplied.as_deref(), amount, &currency);
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
        reference: Option<&str>,
        amount: u64,
        currency: &str,
    ) -> Result<(PaymentRow, bool)> {
        if reference.is_some_and(|r| existing.reference != r)
            || existing.amount != amount
            || existing.currency != currency
        {
            return Err(Error::Conflict(
                "Idempotency-Key was used with different parameters".into(),
            ));
        }
        Ok((existing, false))
    }

    pub(super) fn provider_lines(
        p: &PaymentRow,
        charge_currency: &str,
        charge: u64,
    ) -> Vec<crate::invoice::ChargeLine> {
        let Some(b) = p.breakdown() else {
            return vec![];
        };
        if !charge_currency.eq_ignore_ascii_case(&p.currency) || charge < b.total {
            return vec![];
        }
        b.charge_lines(charge - b.total)
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
        let configs = db::list_provider_configs(&self.pool, tenant.id, true).await?;
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
            });
        }
        Ok(out)
    }

    pub(super) fn return_urls(&self, tenant: &Tenant, p: &PaymentRow) -> (String, String) {
        let fallback = self.pay_url(&p.pid());
        let origins = tenant.origins();
        let allowed = |u: Option<&String>| {
            u.filter(|u| crate::model::url_in_origins(u, &origins))
                .cloned()
        };
        (
            allowed(p.success_url.as_ref())
                .or_else(|| allowed(tenant.default_success_url.as_ref()))
                .unwrap_or_else(|| fallback.clone()),
            allowed(p.cancel_url.as_ref())
                .or_else(|| allowed(tenant.default_cancel_url.as_ref()))
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
