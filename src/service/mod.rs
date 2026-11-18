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

mod attempts;
mod deliveries;
mod expiry;
mod payments;
mod refunds;
mod saved;
mod settlement;
pub use deliveries::{backoff_secs, webhook_signature};
