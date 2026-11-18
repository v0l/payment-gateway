pub mod creds;
pub mod fixedfloat;
pub mod guard;
pub mod lightning;
pub mod mock;
pub mod onchain;
pub mod paypal;
pub mod revolut;
pub mod stripe;
pub mod wallet;
pub mod webhook_check;

pub use wallet::{WalletBalance, WalletSource};

use crate::crypto::Sealer;
use crate::db::{self, AttemptRow, PaymentId, ProviderConfigRow, SavedMethodRow};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::Value;
use sqlx::MySqlPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct AttemptRequest {
    pub payment_id: PaymentId,
    pub amount: u64,
    pub currency: String,
    pub description: String,
    pub expires: DateTime<Utc>,
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
    pub customer_ref: Option<String>,
    pub customer_email: Option<String>,
    pub option: Option<String>,
    pub save_method: bool,
    pub receive: Option<Arc<dyn Provider>>,
    pub lines: Vec<crate::invoice::ChargeLine>,
}

#[derive(Debug, Clone)]
pub struct CreatedAttempt {
    pub lookup: String,
    pub external_id: Option<String>,
    pub details: Value,
    pub expires: Option<DateTime<Utc>>,
    pub completed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SavedMethodInfo {
    pub customer_id: String,
    pub method_id: String,
    pub brand: Option<String>,
    pub last4: Option<String>,
    pub exp_month: Option<u16>,
    pub exp_year: Option<u16>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    Paid {
        lookup: String,
        reference: String,
        amount: Option<u64>,
        confirmed: bool,
        saved: Option<SavedMethodInfo>,
    },
    Failed {
        lookup: String,
        reason: String,
    },
    Refunded {
        lookup: String,
        external_id: String,
        amount: Option<u64>,
    },
    RefundFailed {
        lookup: String,
        external_id: String,
        reason: String,
    },
    Dispute {
        lookup: String,
        state: DisputeState,
        reason: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisputeState {
    Open,
    Won,
    Lost,
}

impl DisputeState {
    pub fn as_str(self) -> &'static str {
        match self {
            DisputeState::Open => "open",
            DisputeState::Won => "won",
            DisputeState::Lost => "lost",
        }
    }
}

#[async_trait]
pub trait HealthRecorder: Send + Sync {
    async fn provider_call(&self, provider_id: u64, outcome: Result<(), String>);
}

#[derive(Debug, Clone)]
pub struct RefundRequest {
    pub amount: u64,
    pub currency: String,
    pub destination: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RefundOutcome {
    pub external_id: Option<String>,
}

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn event(&self, event: ProviderEvent, cursor: Option<String>) -> Result<()>;
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn method(&self) -> &'static str;

    fn options(&self) -> Vec<String> {
        vec![]
    }

    async fn create(&self, req: AttemptRequest) -> Result<CreatedAttempt>;

    async fn cancel(&self, _attempt: &AttemptRow) -> Result<()> {
        Ok(())
    }

    async fn setup(&self, _hook_url: &str) -> Result<Option<Value>> {
        Ok(None)
    }

    fn watches(&self) -> bool {
        false
    }

    async fn watch(&self, _sink: Arc<dyn EventSink>, _cursor: Option<String>) -> Result<()> {
        futures::future::pending().await
    }

    async fn handle_webhook(
        &self,
        _headers: &HashMap<String, String>,
        _body: &[u8],
    ) -> Result<Vec<ProviderEvent>> {
        bail!("{} does not accept webhooks", self.method())
    }

    async fn reconcile(&self, _attempt: &AttemptRow) -> Result<Option<ProviderEvent>> {
        Ok(None)
    }

    async fn receive_address(&self, _amount_msat: u64) -> Result<String> {
        bail!("{} cannot hand out receive addresses", self.method())
    }

    async fn refund(&self, _attempt: &AttemptRow, _req: RefundRequest) -> Result<RefundOutcome> {
        bail!("{} does not support refunds", self.method())
    }

    async fn charge_saved(
        &self,
        _saved: &SavedMethodRow,
        _req: AttemptRequest,
    ) -> Result<CreatedAttempt> {
        bail!("{} does not support saved payment methods", self.method())
    }

    fn event_id(&self, _body: &[u8]) -> Option<String> {
        None
    }

    async fn test_connection(&self) -> Result<String> {
        bail!("{} does not support connection tests", self.method())
    }

    async fn remove_saved(&self, _saved: &SavedMethodRow) -> Result<()> {
        Ok(())
    }

    async fn list_saved(&self, _customer_id: &str) -> Result<Vec<SavedMethodInfo>> {
        bail!("{} does not list saved payment methods", self.method())
    }

    async fn capture_return(&self, _attempt: &AttemptRow) -> Result<Option<ProviderEvent>> {
        Ok(None)
    }

    async fn wallet_balances(&self) -> Result<Vec<WalletBalance>> {
        Ok(vec![])
    }
}

#[async_trait]
pub trait ProviderFactory: Send + Sync {
    async fn build(&self, row: &ProviderConfigRow, config: Value) -> Result<Arc<dyn Provider>>;
}

pub struct DefaultFactory {
    pub allow_mock: bool,
    pub mocks: Arc<mock::MockFactory>,
}

impl DefaultFactory {
    pub fn new(allow_mock: bool) -> Self {
        Self {
            allow_mock,
            mocks: Arc::new(mock::MockFactory::default()),
        }
    }
}

#[async_trait]
impl ProviderFactory for DefaultFactory {
    async fn build(&self, row: &ProviderConfigRow, config: Value) -> Result<Arc<dyn Provider>> {
        match (row.method.as_str(), row.provider_type.as_str()) {
            (_, "mock") if self.allow_mock => self.mocks.build(row, config).await,
            ("lightning", "lnd") => Ok(Arc::new(lightning::LightningProvider::lnd(&config).await?)),
            ("onchain", "lnd") => Ok(Arc::new(onchain::OnChainProviderImpl::lnd(&config).await?)),
            ("stripe", "stripe") => Ok(Arc::new(stripe::StripeProvider::new(&config)?)),
            ("revolut", "revolut") => Ok(Arc::new(revolut::RevolutProvider::new(&config)?)),
            ("paypal", "paypal") => Ok(Arc::new(paypal::PayPalProvider::new(&config)?)),
            ("fixedfloat", "fixedfloat") => {
                Ok(Arc::new(fixedfloat::FixedFloatProvider::new(&config)?))
            }
            (m, t) => bail!("unsupported provider {m}/{t}"),
        }
    }
}

#[derive(Clone)]
pub struct Registry {
    pool: MySqlPool,
    sealer: Sealer,
    factory: Arc<dyn ProviderFactory>,
    cache: Arc<Mutex<HashMap<u64, Arc<dyn Provider>>>>,
    build_lock: Arc<tokio::sync::Mutex<()>>,
    pub metrics: Arc<crate::metrics::Metrics>,
    pub call_timeout: std::time::Duration,
    pub health: Arc<std::sync::OnceLock<std::sync::Weak<dyn HealthRecorder>>>,
}

impl Registry {
    pub fn new(pool: MySqlPool, sealer: Sealer, factory: Arc<dyn ProviderFactory>) -> Self {
        Self {
            pool,
            sealer,
            factory,
            cache: Default::default(),
            build_lock: Default::default(),
            metrics: Default::default(),
            call_timeout: guard::DEFAULT_CALL_TIMEOUT,
            health: Default::default(),
        }
    }

    pub fn invalidate(&self, id: u64) {
        self.cache.lock().unwrap().remove(&id);
    }

    pub fn open_config(&self, row: &ProviderConfigRow) -> Result<Value> {
        let plain = self.sealer.open(&row.config).context("provider config")?;
        Ok(serde_json::from_slice(&plain)?)
    }

    pub async fn get(&self, id: u64) -> Result<Arc<dyn Provider>> {
        if let Some(p) = self.cache.lock().unwrap().get(&id) {
            return Ok(p.clone());
        }
        let _guard = self.build_lock.lock().await;
        if let Some(p) = self.cache.lock().unwrap().get(&id) {
            return Ok(p.clone());
        }
        if let Some(shared_id) = crate::shared::shared_id_of_key(id) {
            drop(_guard);
            return self.get_shared(id, shared_id).await;
        }
        let row = db::get_provider_config(&self.pool, id)
            .await?
            .context("unknown provider config")?;
        if let Some(shared_id) = row.shared_provider_id {
            drop(_guard);
            let provider = self
                .get_shared(crate::shared::shared_key(shared_id), shared_id)
                .await?;
            self.cache.lock().unwrap().insert(id, provider.clone());
            return Ok(provider);
        }
        let mut config = self.open_config(&row)?;
        let trusted = db::get_tenant(&self.pool, row.tenant_id)
            .await?
            .map(|t| t.trusted)
            .unwrap_or(false);
        if !trusted && let Some(url) = config.get("url").and_then(Value::as_str) {
            let pinned = crate::ssrf::pin_url(&crate::ssrf::SystemResolver, url).await?;
            config["url"] = Value::String(pinned);
        }
        let built = tokio::time::timeout(self.call_timeout, self.factory.build(&row, config))
            .await
            .map_err(|_| anyhow::anyhow!("building provider {id} timed out"))??;
        let provider: Arc<dyn Provider> = Arc::new(
            guard::Guarded::new(built, self.metrics.clone(), self.call_timeout)
                .with_health(id, self.health.clone()),
        );
        self.cache.lock().unwrap().insert(id, provider.clone());
        Ok(provider)
    }

    async fn get_shared(&self, key: u64, shared_id: u64) -> Result<Arc<dyn Provider>> {
        if let Some(p) = self.cache.lock().unwrap().get(&key) {
            return Ok(p.clone());
        }
        let _guard = self.build_lock.lock().await;
        if let Some(p) = self.cache.lock().unwrap().get(&key) {
            return Ok(p.clone());
        }
        let shared = db::get_shared_provider(&self.pool, shared_id)
            .await?
            .context("unknown shared provider")?;
        let plain = self
            .sealer
            .open(&shared.config)
            .context("shared provider config")?;
        let config: Value = serde_json::from_slice(&plain)?;
        let row = ProviderConfigRow {
            id: key,
            tenant_id: 0,
            method: shared.method.clone(),
            provider_type: shared.provider_type.clone(),
            name: shared.name.clone(),
            enabled: shared.enabled,
            config: shared.config.clone(),
            fee_rate: None,
            fee_base: None,
            fee_currency: None,
            shared_provider_id: Some(shared_id),
        };
        let provider = self.factory.build(&row, config).await?;
        self.cache.lock().unwrap().insert(key, provider.clone());
        Ok(provider)
    }

    pub async fn persist_config(&self, id: u64, config: &Value) -> Result<()> {
        let sealed = self.sealer.seal(&serde_json::to_vec(config)?)?;
        db::update_provider_config_blob(&self.pool, id, &sealed).await
    }
}
