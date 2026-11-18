use anyhow::{Result, anyhow};
use async_trait::async_trait;
use fedimint_tonic_lnd::Client;
use fedimint_tonic_lnd::lnrpc::{ChannelBalanceRequest, WalletBalanceRequest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletBalance {
    pub identity: String,
    pub kind: String,
    pub msat: u64,
}

impl WalletBalance {
    pub fn new(identity: impl Into<String>, kind: &str, msat: u64) -> Self {
        Self {
            identity: identity.into(),
            kind: kind.to_string(),
            msat,
        }
    }
}

#[async_trait]
pub trait WalletSource: Send + Sync {
    async fn balances(&self) -> Result<Vec<WalletBalance>>;
}

pub struct LndWallet {
    client: Client,
    url: String,
}

impl LndWallet {
    pub fn new(client: Client, url: &str) -> Self {
        Self {
            client,
            url: url.to_string(),
        }
    }
}

#[async_trait]
impl WalletSource for LndWallet {
    async fn balances(&self) -> Result<Vec<WalletBalance>> {
        let mut client = self.client.clone();
        let channels = client
            .lightning()
            .channel_balance(ChannelBalanceRequest {})
            .await
            .map_err(|e| anyhow!("channel balance: {}", e.message()))?
            .into_inner();
        let chain = client
            .lightning()
            .wallet_balance(WalletBalanceRequest::default())
            .await
            .map_err(|e| anyhow!("wallet balance: {}", e.message()))?
            .into_inner();
        let local = channels.local_balance.map(|a| a.msat).unwrap_or(0);
        Ok(vec![
            WalletBalance::new(format!("{}#channels", self.url), "lightning", local),
            WalletBalance::new(
                format!("{}#chain", self.url),
                "onchain",
                chain.confirmed_balance.max(0) as u64 * 1000,
            ),
        ])
    }
}
