use super::source::{BitcoindAccess, ChainPayment, ChainSource};
use crate::xpub::XpubSpec;
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

const HISTORY: u64 = 2000;

pub struct BitcoindSource {
    client: reqwest::Client,
    access: BitcoindAccess,
    wallet: String,
    imported: Mutex<Option<u32>>,
    watched: Mutex<HashSet<String>>,
}

impl BitcoindSource {
    pub fn new(access: BitcoindAccess, provider_id: u64) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()?,
            access,
            wallet: format!("pgw_xpub_{provider_id}"),
            imported: Mutex::new(None),
            watched: Mutex::new(HashSet::new()),
        })
    }

    async fn call(&self, in_wallet: bool, method: &str, params: Value) -> Result<Value> {
        let base = self.access.url.trim_end_matches('/');
        let url = if in_wallet {
            format!("{base}/wallet/{}", self.wallet)
        } else {
            base.to_string()
        };
        let rsp = self
            .client
            .post(url)
            .basic_auth(&self.access.user, Some(&self.access.pass))
            .json(&json!({"jsonrpc": "1.0", "id": "pgw", "method": method, "params": params}))
            .send()
            .await?;
        let status = rsp.status();
        let body: Value = rsp.json().await.unwrap_or(Value::Null);
        if let Some(err) = body.get("error").filter(|e| !e.is_null()) {
            bail!(
                "bitcoind {method}: {}",
                err["message"].as_str().unwrap_or("error")
            );
        }
        if !status.is_success() {
            bail!("bitcoind {method}: http {status}");
        }
        Ok(body["result"].clone())
    }

    async fn ensure_wallet(&self) -> Result<()> {
        let loaded = self.call(false, "listwallets", json!([])).await?;
        if loaded
            .as_array()
            .is_some_and(|w| w.iter().any(|n| n.as_str() == Some(self.wallet.as_str())))
        {
            return Ok(());
        }
        if self
            .call(false, "loadwallet", json!([self.wallet]))
            .await
            .is_ok()
        {
            return Ok(());
        }
        self.call(
            false,
            "createwallet",
            json!({
                "wallet_name": self.wallet,
                "disable_private_keys": true,
                "blank": true,
                "descriptors": true,
                "load_on_startup": true
            }),
        )
        .await?;
        Ok(())
    }

    async fn import(&self, spec: &XpubSpec, upto: u32, timestamp: Value) -> Result<()> {
        let result = self
            .call(
                true,
                "importdescriptors",
                json!([{
                    "desc": spec.descriptor(),
                    "range": [0, upto],
                    "timestamp": timestamp,
                    "active": false,
                    "internal": false
                }]),
            )
            .await?;
        let failed = result
            .as_array()
            .and_then(|a| a.iter().find(|r| r["success"] != true));
        if let Some(f) = failed {
            bail!(
                "bitcoind importdescriptors: {}",
                f["error"]["message"].as_str().unwrap_or("failed")
            );
        }
        Ok(())
    }
}

fn sats(amount: &Value) -> Option<u64> {
    let btc = amount.as_f64()?;
    (btc >= 0.0).then(|| (btc * 100_000_000.0).round() as u64)
}

#[async_trait]
impl ChainSource for BitcoindSource {
    fn name(&self) -> &'static str {
        "bitcoind"
    }

    async fn prepare(&self, spec: &XpubSpec, upto: u32, since: Option<i64>) -> Result<()> {
        let current = *self.imported.lock().unwrap();
        if current.is_some_and(|c| c >= upto) {
            return Ok(());
        }
        self.ensure_wallet().await?;
        let timestamp = match (current, since) {
            (None, Some(t)) => json!(t),
            _ => json!("now"),
        };
        self.import(spec, upto + 20, timestamp).await?;
        *self.imported.lock().unwrap() = Some(upto);
        Ok(())
    }

    async fn payments(&self, addresses: &[String]) -> Result<Vec<ChainPayment>> {
        {
            let mut watched = self.watched.lock().unwrap();
            watched.extend(addresses.iter().cloned());
        }
        let wanted: HashSet<&String> = addresses.iter().collect();
        let rows = self
            .call(true, "listtransactions", json!(["*", HISTORY, 0, true]))
            .await?;
        let mut out = vec![];
        for r in rows.as_array().into_iter().flatten() {
            if r["category"] != "receive" {
                continue;
            }
            let (Some(address), Some(txid), Some(vout), Some(sats)) = (
                r["address"].as_str(),
                r["txid"].as_str(),
                r["vout"].as_u64(),
                sats(&r["amount"]),
            ) else {
                continue;
            };
            if !wanted.contains(&address.to_string()) {
                continue;
            }
            out.push(ChainPayment {
                address: address.to_string(),
                txid: txid.to_string(),
                vout: vout as u32,
                sats,
                confirmations: r["confirmations"].as_i64().unwrap_or(0).max(0) as u32,
            });
        }
        Ok(out)
    }

    async fn rescan(&self, spec: &XpubSpec, from: u32, count: u32) -> Result<()> {
        self.ensure_wallet().await?;
        self.import(spec, from + count, json!(0)).await?;
        *self.imported.lock().unwrap() = Some(from + count);
        Ok(())
    }

    async fn check(&self) -> Result<String> {
        let info = self.call(false, "getblockchaininfo", json!([])).await?;
        Ok(format!(
            "bitcoind reachable, {} height {}",
            info["chain"].as_str().unwrap_or("?"),
            info["blocks"].as_u64().unwrap_or(0)
        ))
    }
}
