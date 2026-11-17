use super::{
    AttemptRequest, CreatedAttempt, EventSink, Provider, ProviderEvent, RefundOutcome,
    RefundRequest,
};
use crate::db::AttemptRow;
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use futures::StreamExt;
use payments_rs::lightning::{
    AddInvoiceRequest, AddInvoiceResponse, InvoiceUpdate, LightningNode, LndNode,
    PayInvoiceRequest, setup_crypto_provider,
};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::Arc;

pub struct LightningProvider {
    node: Arc<dyn LightningNode>,
}

impl LightningProvider {
    pub fn new(node: Arc<dyn LightningNode>) -> Self {
        Self { node }
    }

    pub async fn lnd(config: &Value) -> Result<Self> {
        setup_crypto_provider();
        let field = |k: &str| {
            config
                .get(k)
                .and_then(Value::as_str)
                .with_context(|| format!("lnd config needs {k}"))
        };
        let node = LndNode::new(
            field("url")?,
            Path::new(field("cert_path")?),
            Path::new(field("macaroon_path")?),
        )
        .await?;
        Ok(Self::new(Arc::new(node)))
    }
}

pub fn invoice_amount_msat(invoice: &str) -> Result<Option<u64>> {
    let parsed = AddInvoiceResponse::from_invoice(invoice, None)?;
    Ok(parsed.parsed_invoice.amount_milli_satoshis())
}

#[async_trait]
impl Provider for LightningProvider {
    fn method(&self) -> &'static str {
        "lightning"
    }

    async fn create(&self, req: AttemptRequest) -> Result<CreatedAttempt> {
        let expire = (req.expires - chrono::Utc::now())
            .num_seconds()
            .clamp(60, u32::MAX as i64) as u32;
        let invoice = self
            .node
            .add_invoice(AddInvoiceRequest {
                amount: req.amount,
                memo: Some(req.description.clone()),
                expire: Some(expire),
            })
            .await?;
        let hash = invoice.payment_hash();
        Ok(CreatedAttempt {
            lookup: hash.clone(),
            external_id: invoice.external_id.clone(),
            details: json!({
                "bolt11": invoice.pr(),
                "payment_hash": hash,
                "amount_msat": req.amount,
            }),
            expires: None,
            completed: false,
        })
    }

    async fn cancel(&self, attempt: &AttemptRow) -> Result<()> {
        self.node
            .cancel_invoice(&hex::decode(&attempt.lookup)?)
            .await
    }

    fn watches(&self) -> bool {
        true
    }

    async fn watch(&self, sink: Arc<dyn EventSink>, cursor: Option<String>) -> Result<()> {
        let from = cursor.as_deref().and_then(|c| hex::decode(c).ok());
        let mut stream = self.node.subscribe_invoices(from).await?;
        while let Some(update) = stream.next().await {
            match update {
                InvoiceUpdate::Settled {
                    payment_hash,
                    external_id,
                    ..
                } => {
                    let _ = external_id;
                    sink.event(
                        ProviderEvent::Paid {
                            reference: payment_hash.clone(),
                            lookup: payment_hash.clone(),
                            amount: None,
                            confirmed: true,
                            saved: None,
                        },
                        Some(payment_hash),
                    )
                    .await?;
                }
                InvoiceUpdate::Error(e) => bail!("invoice stream error: {e}"),
                _ => {}
            }
        }
        Err(anyhow!("invoice stream ended"))
    }

    async fn refund(&self, _attempt: &AttemptRow, req: RefundRequest) -> Result<RefundOutcome> {
        let invoice = req
            .destination
            .context("a bolt11 invoice is needed as the refund destination")?;
        let invoice = invoice.trim().trim_start_matches("lightning:").to_string();
        match invoice_amount_msat(&invoice)? {
            Some(a) if a <= req.amount => {}
            Some(_) => bail!("invoice asks for more than the refundable amount"),
            None => bail!("invoice must carry an amount"),
        }
        let paid = self
            .node
            .pay_invoice(PayInvoiceRequest {
                invoice,
                timeout_seconds: Some(60),
            })
            .await?;
        Ok(RefundOutcome {
            external_id: Some(paid.payment_hash),
        })
    }
}
