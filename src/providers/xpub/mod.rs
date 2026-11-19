mod bitcoind;
mod esplora;
pub mod source;

pub use bitcoind::BitcoindSource;
pub use esplora::EsploraSource;
pub use source::{BitcoindAccess, ChainConfig, ChainPayment, ChainSource};

use super::{
    AttemptRequest, CreatedAttempt, EventSink, Provider, ProviderEvent, RefundOutcome,
    RefundRequest,
};
use crate::db::{self, ProviderConfigRow};
use crate::xpub::{PREVIEW_COUNT, XpubSpec};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::MySqlPool;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub const MANUAL_REFUND: &str = "manual";

pub struct XpubProvider {
    id: u64,
    pool: MySqlPool,
    spec: XpubSpec,
    source: Arc<dyn ChainSource>,
    min_confirmations: u32,
    chain: ChainConfig,
    sent: Mutex<HashMap<(String, u32), (u64, bool)>>,
}

fn min_confirmations(config: &Value) -> u32 {
    config
        .get("min_confirmations")
        .and_then(Value::as_u64)
        .unwrap_or(1) as u32
}

pub fn parse_spec(config: &Value, chain: &ChainConfig) -> Result<XpubSpec> {
    let key = config
        .get("xpub")
        .and_then(Value::as_str)
        .context("config needs xpub")?;
    XpubSpec::parse(key, chain.network)
}

pub async fn validate_config(
    config: &Value,
    chain: &ChainConfig,
    trusted: bool,
) -> Result<Vec<String>> {
    let spec = parse_spec(config, chain)?;
    if let Some(c) = config.get("min_confirmations")
        && !c.as_u64().is_some_and(|c| (0..=100).contains(&c))
    {
        bail!("min_confirmations must be 0-100");
    }
    match config.get("source").and_then(Value::as_str) {
        None | Some("esplora") | Some("bitcoind") => {}
        Some(_) => bail!("source must be esplora or bitcoind"),
    }
    if let Some(url) = config.get("esplora_url").and_then(Value::as_str) {
        if !trusted {
            crate::ssrf::require_https(url)?;
            crate::ssrf::resolve_public(url).await?;
        } else if !crate::validate::valid_url(url) {
            bail!("esplora_url must be an http(s) url");
        }
    }
    if config.get("source").and_then(Value::as_str) == Some("bitcoind") && chain.bitcoind.is_none()
    {
        bail!("this gateway has no bitcoind configured");
    }
    spec.addresses(0, PREVIEW_COUNT)
}

async fn build_source(
    pool: &MySqlPool,
    row: &ProviderConfigRow,
    config: &Value,
    chain: &ChainConfig,
) -> Result<Arc<dyn ChainSource>> {
    let wanted = config.get("source").and_then(Value::as_str);
    let custom = config
        .get("esplora_url")
        .and_then(Value::as_str)
        .map(str::to_string);
    let url = custom.clone().or_else(|| chain.esplora_url.clone());
    let use_esplora = match wanted {
        Some("esplora") => true,
        Some("bitcoind") => false,
        _ => url.is_some() || chain.bitcoind.is_none(),
    };
    if use_esplora {
        let url = url.context("no esplora url is configured for this network")?;
        let pin = if custom.is_some() {
            let trusted = db::get_tenant(pool, row.tenant_id)
                .await?
                .is_some_and(|t| t.trusted);
            if trusted {
                None
            } else {
                let t = crate::ssrf::resolve_public(&url).await?;
                Some((t.host, t.addr))
            }
        } else {
            None
        };
        return Ok(Arc::new(EsploraSource::new(&url, pin)?));
    }
    let access = chain
        .bitcoind
        .clone()
        .context("this gateway has no bitcoind configured")?;
    Ok(Arc::new(BitcoindSource::new(access, row.id)?))
}

impl XpubProvider {
    pub async fn build(
        pool: MySqlPool,
        row: &ProviderConfigRow,
        config: &Value,
        chain: &ChainConfig,
    ) -> Result<Self> {
        let spec = parse_spec(config, chain)?;
        let source = build_source(&pool, row, config, chain).await?;
        Ok(Self::new(
            row.id,
            pool,
            spec,
            source,
            min_confirmations(config),
            chain.clone(),
        ))
    }

    pub fn new(
        id: u64,
        pool: MySqlPool,
        spec: XpubSpec,
        source: Arc<dyn ChainSource>,
        min_confirmations: u32,
        chain: ChainConfig,
    ) -> Self {
        Self {
            id,
            pool,
            spec,
            source,
            min_confirmations,
            chain,
            sent: Default::default(),
        }
    }

    async fn next_address(&self) -> Result<(u32, String)> {
        let spec = self.spec.clone();
        db::allocate_xpub_index(&self.pool, self.id, Utc::now(), move |i| {
            Ok(spec.address(i)?.to_string())
        })
        .await
    }

    async fn emit(&self, sink: &Arc<dyn EventSink>, payments: Vec<ChainPayment>) -> Result<usize> {
        let mut count = 0;
        for p in payments {
            let confirmed = p.confirmations >= self.min_confirmations.max(1)
                || (self.min_confirmations == 0 && p.confirmations > 0);
            let key = (p.txid.clone(), p.vout);
            let state = (p.sats, confirmed);
            if self.sent.lock().unwrap().get(&key) == Some(&state) {
                continue;
            }
            sink.event(
                ProviderEvent::Paid {
                    lookup: p.address.clone(),
                    reference: format!("{}:{}", p.txid, p.vout),
                    amount: Some(source::sats_to_msat(p.sats)?),
                    confirmed,
                    saved: None,
                },
                None,
            )
            .await?;
            self.sent.lock().unwrap().insert(key, state);
            count += 1;
        }
        Ok(count)
    }

    pub async fn poll_once(&self, sink: &Arc<dyn EventSink>) -> Result<usize> {
        let since = Utc::now() - chrono::Duration::from_std(self.chain.grace)?;
        let next = db::xpub_next_index(&self.pool, self.id).await?;
        self.source
            .prepare(&self.spec, next, Some(since.timestamp()))
            .await?;
        let mut count = 0;
        if let Some((from, n)) = db::take_xpub_rescan(&self.pool, self.id).await? {
            self.source.rescan(&self.spec, from, n).await?;
            let addresses = db::xpub_attempt_addresses(&self.pool, self.id, from, n).await?;
            let payments = self.source.payments(&addresses).await?;
            count += self.emit(sink, payments).await?;
        }
        let addresses = db::xpub_watch_addresses(&self.pool, self.id, since).await?;
        if addresses.is_empty() {
            return Ok(count);
        }
        let payments = self.source.payments(&addresses).await?;
        Ok(count + self.emit(sink, payments).await?)
    }
}

#[async_trait]
impl Provider for XpubProvider {
    fn method(&self) -> &'static str {
        "onchain"
    }

    async fn create(&self, req: AttemptRequest) -> Result<CreatedAttempt> {
        let (index, address) = self.next_address().await?;
        let sats = req.amount.div_ceil(1000);
        Ok(CreatedAttempt {
            lookup: address.clone(),
            external_id: None,
            details: json!({
                "address": address,
                "amount_sats": sats,
                "derivation_index": index,
                "uri": format!("bitcoin:{address}?amount={}", super::onchain::sats_to_btc(sats)),
            }),
            expires: None,
            completed: false,
        })
    }

    async fn receive_address(&self, _amount_msat: u64) -> Result<String> {
        Ok(self.next_address().await?.1)
    }

    fn watches(&self) -> bool {
        true
    }

    async fn watch(&self, sink: Arc<dyn EventSink>, _cursor: Option<String>) -> Result<()> {
        loop {
            self.poll_once(&sink).await?;
            tokio::time::sleep(self.chain.poll).await;
        }
    }

    async fn refund(
        &self,
        _attempt: &crate::db::AttemptRow,
        req: RefundRequest,
    ) -> Result<RefundOutcome> {
        if let Some(d) = req.destination.as_deref().filter(|d| !d.trim().is_empty()) {
            log::info!("manual on-chain refund to {}", crate::logsafe::clean(d));
        }
        Ok(RefundOutcome {
            external_id: Some(MANUAL_REFUND.to_string()),
        })
    }

    async fn test_connection(&self) -> Result<String> {
        let chain = self.source.check().await?;
        let preview = self.spec.addresses(0, 1)?;
        Ok(format!("{chain}. First address {}", preview[0]))
    }

    async fn describe(&self) -> Result<Option<Value>> {
        let next = db::xpub_next_index(&self.pool, self.id).await?;
        let chain = match self.source.check().await {
            Ok(m) => json!({"ok": true, "message": m}),
            Err(e) => json!({"ok": false, "message": crate::logsafe::clean(&format!("{e:#}"))}),
        };
        Ok(Some(json!({
            "kind": "xpub",
            "network": self.spec.network.to_string(),
            "script": self.spec.kind.name(),
            "next_index": next,
            "preview": self.spec.addresses(0, PREVIEW_COUNT)?,
            "next_address": self.spec.address(next)?.to_string(),
            "source": self.source.name(),
            "min_confirmations": self.min_confirmations,
            "chain": chain,
        })))
    }

    async fn rescan(&self, from: u32, count: u32) -> Result<String> {
        let next = db::xpub_next_index(&self.pool, self.id).await?;
        if count == 0 || count > 1000 || from >= next.max(1) {
            bail!("range must start below the next index {next} and cover 1-1000 addresses");
        }
        db::request_xpub_rescan(&self.pool, self.id, from, count).await?;
        Ok(format!(
            "rescan of {count} addresses from index {from} queued"
        ))
    }
}
