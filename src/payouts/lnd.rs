use super::{OnchainState, PaymentState, PayoutNode};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use fedimint_tonic_lnd::Client;
use fedimint_tonic_lnd::lnrpc::{self, payment::PaymentStatus};
use fedimint_tonic_lnd::routerrpc::{SendPaymentRequest, TrackPaymentRequest};
use payments_rs::lightning::{LndNode, setup_crypto_provider};
use std::path::Path;

pub struct LndPayoutNode {
    client: Client,
    sat_per_vbyte: u64,
    timeout_seconds: i32,
}

impl LndPayoutNode {
    pub async fn connect(
        url: &str,
        cert: &Path,
        macaroon: &Path,
        sat_per_vbyte: u64,
    ) -> Result<Self> {
        setup_crypto_provider();
        let node = LndNode::new(url, cert, macaroon)
            .await
            .with_context(|| format!("connecting to {url}"))?;
        Ok(Self {
            client: node.client(),
            sat_per_vbyte,
            timeout_seconds: 60,
        })
    }
}

fn state_of(p: lnrpc::Payment) -> PaymentState {
    match PaymentStatus::try_from(p.status) {
        Ok(PaymentStatus::Succeeded) => PaymentState::Succeeded {
            fee_msat: p.fee_msat.max(0) as u64,
            reference: p.payment_preimage,
        },
        Ok(PaymentStatus::Failed) => {
            PaymentState::Failed(p.failure_reason().as_str_name().to_string())
        }
        _ => PaymentState::InFlight,
    }
}

#[async_trait]
impl PayoutNode for LndPayoutNode {
    async fn pay_invoice(&self, invoice: &str, fee_limit_msat: u64) -> Result<()> {
        let mut client = self.client.clone();
        let mut stream = client
            .router()
            .send_payment_v2(SendPaymentRequest {
                payment_request: invoice.trim().trim_start_matches("lightning:").to_string(),
                timeout_seconds: self.timeout_seconds,
                fee_limit_msat: fee_limit_msat as i64,
                no_inflight_updates: true,
                ..Default::default()
            })
            .await
            .map_err(|e| anyhow!("send payment: {}", e.message()))?
            .into_inner();
        while stream
            .message()
            .await
            .map_err(|e| anyhow!("payment stream: {}", e.message()))?
            .is_some()
        {}
        Ok(())
    }

    async fn payment_state(&self, payment_hash: &str) -> Result<PaymentState> {
        let mut client = self.client.clone();
        let res = client
            .router()
            .track_payment_v2(TrackPaymentRequest {
                payment_hash: hex::decode(payment_hash)?,
                no_inflight_updates: false,
            })
            .await;
        let mut stream = match res {
            Ok(r) => r.into_inner(),
            Err(e) if e.code() == fedimint_tonic_lnd::tonic::Code::NotFound => {
                return Ok(PaymentState::NotFound);
            }
            Err(e) => return Err(anyhow!("track payment: {}", e.message())),
        };
        match stream.message().await {
            Ok(Some(p)) => Ok(state_of(p)),
            Ok(None) => Ok(PaymentState::NotFound),
            Err(e) if e.code() == fedimint_tonic_lnd::tonic::Code::NotFound => {
                Ok(PaymentState::NotFound)
            }
            Err(e) => Err(anyhow!("track payment: {}", e.message())),
        }
    }

    async fn send_coins(&self, address: &str, sats: u64, label: &str) -> Result<()> {
        let mut client = self.client.clone();
        client
            .lightning()
            .send_coins(lnrpc::SendCoinsRequest {
                addr: address.to_string(),
                amount: sats as i64,
                sat_per_vbyte: self.sat_per_vbyte,
                label: label.to_string(),
                ..Default::default()
            })
            .await
            .map_err(|e| anyhow!("send coins: {}", e.message()))?;
        Ok(())
    }

    async fn onchain_state(&self, label: &str) -> Result<OnchainState> {
        let mut client = self.client.clone();
        let txs = client
            .lightning()
            .get_transactions(lnrpc::GetTransactionsRequest {
                start_height: 0,
                end_height: -1,
                ..Default::default()
            })
            .await
            .map_err(|e| anyhow!("get transactions: {}", e.message()))?
            .into_inner();
        Ok(txs
            .transactions
            .into_iter()
            .find(|t| t.label == label)
            .map(|t| OnchainState::Sent {
                txid: t.tx_hash,
                fee_sat: t.total_fees.max(0) as u64,
            })
            .unwrap_or(OnchainState::NotFound))
    }
}
