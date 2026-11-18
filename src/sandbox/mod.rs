pub mod bitcoind;
pub mod lnd;

use crate::db::{self, NewProviderConfig, PaymentId, Tenant};
use crate::model::{CreateAttempt, CreatePayment};
use crate::service::{Error, Service, webhook_signature};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const TENANT_NAME: &str = "sandbox";
const MIN_NODE_FUNDS: u64 = 2_000_000;
const NODE_FUNDING: u64 = 10_000_000;
const CHANNEL_SATS: u64 = 5_000_000;
const CHANNEL_PUSH_SATS: u64 = 1_000_000;
const MAX_HOOKS: usize = 50;

#[async_trait]
pub trait Bitcoind: Send + Sync {
    async fn ensure_wallet(&self) -> Result<()>;
    async fn new_address(&self) -> Result<String>;
    async fn mine(&self, blocks: u32, to: &str) -> Result<u64>;
    async fn send(&self, address: &str, sats: u64) -> Result<String>;
    async fn height(&self) -> Result<u64>;
    async fn balance_sats(&self) -> Result<u64>;
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct NodeInfo {
    pub pubkey: String,
    pub synced: bool,
    pub height: u64,
    pub active_channels: u32,
    pub onchain_sats: u64,
    pub unconfirmed_sats: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ChannelInfo {
    pub remote_pubkey: String,
    pub capacity_sats: u64,
    pub local_sats: u64,
    pub remote_sats: u64,
    pub active: bool,
}

#[async_trait]
pub trait LnNode: Send + Sync {
    async fn info(&self) -> Result<NodeInfo>;
    async fn new_address(&self) -> Result<String>;
    async fn connect(&self, pubkey: &str, host: &str) -> Result<()>;
    async fn open_channel(&self, pubkey: &str, sats: u64, push_sats: u64) -> Result<()>;
    async fn channels(&self) -> Result<Vec<ChannelInfo>>;
    async fn pay_invoice(&self, bolt11: &str) -> Result<String>;
    async fn send_coins(&self, address: &str, sats: u64) -> Result<String>;
}

#[derive(Clone)]
pub struct LndAccess {
    pub url: String,
    pub cert_path: String,
    pub macaroon_path: String,
}

#[derive(Clone)]
pub struct SandboxConfig {
    pub bitcoind_url: String,
    pub bitcoind_user: String,
    pub bitcoind_pass: String,
    pub gateway: LndAccess,
    pub customer: LndAccess,
    pub gateway_p2p: String,
}

impl SandboxConfig {
    pub fn from_env() -> Result<Option<Self>> {
        let Ok(bitcoind_url) = std::env::var("SANDBOX_BITCOIND_URL") else {
            return Ok(None);
        };
        let get = |k: &str| std::env::var(k).with_context(|| k.to_string());
        Ok(Some(Self {
            bitcoind_url,
            bitcoind_user: get("SANDBOX_BITCOIND_USER")?,
            bitcoind_pass: get("SANDBOX_BITCOIND_PASS")?,
            gateway: LndAccess {
                url: get("SANDBOX_GATEWAY_LND_URL")?,
                cert_path: get("SANDBOX_GATEWAY_LND_CERT")?,
                macaroon_path: get("SANDBOX_GATEWAY_LND_MACAROON")?,
            },
            customer: LndAccess {
                url: get("SANDBOX_CUSTOMER_LND_URL")?,
                cert_path: get("SANDBOX_CUSTOMER_LND_CERT")?,
                macaroon_path: get("SANDBOX_CUSTOMER_LND_MACAROON")?,
            },
            gateway_p2p: get("SANDBOX_GATEWAY_P2P")?,
        }))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReceivedHook {
    pub received: DateTime<Utc>,
    pub signature_valid: bool,
    pub event_type: String,
    pub event_id: String,
    pub payment_id: String,
    pub status: String,
    pub body: String,
}

#[derive(Debug, Serialize)]
pub struct Part<T> {
    pub ok: bool,
    pub value: Option<T>,
    pub error: Option<String>,
}

impl<T> Part<T> {
    fn of(r: Result<T>) -> Self {
        match r {
            Ok(v) => Part {
                ok: true,
                value: Some(v),
                error: None,
            },
            Err(e) => Part {
                ok: false,
                value: None,
                error: Some(format!("{e:#}")),
            },
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ChainStatus {
    pub height: u64,
    pub miner_sats: u64,
}

#[derive(Debug, Serialize)]
pub struct TenantStatus {
    pub id: u64,
    pub providers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct Status {
    pub chain: Part<ChainStatus>,
    pub gateway: Part<NodeInfo>,
    pub customer: Part<NodeInfo>,
    pub channels: Part<Vec<ChannelInfo>>,
    pub tenant: Option<TenantStatus>,
    pub ready: bool,
}

pub struct Sandbox {
    pub btc: Arc<dyn Bitcoind>,
    pub gateway: Arc<dyn LnNode>,
    pub customer: Arc<dyn LnNode>,
    pub gateway_access: LndAccess,
    pub gateway_p2p: String,
    pub service: Arc<Service>,
    pub poll: Duration,
    pub polls: u32,
    hooks: Mutex<VecDeque<ReceivedHook>>,
}

impl Sandbox {
    pub fn from_config(cfg: SandboxConfig, service: Arc<Service>) -> Result<Arc<Self>> {
        Ok(Arc::new(Self::new(
            Arc::new(bitcoind::BitcoindRpc::new(
                &cfg.bitcoind_url,
                &cfg.bitcoind_user,
                &cfg.bitcoind_pass,
                "sandbox",
            )?),
            Arc::new(lnd::LndRpc::new(
                &cfg.gateway.url,
                &cfg.gateway.cert_path,
                &cfg.gateway.macaroon_path,
            )),
            Arc::new(lnd::LndRpc::new(
                &cfg.customer.url,
                &cfg.customer.cert_path,
                &cfg.customer.macaroon_path,
            )),
            cfg.gateway,
            cfg.gateway_p2p,
            service,
        )))
    }

    pub fn new(
        btc: Arc<dyn Bitcoind>,
        gateway: Arc<dyn LnNode>,
        customer: Arc<dyn LnNode>,
        gateway_access: LndAccess,
        gateway_p2p: String,
        service: Arc<Service>,
    ) -> Self {
        Self {
            btc,
            gateway,
            customer,
            gateway_access,
            gateway_p2p,
            service,
            poll: Duration::from_secs(1),
            polls: 60,
            hooks: Default::default(),
        }
    }

    pub async fn tenant(&self) -> Result<Option<Tenant>> {
        db::tenant_by_name(&self.service.pool, TENANT_NAME).await
    }

    pub async fn is_sandbox_tenant(&self, tenant_id: u64) -> bool {
        matches!(self.tenant().await, Ok(Some(t)) if t.id == tenant_id)
    }

    pub async fn status(&self) -> Status {
        let chain = async {
            Ok(ChainStatus {
                height: self.btc.height().await?,
                miner_sats: self.btc.balance_sats().await?,
            })
        }
        .await;
        let gateway = self.gateway.info().await;
        let customer = self.customer.info().await;
        let channels = self.customer.channels().await;
        let tenant = match self.tenant().await {
            Ok(Some(t)) => {
                let providers = db::list_provider_configs(&self.service.pool, t.id, true)
                    .await
                    .map(|p| p.into_iter().map(|p| p.method).collect())
                    .unwrap_or_default();
                Some(TenantStatus {
                    id: t.id,
                    providers,
                })
            }
            _ => None,
        };
        let channel_ok = channels
            .as_ref()
            .map(|c| c.iter().any(|c| c.active))
            .unwrap_or(false);
        let ready = chain.is_ok()
            && gateway.as_ref().is_ok_and(|n| n.synced)
            && customer.as_ref().is_ok_and(|n| n.synced)
            && channel_ok
            && tenant.as_ref().is_some_and(|t| {
                t.providers.iter().any(|p| p == "lightning")
                    && t.providers.iter().any(|p| p == "onchain")
            });
        Status {
            chain: Part::of(chain),
            gateway: Part::of(gateway),
            customer: Part::of(customer),
            channels: Part::of(channels),
            tenant,
            ready,
        }
    }

    async fn wait<F, Fut>(&self, what: &str, mut check: F) -> Result<()>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<bool>>,
    {
        for _ in 0..self.polls {
            if check().await.unwrap_or(false) {
                return Ok(());
            }
            tokio::time::sleep(self.poll).await;
        }
        bail!("timed out waiting for {what}")
    }

    pub async fn miner_address(&self) -> Result<String> {
        self.btc.new_address().await
    }

    pub async fn customer_channels(&self) -> Result<Vec<ChannelInfo>> {
        self.customer.channels().await
    }

    pub async fn mine(&self, blocks: u32) -> Result<u64> {
        if blocks == 0 || blocks > 200 {
            bail!("blocks must be 1-200");
        }
        let to = self.miner_address().await?;
        self.btc.mine(blocks, &to).await
    }

    pub async fn bootstrap(&self) -> Result<Vec<String>> {
        let mut log = vec![];
        self.btc.ensure_wallet().await?;
        log.push("bitcoind wallet ready".to_string());

        if self.btc.balance_sats().await? < NODE_FUNDING * 3 {
            let height = self.mine(101).await?;
            log.push(format!(
                "mined 101 blocks for coinbase maturity (height {height})"
            ));
        }

        for (name, node) in [("gateway", &self.gateway), ("customer", &self.customer)] {
            self.wait(&format!("{name} lnd"), || async {
                Ok(node.info().await.is_ok())
            })
            .await?;
            let info = node.info().await?;
            if info.onchain_sats + info.unconfirmed_sats < MIN_NODE_FUNDS {
                let addr = node.new_address().await?;
                let txid = self.btc.send(&addr, NODE_FUNDING).await?;
                log.push(format!(
                    "funded {name} wallet with {NODE_FUNDING} sats ({txid})"
                ));
                self.mine(3).await?;
            }
        }

        for (name, node) in [("gateway", &self.gateway), ("customer", &self.customer)] {
            self.wait(&format!("{name} chain sync"), || async {
                let info = node.info().await?;
                Ok(info.synced && info.onchain_sats >= MIN_NODE_FUNDS)
            })
            .await?;
        }
        log.push("both nodes synced and funded".to_string());

        let gateway_key = self.gateway.info().await?.pubkey;
        self.customer
            .connect(&gateway_key, &self.gateway_p2p)
            .await?;
        log.push("customer connected to gateway".to_string());

        let has_channel = self
            .customer
            .channels()
            .await?
            .iter()
            .any(|c| c.remote_pubkey == gateway_key);
        if !has_channel {
            self.customer
                .open_channel(&gateway_key, CHANNEL_SATS, CHANNEL_PUSH_SATS)
                .await?;
            log.push(format!(
                "opened {CHANNEL_SATS} sat channel (pushed {CHANNEL_PUSH_SATS} to the gateway)"
            ));
            self.mine(6).await?;
        }
        self.wait("channel to become active", || async {
            Ok(self
                .customer
                .channels()
                .await?
                .iter()
                .any(|c| c.remote_pubkey == gateway_key && c.active))
        })
        .await?;
        log.push("channel active".to_string());

        let created = self.ensure_tenant().await?;
        match created {
            Some(key) => log.push(format!(
                "created tenant '{TENANT_NAME}', api key {key} (shown once)"
            )),
            None => log.push(format!("tenant '{TENANT_NAME}' already exists")),
        }
        log.extend(self.ensure_providers().await?);
        Ok(log)
    }

    pub async fn ensure_tenant(&self) -> Result<Option<String>> {
        if self.tenant().await?.is_some() {
            return Ok(None);
        }
        let key = crate::crypto::new_api_key();
        let secret = crate::crypto::new_webhook_secret();
        let sealed = self.service.sealer.seal(secret.as_bytes())?;
        let hook = format!("{}/sandbox/hook", self.service.public_url);
        db::insert_tenant(
            &self.service.pool,
            TENANT_NAME,
            &crate::crypto::hash_api_key(&key),
            Some(&hook),
            &sealed,
        )
        .await?;
        Ok(Some(key))
    }

    pub async fn rotate_key(&self) -> Result<String> {
        let tenant = self
            .tenant()
            .await?
            .ok_or_else(|| anyhow!("run bootstrap first"))?;
        let key = crate::crypto::new_api_key();
        db::rotate_tenant_key(
            &self.service.pool,
            tenant.id,
            &crate::crypto::hash_api_key(&key),
        )
        .await?;
        Ok(key)
    }

    pub async fn ensure_providers(&self) -> Result<Vec<String>> {
        let tenant = self
            .tenant()
            .await?
            .ok_or_else(|| anyhow!("tenant missing"))?;
        let existing = db::list_provider_configs(&self.service.pool, tenant.id, false).await?;
        let mut log = vec![];
        for (method, name, extra) in [
            ("lightning", "regtest-lightning", json!({})),
            (
                "onchain",
                "regtest-onchain",
                json!({"address_type": "p2wkh", "min_confirmations": 1}),
            ),
        ] {
            if existing.iter().any(|p| p.method == method) {
                continue;
            }
            let mut config = json!({
                "url": self.gateway_access.url,
                "cert_path": self.gateway_access.cert_path,
                "macaroon_path": self.gateway_access.macaroon_path,
            });
            config
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let sealed = self.service.sealer.seal(&serde_json::to_vec(&config)?)?;
            db::insert_provider_config(
                &self.service.pool,
                NewProviderConfig {
                    tenant_id: tenant.id,
                    method,
                    provider_type: "lnd",
                    name,
                    sealed_config: &sealed,
                    fee_rate: None,
                    fee_base: None,
                    fee_currency: None,
                },
            )
            .await?;
            log.push(format!("added {method} provider backed by the gateway LND"));
        }
        Ok(log)
    }

    pub async fn fund(&self, target: &str, sats: u64) -> Result<String> {
        if sats == 0 || sats > 100_000_000 {
            bail!("sats must be 1-100000000");
        }
        let node = match target {
            "gateway" => &self.gateway,
            "customer" => &self.customer,
            other => bail!("unknown target {other}"),
        };
        let addr = node.new_address().await?;
        let txid = self.btc.send(&addr, sats).await?;
        self.mine(1).await?;
        Ok(txid)
    }

    pub async fn faucet(&self, address: &str, sats: u64) -> Result<String> {
        if address.trim().is_empty() {
            bail!("address required");
        }
        if sats == 0 || sats > 100_000_000 {
            bail!("sats must be 1-100000000");
        }
        let txid = self.btc.send(address.trim(), sats).await?;
        self.mine(1).await?;
        Ok(txid)
    }

    pub async fn customer_pay_invoice(&self, bolt11: &str) -> Result<String> {
        self.customer.pay_invoice(bolt11).await
    }

    pub async fn create_payment(&self, input: CreatePayment) -> std::result::Result<Value, Error> {
        let tenant = self
            .tenant()
            .await?
            .ok_or_else(|| Error::Invalid("run bootstrap first".into()))?;
        let (row, _) = self.service.create_payment(&tenant, input, None).await?;
        serde_json::to_value(self.service.view(&row).await?).map_err(|e| Error::Internal(e.into()))
    }

    pub async fn recent_payments(&self) -> Result<Vec<Value>> {
        let Some(tenant) = self.tenant().await? else {
            return Ok(vec![]);
        };
        let mut out = vec![];
        for row in db::recent_payments(&self.service.pool, tenant.id, 25).await? {
            let view = self
                .service
                .view(&row)
                .await
                .map_err(|e| anyhow!("{e:?}"))?;
            out.push(serde_json::to_value(view)?);
        }
        Ok(out)
    }

    pub async fn pay_from_customer(
        &self,
        payment_id: &PaymentId,
    ) -> std::result::Result<String, Error> {
        let tenant = self
            .tenant()
            .await?
            .ok_or_else(|| Error::Invalid("run bootstrap first".into()))?;
        let payment = self.service.payment_for_tenant(&tenant, payment_id).await?;
        if payment.status != "pending" {
            return Err(Error::Conflict(format!("payment is {}", payment.status)));
        }
        let attempts = db::attempts_for_payment(&self.service.pool, payment_id).await?;
        let Some(attempt) = attempts.iter().rev().find(|a| {
            a.status == "pending" && matches!(a.method.as_str(), "lightning" | "onchain")
        }) else {
            return Err(Error::Invalid(
                "choose lightning or onchain on the payment first".into(),
            ));
        };
        let details: Value = serde_json::from_str(&attempt.details).unwrap_or(Value::Null);
        match attempt.method.as_str() {
            "lightning" => {
                let bolt11 = details["bolt11"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("attempt has no invoice".into()))?;
                let preimage = self
                    .customer
                    .pay_invoice(bolt11)
                    .await
                    .map_err(|e| Error::Upstream(format!("{e:#}")))?;
                Ok(format!(
                    "paid invoice from the customer wallet (preimage {preimage})"
                ))
            }
            _ => {
                let address = details["address"]
                    .as_str()
                    .ok_or_else(|| Error::Invalid("attempt has no address".into()))?;
                let sats = details["amount_sats"]
                    .as_u64()
                    .ok_or_else(|| Error::Invalid("attempt has no amount".into()))?;
                let txid = self
                    .customer
                    .send_coins(address, sats)
                    .await
                    .map_err(|e| Error::Upstream(format!("{e:#}")))?;
                self.mine(1)
                    .await
                    .map_err(|e| Error::Upstream(format!("{e:#}")))?;
                Ok(format!(
                    "sent {sats} sats on-chain ({txid}) and mined a block"
                ))
            }
        }
    }

    pub async fn start_attempt(
        &self,
        payment_id: &PaymentId,
        method: &str,
    ) -> std::result::Result<(), Error> {
        let tenant = self
            .tenant()
            .await?
            .ok_or_else(|| Error::Invalid("run bootstrap first".into()))?;
        self.service
            .create_attempt(
                &tenant,
                payment_id,
                CreateAttempt {
                    method: method.to_string(),
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    pub async fn record_hook(&self, signature: Option<&str>, body: &str) {
        let valid = match self.tenant().await {
            Ok(Some(t)) => self.verify(&t, signature, body),
            _ => false,
        };
        let parsed: Value = serde_json::from_str(body).unwrap_or(Value::Null);
        let hook = ReceivedHook {
            received: Utc::now(),
            signature_valid: valid,
            event_type: parsed["type"].as_str().unwrap_or("?").to_string(),
            event_id: parsed["id"].as_str().unwrap_or("?").to_string(),
            payment_id: parsed["data"]["payment"]["id"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            status: parsed["data"]["payment"]["status"]
                .as_str()
                .unwrap_or("")
                .to_string(),
            body: body.chars().take(8192).collect(),
        };
        let mut hooks = self.hooks.lock().unwrap();
        hooks.push_front(hook);
        hooks.truncate(MAX_HOOKS);
    }

    fn verify(&self, tenant: &Tenant, signature: Option<&str>, body: &str) -> bool {
        let Some(sig) = signature else { return false };
        let Some((ts, v1)) = sig.strip_prefix("t=").and_then(|s| s.split_once(",v1=")) else {
            return false;
        };
        let Ok(ts) = ts.parse::<i64>() else {
            return false;
        };
        if (Utc::now().timestamp() - ts).abs() > 300 {
            return false;
        }
        let Ok(secret) = self.service.sealer.open(&tenant.webhook_secret) else {
            return false;
        };
        let Ok(secret) = String::from_utf8(secret) else {
            return false;
        };
        let expected = webhook_signature(&secret, ts, body);
        subtle::ConstantTimeEq::ct_eq(expected.as_bytes(), v1.as_bytes()).into()
    }

    pub fn hooks(&self) -> Vec<ReceivedHook> {
        self.hooks.lock().unwrap().iter().cloned().collect()
    }
}
