use super::source::{ChainPayment, ChainSource};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde::Deserialize;
use std::time::Duration;

pub struct EsploraSource {
    client: reqwest::Client,
    base: String,
}

#[derive(Deserialize)]
struct Tx {
    txid: String,
    vout: Vec<Output>,
    status: Status,
}

#[derive(Deserialize)]
struct Output {
    scriptpubkey_address: Option<String>,
    value: u64,
}

#[derive(Deserialize)]
struct Status {
    confirmed: bool,
    block_height: Option<u64>,
}

impl EsploraSource {
    pub fn new(base: &str, pin: Option<(String, std::net::SocketAddr)>) -> Result<Self> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none());
        if let Some((host, addr)) = pin {
            builder = builder.resolve(&host, addr);
        }
        Ok(Self {
            client: builder.build()?,
            base: base.trim_end_matches('/').to_string(),
        })
    }

    async fn get_text(&self, path: &str) -> Result<String> {
        let rsp = self
            .client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .context("esplora request failed")?;
        let status = rsp.status();
        if !status.is_success() {
            bail!("esplora answered http {status}");
        }
        Ok(rsp.text().await?)
    }

    async fn tip(&self) -> Result<u64> {
        self.get_text("/blocks/tip/height")
            .await?
            .trim()
            .parse()
            .context("esplora returned a bad tip height")
    }
}

#[async_trait]
impl ChainSource for EsploraSource {
    fn name(&self) -> &'static str {
        "esplora"
    }

    async fn payments(&self, addresses: &[String]) -> Result<Vec<ChainPayment>> {
        let mut out = vec![];
        let mut tip = None;
        for address in addresses {
            let body = self.get_text(&format!("/address/{address}/txs")).await?;
            let txs: Vec<Tx> = serde_json::from_str(&body).context("esplora returned bad json")?;
            for tx in txs {
                let confirmations = match (tx.status.confirmed, tx.status.block_height) {
                    (true, Some(height)) => {
                        let tip = match tip {
                            Some(t) => t,
                            None => {
                                let t = self.tip().await?;
                                tip = Some(t);
                                t
                            }
                        };
                        (tip.saturating_sub(height) + 1) as u32
                    }
                    _ => 0,
                };
                for (vout, o) in tx.vout.iter().enumerate() {
                    if o.scriptpubkey_address.as_deref() == Some(address.as_str()) {
                        out.push(ChainPayment {
                            address: address.clone(),
                            txid: tx.txid.clone(),
                            vout: vout as u32,
                            sats: o.value,
                            confirmations,
                        });
                    }
                }
            }
        }
        Ok(out)
    }

    async fn check(&self) -> Result<String> {
        Ok(format!("esplora reachable, tip {}", self.tip().await?))
    }
}
