use super::{ChannelInfo, LnNode, NodeInfo};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use fedimint_tonic_lnd::Client;
use fedimint_tonic_lnd::lnrpc;
use payments_rs::lightning::{LightningNode, LndNode, PayInvoiceRequest, setup_crypto_provider};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

const FEE_RATE: u64 = 2;

pub struct LndRpc {
    url: String,
    cert: PathBuf,
    macaroon: PathBuf,
    node: Mutex<Option<Arc<LndNode>>>,
}

impl LndRpc {
    pub fn new(url: &str, cert: &str, macaroon: &str) -> Self {
        Self {
            url: url.to_string(),
            cert: cert.into(),
            macaroon: macaroon.into(),
            node: Mutex::new(None),
        }
    }

    async fn node(&self) -> Result<Arc<LndNode>> {
        let mut guard = self.node.lock().await;
        if let Some(n) = guard.as_ref() {
            return Ok(n.clone());
        }
        setup_crypto_provider();
        let node = Arc::new(
            LndNode::new(&self.url, &self.cert, &self.macaroon)
                .await
                .with_context(|| format!("connecting to {}", self.url))?,
        );
        *guard = Some(node.clone());
        Ok(node)
    }

    async fn client(&self) -> Result<Client> {
        Ok(self.node().await?.client())
    }

    async fn forget(&self) {
        *self.node.lock().await = None;
    }

    async fn with_client<T, F, Fut>(&self, f: F) -> Result<T>
    where
        F: FnOnce(Client) -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let client = self.client().await?;
        let out = f(client).await;
        if out.is_err() {
            self.forget().await;
        }
        out
    }
}

#[async_trait]
impl LnNode for LndRpc {
    async fn info(&self) -> Result<NodeInfo> {
        self.with_client(|mut c| async move {
            let info = c
                .lightning()
                .get_info(lnrpc::GetInfoRequest {})
                .await?
                .into_inner();
            let bal = c
                .lightning()
                .wallet_balance(lnrpc::WalletBalanceRequest::default())
                .await?
                .into_inner();
            Ok(NodeInfo {
                pubkey: info.identity_pubkey,
                synced: info.synced_to_chain,
                height: info.block_height as u64,
                active_channels: info.num_active_channels,
                onchain_sats: bal.confirmed_balance.max(0) as u64,
                unconfirmed_sats: bal.unconfirmed_balance.max(0) as u64,
            })
        })
        .await
    }

    async fn new_address(&self) -> Result<String> {
        self.with_client(|mut c| async move {
            let a = c
                .lightning()
                .new_address(lnrpc::NewAddressRequest {
                    r#type: 0,
                    account: String::new(),
                })
                .await?
                .into_inner();
            Ok(a.address)
        })
        .await
    }

    async fn connect(&self, pubkey: &str, host: &str) -> Result<()> {
        let (pubkey, host) = (pubkey.to_string(), host.to_string());
        self.with_client(|mut c| async move {
            let res = c
                .lightning()
                .connect_peer(lnrpc::ConnectPeerRequest {
                    addr: Some(lnrpc::LightningAddress { pubkey, host }),
                    perm: false,
                    timeout: 15,
                })
                .await;
            match res {
                Ok(_) => Ok(()),
                Err(e) if e.message().contains("already connected") => Ok(()),
                Err(e) => bail!("connect peer: {}", e.message()),
            }
        })
        .await
    }

    async fn open_channel(&self, pubkey: &str, sats: u64, push_sats: u64) -> Result<()> {
        let node_pubkey = hex::decode(pubkey)?;
        self.with_client(|mut c| async move {
            c.lightning()
                .open_channel_sync(lnrpc::OpenChannelRequest {
                    node_pubkey,
                    local_funding_amount: sats as i64,
                    push_sat: push_sats as i64,
                    sat_per_vbyte: FEE_RATE,
                    min_confs: 1,
                    ..Default::default()
                })
                .await
                .map_err(|e| anyhow::anyhow!("open channel: {}", e.message()))?;
            Ok(())
        })
        .await
    }

    async fn channels(&self) -> Result<Vec<ChannelInfo>> {
        self.with_client(|mut c| async move {
            let list = c
                .lightning()
                .list_channels(lnrpc::ListChannelsRequest::default())
                .await?
                .into_inner();
            Ok(list
                .channels
                .into_iter()
                .map(|ch| ChannelInfo {
                    remote_pubkey: ch.remote_pubkey,
                    capacity_sats: ch.capacity.max(0) as u64,
                    local_sats: ch.local_balance.max(0) as u64,
                    remote_sats: ch.remote_balance.max(0) as u64,
                    active: ch.active,
                })
                .collect())
        })
        .await
    }

    async fn can_route(&self, pubkey: &str, sats: u64) -> Result<bool> {
        let pub_key = pubkey.to_string();
        let mut c = self.client().await?;
        let found = c
            .lightning()
            .query_routes(lnrpc::QueryRoutesRequest {
                pub_key,
                amt: sats as i64,
                ..Default::default()
            })
            .await
            .is_ok_and(|r| !r.into_inner().routes.is_empty());
        Ok(found)
    }

    async fn pay_invoice(&self, bolt11: &str) -> Result<String> {
        let invoice = bolt11.trim().trim_start_matches("lightning:").to_string();
        let node = self.node().await?;
        let paid = node
            .pay_invoice(PayInvoiceRequest {
                invoice,
                timeout_seconds: Some(30),
            })
            .await;
        if paid.is_err() {
            self.forget().await;
        }
        Ok(paid?.payment_preimage.unwrap_or_default())
    }

    async fn send_coins(&self, address: &str, sats: u64) -> Result<String> {
        let addr = address.to_string();
        self.with_client(|mut c| async move {
            let rsp = c
                .lightning()
                .send_coins(lnrpc::SendCoinsRequest {
                    addr,
                    amount: sats as i64,
                    sat_per_vbyte: FEE_RATE,
                    ..Default::default()
                })
                .await
                .map_err(|e| anyhow::anyhow!("send coins: {}", e.message()))?
                .into_inner();
            Ok(rsp.txid)
        })
        .await
    }
}
