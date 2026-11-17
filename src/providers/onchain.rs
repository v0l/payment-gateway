use super::{
    AttemptRequest, CreatedAttempt, EventSink, Provider, ProviderEvent, RefundOutcome,
    RefundRequest,
};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use futures::StreamExt;
use payments_rs::currency::CurrencyAmount;
use payments_rs::lightning::setup_crypto_provider;
use payments_rs::onchain::{
    ChainPaymentUpdate, LndAddressType, LndOnChainConfig, LndOnChainProvider, NewAddressRequest,
    OnChainProvider, SendCoinsRequest, SendOutput,
};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

pub struct OnChainProviderImpl {
    chain: Arc<dyn OnChainProvider>,
}

impl OnChainProviderImpl {
    pub fn new(chain: Arc<dyn OnChainProvider>) -> Self {
        Self { chain }
    }

    pub async fn lnd(config: &Value) -> Result<Self> {
        setup_crypto_provider();
        let field = |k: &str| {
            config
                .get(k)
                .and_then(Value::as_str)
                .with_context(|| format!("lnd config needs {k}"))
        };
        let address_type = match config.get("address_type").and_then(Value::as_str) {
            Some("p2wkh") => LndAddressType::WitnessPubkeyHash,
            Some("np2wkh") => LndAddressType::NestedPubkeyHash,
            Some("p2tr") | None => LndAddressType::TaprootPubkey,
            Some(other) => bail!("unknown address_type {other}"),
        };
        let min_confirmations = config
            .get("min_confirmations")
            .and_then(Value::as_u64)
            .unwrap_or(1) as u32;
        let chain = LndOnChainProvider::new(
            field("url")?,
            Path::new(field("cert_path")?),
            Path::new(field("macaroon_path")?),
            LndOnChainConfig {
                address_type,
                account: config
                    .get("account")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                min_confirmations,
            },
        )
        .await?;
        Ok(Self::new(Arc::new(chain)))
    }
}

#[async_trait]
impl Provider for OnChainProviderImpl {
    fn method(&self) -> &'static str {
        "onchain"
    }

    async fn create(&self, req: AttemptRequest) -> Result<CreatedAttempt> {
        let address = self.receive_address(req.amount).await?;
        let sats = req.amount.div_ceil(1000);
        Ok(CreatedAttempt {
            lookup: address.clone(),
            external_id: None,
            details: json!({
                "address": address,
                "amount_sats": sats,
                "uri": format!("bitcoin:{address}?amount={}", sats_to_btc(sats)),
            }),
            expires: None,
            completed: false,
        })
    }

    async fn receive_address(&self, amount_msat: u64) -> Result<String> {
        Ok(self
            .chain
            .new_address(NewAddressRequest {
                amount: CurrencyAmount::millisats(amount_msat),
                memo: None,
                label: None,
            })
            .await?
            .address)
    }

    fn watches(&self) -> bool {
        true
    }

    async fn watch(&self, sink: Arc<dyn EventSink>, _cursor: Option<String>) -> Result<()> {
        let mut stream = self.chain.subscribe_payments(None).await?;
        while let Some(update) = stream.next().await {
            match update {
                ChainPaymentUpdate::Detected {
                    address,
                    txid,
                    vout,
                    amount_msat,
                    ..
                } => {
                    sink.event(
                        ProviderEvent::Paid {
                            lookup: address,
                            reference: format!("{txid}:{vout}"),
                            amount: Some(amount_msat),
                            confirmed: false,
                            saved: None,
                        },
                        None,
                    )
                    .await?
                }
                ChainPaymentUpdate::Confirmed {
                    address,
                    txid,
                    vout,
                    amount_msat,
                    ..
                } => {
                    sink.event(
                        ProviderEvent::Paid {
                            lookup: address,
                            reference: format!("{txid}:{vout}"),
                            amount: Some(amount_msat),
                            confirmed: true,
                            saved: None,
                        },
                        None,
                    )
                    .await?
                }
                ChainPaymentUpdate::Error(e) => bail!("chain stream error: {e}"),
            }
        }
        Err(anyhow!("chain stream ended"))
    }

    async fn refund(
        &self,
        _attempt: &crate::db::AttemptRow,
        req: RefundRequest,
    ) -> Result<RefundOutcome> {
        let address = req
            .destination
            .context("an on-chain address is needed as the refund destination")?;
        let sent = self
            .chain
            .send_coins(SendCoinsRequest {
                outputs: vec![SendOutput {
                    address,
                    amount: CurrencyAmount::millisats(req.amount),
                }],
                sat_per_vbyte: None,
                target_conf: Some(6),
                label: Some("refund".into()),
            })
            .await?;
        Ok(RefundOutcome {
            external_id: Some(sent.txid),
        })
    }
}

fn sats_to_btc(sats: u64) -> String {
    let s = format!("{}.{:08}", sats / 100_000_000, sats % 100_000_000);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn btc_formatting_has_no_float_error() {
        assert_eq!(sats_to_btc(150_000), "0.0015");
        assert_eq!(sats_to_btc(100_000_000), "1");
        assert_eq!(sats_to_btc(1), "0.00000001");
    }
}
