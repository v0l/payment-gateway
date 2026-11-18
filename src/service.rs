use crate::crypto::Sealer;
use crate::db::{
    self, AttemptRow, NewAttempt, NewPayment, NewSavedMethod, PaymentId, PaymentRow,
    ProviderConfigRow, Tenant,
};
use crate::model::{
    AttemptView, CreateAttempt, CreatePayment, METHODS, PaymentView, RefundInput, RefundView,
    charge_currency, is_fiat_method, method_supports, options_from_config,
};
use crate::providers::{AttemptRequest, EventSink, ProviderEvent, RefundRequest, Registry};
use crate::rates::{self, RateService};
use anyhow::anyhow;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::Sha256;
use sqlx::MySqlPool;
use std::sync::Arc;
use tokio::sync::broadcast;

pub const MIN_EXPIRY_SECS: i64 = 60;
pub const MAX_EXPIRY_SECS: i64 = 30 * 24 * 3600;
pub const DEFAULT_EXPIRY_SECS: i64 = 3600;
pub const UNCONFIRMED_GRACE_SECS: i64 = 24 * 3600;
pub const WEBHOOK_GIVE_UP_SECS: i64 = 3 * 24 * 3600;

#[derive(Debug)]
pub enum Error {
    NotFound,
    Invalid(String),
    Conflict(String),
    Upstream(String),
    Internal(anyhow::Error),
}

impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<sqlx::Error>() {
            Ok(e) => e.into(),
            Err(e) => Error::Internal(e),
        }
    }
}

impl From<sqlx::Error> for Error {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(d) = &e
            && d.is_unique_violation()
        {
            return Error::Conflict("already exists".into());
        }
        Error::Internal(e.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

fn invalid<T>(msg: impl Into<String>) -> Result<T> {
    Err(Error::Invalid(msg.into()))
}

pub struct Service {
    pub pool: MySqlPool,
    pub sealer: Sealer,
    pub registry: Registry,
    pub rates: Arc<dyn RateService>,
    pub public_url: String,
    pub http: reqwest::Client,
    pub changes: broadcast::Sender<PaymentId>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MethodChoice {
    pub provider_id: u64,
    pub method: String,
    pub name: String,
    pub options: Vec<String>,
    pub fee: u64,
    pub total: u64,
    pub currency: String,
}

fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

pub fn required_receipt(a: &AttemptRow) -> u64 {
    if a.charge_currency == "BTC" {
        a.charge_amount.div_ceil(1000) * 1000
    } else {
        a.charge_amount
    }
}

fn valid_url(u: &str) -> bool {
    url::Url::parse(u)
        .map(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
        .unwrap_or(false)
}

pub fn sign_return(secret: &str, payment_id: &str, reference: &str, status: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(format!("{payment_id}.{reference}.{status}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

impl Service {
    pub fn new(
        pool: MySqlPool,
        sealer: Sealer,
        registry: Registry,
        rates: Arc<dyn RateService>,
        public_url: String,
    ) -> Arc<Self> {
        let (changes, _) = broadcast::channel(1024);
        Arc::new(Self {
            pool,
            sealer,
            registry,
            rates,
            public_url: public_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("http client"),
            changes,
        })
    }

    pub fn pay_url(&self, id: &PaymentId) -> String {
        format!("{}/pay/{}", self.public_url, hex::encode(id))
    }

    pub fn hook_url(&self, tenant_id: u64, provider_id: u64) -> String {
        format!("{}/hook/{tenant_id}/{provider_id}", self.public_url)
    }

    pub fn notify(&self, id: &PaymentId) {
        let _ = self.changes.send(*id);
    }

    pub async fn tenant(&self, id: u64) -> Result<Tenant> {
        db::get_tenant(&self.pool, id).await?.ok_or(Error::NotFound)
    }

    pub async fn payment_for_tenant(&self, tenant: &Tenant, id: &PaymentId) -> Result<PaymentRow> {
        db::get_payment(&self.pool, id)
            .await?
            .filter(|p| p.tenant_id == tenant.id)
            .ok_or(Error::NotFound)
    }

    pub async fn view(&self, p: &PaymentRow) -> Result<PaymentView> {
        let id = p.pid();
        let attempts = db::attempts_for_payment(&self.pool, &id).await?;
        let refunds = db::refunds_for_payment(&self.pool, &id).await?;
        Ok(PaymentView::build(p, &self.public_url, &attempts, &refunds))
    }

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
        match db::insert_payment(&self.pool, &new).await {
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

    fn allowed_methods(p: &PaymentRow) -> Option<Vec<String>> {
        p.methods
            .as_deref()
            .and_then(|m| serde_json::from_str(m).ok())
    }

    async fn charge_numbers(
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

    fn return_urls(&self, tenant: &Tenant, p: &PaymentRow) -> (String, String) {
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

        if input.saved_method_id.is_none() {
            let existing = db::attempts_for_payment(&self.pool, payment_id).await?;
            if let Some(a) = existing.into_iter().find(|a| {
                a.status == "pending"
                    && a.provider_config_id == cfg.id
                    && a.option_key == option_key
                    && a.save_method == input.save_method
                    && a.expires > now
            }) {
                return Ok((p, a));
            }
        }

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
        let (success_url, cancel_url) = self.return_urls(tenant, &p);
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

    async fn emit(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
        tenant: &Tenant,
        p: &PaymentRow,
        event_type: &str,
        extra: Value,
    ) -> Result<()> {
        if tenant.webhook_url.is_none() {
            return Ok(());
        }
        let id = p.pid();
        let attempts = db::attempts_for_payment(&mut **tx, &id).await?;
        let refunds = db::refunds_for_payment(&mut **tx, &id).await?;
        let fresh = db::get_payment(&mut **tx, &id)
            .await?
            .unwrap_or_else(|| p.clone());
        let view = PaymentView::build(&fresh, &self.public_url, &attempts, &refunds);
        let now = Utc::now();
        let event_id = random_hex(16);
        let body = json!({
            "id": event_id,
            "type": event_type,
            "created": now.timestamp(),
            "data": { "payment": view, "detail": extra },
        });
        db::insert_delivery(
            &mut **tx,
            tenant.id,
            &id,
            &event_id,
            event_type,
            &body.to_string(),
            now,
        )
        .await?;
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
                    log::debug!("provider {provider_id}: payment for unknown lookup {lookup}");
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
        }
    }

    pub async fn settle_attempt(
        &self,
        attempt_id: u64,
        reference: &str,
        amount: Option<u64>,
        confirmed: bool,
    ) -> Result<()> {
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
            Utc::now(),
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

        let now = Utc::now();
        db::mark_attempt_paid(&mut *tx, attempt.id, now).await?;
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
                self.emit(
                    &mut tx,
                    &tenant,
                    &p,
                    "payment.paid",
                    json!({"attempt_id": attempt.id, "received": received}),
                )
                .await?;
            }
            _ => {
                self.emit(
                    &mut tx,
                    &tenant,
                    &p,
                    "payment.late_payment",
                    json!({"attempt_id": attempt.id, "received": received}),
                )
                .await?;
            }
        }
        tx.commit().await?;
        self.notify(&pid);
        for other in to_cancel {
            self.cancel_at_provider(&other).await;
        }
        Ok(())
    }

    async fn cancel_at_provider(&self, a: &AttemptRow) {
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

    pub async fn cancel_payment(&self, tenant: &Tenant, id: &PaymentId) -> Result<PaymentRow> {
        let preliminary = self.payment_for_tenant(tenant, id).await?;
        let mut tx = self.pool.begin().await?;
        let p = db::lock_payment(&mut *tx, id)
            .await?
            .ok_or(Error::NotFound)?;
        if p.status != "pending" {
            return Err(Error::Conflict(format!("payment is {}", p.status)));
        }
        if db::has_unconfirmed_receipts(&self.pool, id).await? {
            return Err(Error::Conflict("funds are already on their way".into()));
        }
        db::set_payment_status(&mut *tx, id, &["pending"], "cancelled").await?;
        let mut cancelled = vec![];
        for a in db::attempts_for_payment(&mut *tx, id).await? {
            if a.status == "pending" {
                db::set_attempt_status(&mut *tx, a.id, &["pending"], "cancelled").await?;
                cancelled.push(a);
            }
        }
        self.emit(
            &mut tx,
            tenant,
            &preliminary,
            "payment.cancelled",
            json!({}),
        )
        .await?;
        tx.commit().await?;
        self.notify(id);
        for a in cancelled {
            self.cancel_at_provider(&a).await;
        }
        db::get_payment(&self.pool, id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub async fn expire_due(&self) -> Result<usize> {
        let now = Utc::now();
        let mut count = 0;
        for due in db::due_pending_payments(&self.pool, now).await? {
            let pid = due.pid();
            if db::has_unconfirmed_receipts(&self.pool, &pid).await?
                && now < due.expires + Duration::seconds(UNCONFIRMED_GRACE_SECS)
            {
                continue;
            }
            let tenant = self.tenant(due.tenant_id).await?;
            let mut tx = self.pool.begin().await?;
            let Some(p) = db::lock_payment(&mut *tx, &pid).await? else {
                continue;
            };
            if p.status != "pending" || p.expires > now {
                continue;
            }
            db::set_payment_status(&mut *tx, &pid, &["pending"], "expired").await?;
            let mut expired = vec![];
            for a in db::attempts_for_payment(&mut *tx, &pid).await? {
                if a.status == "pending" {
                    db::set_attempt_status(&mut *tx, a.id, &["pending"], "expired").await?;
                    expired.push(a);
                }
            }
            self.emit(&mut tx, &tenant, &p, "payment.expired", json!({}))
                .await?;
            tx.commit().await?;
            self.notify(&pid);
            for a in expired {
                self.cancel_at_provider(&a).await;
            }
            count += 1;
        }
        for a in db::due_pending_attempts(&self.pool, now).await? {
            if db::set_attempt_status(&self.pool, a.id, &["pending"], "expired").await? {
                self.notify(&db::payment_id_of(&a.payment_id));
            }
        }
        Ok(count)
    }

    pub async fn refund(
        &self,
        tenant: &Tenant,
        id: &PaymentId,
        input: RefundInput,
    ) -> Result<RefundView> {
        self.payment_for_tenant(tenant, id).await?;
        let mut tx = self.pool.begin().await?;
        let p = db::lock_payment(&mut *tx, id)
            .await?
            .ok_or(Error::NotFound)?;
        if !matches!(p.status.as_str(), "paid" | "partially_refunded") {
            return Err(Error::Conflict(format!("payment is {}", p.status)));
        }
        let attempts = db::attempts_for_payment(&mut *tx, id).await?;
        let attempt = match input.attempt_id {
            Some(aid) => attempts.iter().find(|a| a.id == aid && a.status == "paid"),
            None => attempts
                .iter()
                .filter(|a| a.status == "paid")
                .min_by_key(|a| a.paid_at),
        }
        .ok_or_else(|| Error::Invalid("no paid attempt to refund".into()))?
        .clone();

        let refunds = db::refunds_for_payment(&mut *tx, id).await?;
        let committed: u64 = refunds
            .iter()
            .filter(|r| r.attempt_id == attempt.id && r.status != "failed")
            .map(|r| r.amount)
            .sum();
        let remaining = p.amount.saturating_sub(committed);
        let amount = input.amount.unwrap_or(remaining);
        if amount == 0 || amount > remaining {
            return Err(Error::Invalid(format!("refundable amount is {remaining}")));
        }
        let charge_committed: u64 = refunds
            .iter()
            .filter(|r| r.attempt_id == attempt.id && r.status != "failed")
            .map(|r| r.charge_amount)
            .sum();
        let gross = (attempt.base_amount + attempt.processing_fee).max(1) as u128;
        let mut charge = (attempt.charge_amount as u128 * amount as u128 / gross) as u64;
        if amount == remaining {
            charge = charge
                .max(
                    ((attempt.charge_amount as u128 * attempt.base_amount as u128 / gross) as u64)
                        .saturating_sub(charge_committed),
                )
                .min(attempt.charge_amount.saturating_sub(charge_committed));
        }
        if charge == 0 {
            return Err(Error::Invalid("refund is too small".into()));
        }
        if attempt.charge_currency == "BTC" {
            charge = (charge / 1000) * 1000;
            if charge == 0 {
                return Err(Error::Invalid("refund is below 1 sat".into()));
            }
        }
        let refund_id = db::insert_refund(
            &mut *tx,
            id,
            attempt.id,
            amount,
            charge,
            &attempt.charge_currency,
            input.destination.as_deref(),
        )
        .await;
        let refund_id = match refund_id {
            Ok(r) => r,
            Err(e) => return Err(e.into()),
        };
        tx.commit().await?;

        let provider_id = if attempt.method == "fixedfloat" {
            attempt.watch_provider_id
        } else {
            attempt.provider_config_id
        };
        let outcome = match self.registry.get(provider_id).await {
            Ok(provider) => {
                provider
                    .refund(
                        &attempt,
                        RefundRequest {
                            amount: charge,
                            currency: attempt.charge_currency.clone(),
                            destination: input.destination.clone(),
                        },
                    )
                    .await
            }
            Err(e) => Err(e),
        };
        let mut tx = self.pool.begin().await?;
        let p = db::lock_payment(&mut *tx, id)
            .await?
            .ok_or(Error::NotFound)?;
        match outcome {
            Ok(done) => {
                db::finish_refund(
                    &mut *tx,
                    refund_id,
                    "completed",
                    done.external_id.as_deref(),
                    None,
                )
                .await?;
                let total: u64 = db::refunds_for_payment(&mut *tx, id)
                    .await?
                    .iter()
                    .filter(|r| r.status == "completed")
                    .map(|r| r.amount)
                    .sum();
                let status = if total >= p.amount {
                    "refunded"
                } else {
                    "partially_refunded"
                };
                db::set_payment_status(&mut *tx, id, &["paid", "partially_refunded"], status)
                    .await?;
                self.emit(
                    &mut tx,
                    tenant,
                    &p,
                    "refund.completed",
                    json!({"refund_id": refund_id, "amount": amount}),
                )
                .await?;
            }
            Err(e) => {
                let msg = format!("{e:#}");
                db::finish_refund(&mut *tx, refund_id, "failed", None, Some(&msg)).await?;
                self.emit(
                    &mut tx,
                    tenant,
                    &p,
                    "refund.failed",
                    json!({"refund_id": refund_id, "error": msg}),
                )
                .await?;
            }
        }
        tx.commit().await?;
        self.notify(id);
        let row = db::get_refund(&self.pool, refund_id)
            .await?
            .ok_or(Error::NotFound)?;
        Ok((&row).into())
    }

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

    pub async fn deliver_due(&self) -> Result<usize> {
        let now = Utc::now();
        let due = db::due_deliveries(&self.pool, now).await?;
        let mut sent = 0;
        for d in due {
            if !db::claim_delivery(&self.pool, d.id, now, 60).await? {
                continue;
            }
            let tenant = self.tenant(d.tenant_id).await?;
            let outcome = self.post_webhook(&tenant, &d.event_id, &d.body).await;
            let now = Utc::now();
            match outcome {
                Ok(()) => {
                    db::delivery_succeeded(&self.pool, d.id, now).await?;
                    sent += 1;
                }
                Err(e) => {
                    let give_up = d.created + Duration::seconds(WEBHOOK_GIVE_UP_SECS);
                    let delay = backoff_secs(d.attempts + 1);
                    let next = (now + Duration::seconds(delay)).min(give_up + Duration::seconds(1));
                    let next = if now >= give_up { None } else { Some(next) };
                    db::delivery_failed(&self.pool, d.id, next, now, &e.to_string()).await?;
                }
            }
        }
        Ok(sent)
    }

    async fn post_webhook(
        &self,
        tenant: &Tenant,
        event_id: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        let url = tenant
            .webhook_url
            .as_deref()
            .ok_or_else(|| anyhow!("tenant has no webhook_url"))?;
        let client = if tenant.trusted {
            self.http.clone()
        } else {
            let target = crate::ssrf::resolve_public(url).await?;
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .resolve(&target.host, target.addr)
                .build()?
        };
        let secret = String::from_utf8(self.sealer.open(&tenant.webhook_secret)?)?;
        let ts = Utc::now().timestamp();
        let sig = webhook_signature(&secret, ts, body);
        let rsp = client
            .post(url)
            .header("content-type", "application/json")
            .header("x-gateway-event-id", event_id)
            .header("x-gateway-signature", format!("t={ts},v1={sig}"))
            .body(body.to_string())
            .send()
            .await?;
        if !rsp.status().is_success() {
            return Err(anyhow!("webhook returned {}", rsp.status()));
        }
        Ok(())
    }
}

pub fn backoff_secs(attempt: u32) -> i64 {
    (15i64 << attempt.min(11)).min(6 * 3600)
}

pub fn webhook_signature(secret: &str, ts: i64, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(format!("{ts}.{body}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub struct ProviderSink {
    pub service: Arc<Service>,
    pub provider_id: u64,
}

#[async_trait]
impl EventSink for ProviderSink {
    async fn event(&self, event: ProviderEvent, cursor: Option<String>) -> anyhow::Result<()> {
        self.service
            .apply_event(self.provider_id, event)
            .await
            .map_err(|e| anyhow!("{e:?}"))?;
        if let Some(c) = cursor {
            db::set_cursor(&self.service.pool, self.provider_id, &c).await?;
        }
        Ok(())
    }
}

#[allow(dead_code)]
fn _assert_types(_: DateTime<Utc>, _: AttemptView) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_and_caps() {
        assert_eq!(backoff_secs(1), 30);
        assert_eq!(backoff_secs(2), 60);
        assert_eq!(backoff_secs(30), 6 * 3600);
    }

    #[test]
    fn signatures_depend_on_secret_time_and_body() {
        let a = webhook_signature("s", 1, "{}");
        assert_eq!(a, webhook_signature("s", 1, "{}"));
        assert_ne!(a, webhook_signature("s2", 1, "{}"));
        assert_ne!(a, webhook_signature("s", 2, "{}"));
        assert_ne!(a, webhook_signature("s", 1, "{ }"));
    }

    #[test]
    fn return_signature_binds_status() {
        assert_ne!(
            sign_return("s", "id", "ref", "paid"),
            sign_return("s", "id", "ref", "cancelled")
        );
    }
}
