use crate::xpub::XpubSpec;
use anyhow::{Result, bail};
use async_trait::async_trait;
use bitcoin::Network;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainPayment {
    pub address: String,
    pub txid: String,
    pub vout: u32,
    pub sats: u64,
    pub confirmations: u32,
}

#[async_trait]
pub trait ChainSource: Send + Sync {
    fn name(&self) -> &'static str;

    async fn prepare(&self, _spec: &XpubSpec, _upto: u32, _since: Option<i64>) -> Result<()> {
        Ok(())
    }

    async fn payments(&self, addresses: &[String]) -> Result<Vec<ChainPayment>>;

    async fn rescan(&self, _spec: &XpubSpec, _from: u32, _count: u32) -> Result<()> {
        Ok(())
    }

    async fn check(&self) -> Result<String>;
}

#[derive(Clone, Debug)]
pub struct BitcoindAccess {
    pub url: String,
    pub user: String,
    pub pass: String,
}

#[derive(Clone, Debug)]
pub struct ChainConfig {
    pub network: Network,
    pub esplora_url: Option<String>,
    pub bitcoind: Option<BitcoindAccess>,
    pub poll: std::time::Duration,
    pub grace: std::time::Duration,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self::for_network(Network::Bitcoin)
    }
}

impl ChainConfig {
    pub fn for_network(network: Network) -> Self {
        Self {
            network,
            esplora_url: default_esplora(network).map(str::to_string),
            bitcoind: None,
            poll: std::time::Duration::from_secs(20),
            grace: std::time::Duration::from_secs(48 * 3600),
        }
    }

    pub fn with_esplora(mut self, url: Option<String>) -> Self {
        self.esplora_url = url;
        self
    }

    pub fn with_bitcoind(mut self, access: Option<BitcoindAccess>) -> Self {
        self.bitcoind = access;
        self
    }
}

pub fn default_esplora(network: Network) -> Option<&'static str> {
    match network {
        Network::Bitcoin => Some("https://mempool.space/api"),
        Network::Testnet => Some("https://mempool.space/testnet/api"),
        Network::Signet => Some("https://mempool.space/signet/api"),
        _ => None,
    }
}

pub fn sats_to_msat(sats: u64) -> Result<u64> {
    match sats.checked_mul(1000) {
        Some(v) => Ok(v),
        None => bail!("amount out of range"),
    }
}
