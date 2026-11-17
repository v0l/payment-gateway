use super::{
    AttemptRequest, CreatedAttempt, EventSink, Provider, ProviderEvent, RefundOutcome,
    RefundRequest, SavedMethodInfo,
};
use crate::db::{AttemptRow, ProviderConfigRow, SavedMethodRow};
use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;

#[derive(Default)]
pub struct MockFactory {
    providers: Mutex<HashMap<u64, Arc<MockProvider>>>,
}

impl MockFactory {
    pub async fn build(&self, row: &ProviderConfigRow, config: Value) -> Result<Arc<dyn Provider>> {
        let method = super::super::model::METHODS
            .iter()
            .find(|m| **m == row.method)
            .copied()
            .ok_or_else(|| anyhow!("unknown method {}", row.method))?;
        let provider = Arc::new(MockProvider::new(method, config));
        self.providers
            .lock()
            .unwrap()
            .insert(row.id, provider.clone());
        Ok(provider)
    }

    pub fn get(&self, id: u64) -> Arc<MockProvider> {
        self.providers
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .expect("mock provider not built yet")
    }
}

pub struct MockProvider {
    method: &'static str,
    config: Value,
    counter: AtomicU64,
    queue: Mutex<VecDeque<(ProviderEvent, Option<String>)>>,
    notify: Notify,
    pub created: Mutex<Vec<AttemptRequest>>,
    pub cancelled: Mutex<Vec<String>>,
    pub refunds: Mutex<Vec<RefundRequest>>,
    pub fail_create: Mutex<bool>,
    pub fail_refund: Mutex<bool>,
    pub watch_starts: Mutex<Vec<Option<String>>>,
    settled: Mutex<std::collections::HashSet<String>>,
}

impl MockProvider {
    pub fn new(method: &'static str, config: Value) -> Self {
        Self {
            method,
            config,
            counter: AtomicU64::new(0),
            queue: Default::default(),
            notify: Notify::new(),
            created: Default::default(),
            cancelled: Default::default(),
            refunds: Default::default(),
            fail_create: Default::default(),
            fail_refund: Default::default(),
            watch_starts: Default::default(),
            settled: Default::default(),
        }
    }

    pub fn emit(&self, event: ProviderEvent, cursor: Option<&str>) {
        self.queue
            .lock()
            .unwrap()
            .push_back((event, cursor.map(str::to_string)));
        self.notify.notify_one();
    }

    pub fn mark_settled(&self, lookup: &str) {
        self.settled.lock().unwrap().insert(lookup.to_string());
    }

    pub fn secret(&self) -> String {
        self.config
            .get("secret")
            .and_then(Value::as_str)
            .unwrap_or("mock-secret")
            .to_string()
    }
}

#[async_trait]
impl Provider for MockProvider {
    fn method(&self) -> &'static str {
        self.method
    }

    fn options(&self) -> Vec<String> {
        super::super::model::options_from_config(self.method, &self.config)
    }

    async fn create(&self, req: AttemptRequest) -> Result<CreatedAttempt> {
        if *self.fail_create.lock().unwrap() {
            bail!("mock create failure");
        }
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        self.created.lock().unwrap().push(req.clone());
        let tag = format!("{}-{}-{n}", self.method, hex::encode(&req.payment_id[..4]));
        let details = match self.method {
            "lightning" => json!({
                "bolt11": format!("lnmock{tag}"),
                "payment_hash": tag,
                "amount_msat": req.amount,
            }),
            "onchain" => json!({
                "address": format!("addr-{tag}"),
                "amount_sats": req.amount.div_ceil(1000),
                "uri": format!("bitcoin:addr-{tag}"),
            }),
            "fixedfloat" => {
                let receive = req
                    .receive
                    .as_ref()
                    .ok_or_else(|| anyhow!("receive provider missing"))?
                    .receive_address(req.amount)
                    .await?;
                return Ok(CreatedAttempt {
                    lookup: receive.clone(),
                    external_id: Some(tag.clone()),
                    details: json!({
                        "coin": req.option,
                        "deposit_address": format!("dep-{tag}"),
                        "deposit_amount": "1.5",
                        "receive_address": receive,
                    }),
                    expires: None,
                    completed: false,
                });
            }
            _ => json!({"checkout_url": format!("https://checkout.mock/{tag}")}),
        };
        let lookup = match self.method {
            "onchain" => format!("addr-{tag}"),
            _ => tag.clone(),
        };
        Ok(CreatedAttempt {
            lookup,
            external_id: Some(tag),
            details,
            expires: None,
            completed: false,
        })
    }

    async fn cancel(&self, attempt: &AttemptRow) -> Result<()> {
        self.cancelled.lock().unwrap().push(attempt.lookup.clone());
        Ok(())
    }

    fn watches(&self) -> bool {
        matches!(self.method, "lightning" | "onchain")
    }

    async fn watch(&self, sink: Arc<dyn EventSink>, cursor: Option<String>) -> Result<()> {
        self.watch_starts.lock().unwrap().push(cursor);
        loop {
            let next = self.queue.lock().unwrap().pop_front();
            match next {
                Some((event, cursor)) => sink.event(event, cursor).await?,
                None => self.notify.notified().await,
            }
        }
    }

    async fn handle_webhook(
        &self,
        headers: &HashMap<String, String>,
        body: &[u8],
    ) -> Result<Vec<ProviderEvent>> {
        if headers.get("x-mock-secret").map(String::as_str) != Some(self.secret().as_str()) {
            bail!("bad mock signature");
        }
        let v: Value = serde_json::from_slice(body)?;
        let lookup = v["lookup"]
            .as_str()
            .ok_or_else(|| anyhow!("lookup"))?
            .to_string();
        Ok(match v["kind"].as_str() {
            Some("failed") => vec![ProviderEvent::Failed {
                lookup,
                reason: "declined".into(),
            }],
            _ => vec![ProviderEvent::Paid {
                reference: lookup.clone(),
                lookup,
                amount: v["amount"].as_u64(),
                confirmed: true,
                saved: v["saved"].as_str().map(|id| SavedMethodInfo {
                    customer_id: "cus_mock".into(),
                    method_id: id.to_string(),
                    brand: Some("visa".into()),
                    last4: Some("4242".into()),
                    exp_month: Some(12),
                    exp_year: Some(2099),
                }),
            }],
        })
    }

    async fn reconcile(&self, attempt: &AttemptRow) -> Result<Option<ProviderEvent>> {
        if self.settled.lock().unwrap().contains(&attempt.lookup) {
            return Ok(Some(ProviderEvent::Paid {
                lookup: attempt.lookup.clone(),
                reference: attempt.lookup.clone(),
                amount: None,
                confirmed: true,
                saved: None,
            }));
        }
        Ok(None)
    }

    async fn receive_address(&self, _amount_msat: u64) -> Result<String> {
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        Ok(format!("recv-addr-{n}"))
    }

    async fn refund(&self, _attempt: &AttemptRow, req: RefundRequest) -> Result<RefundOutcome> {
        if *self.fail_refund.lock().unwrap() {
            bail!("mock refund failure");
        }
        self.refunds.lock().unwrap().push(req);
        Ok(RefundOutcome {
            external_id: Some(format!(
                "refund-{}",
                self.counter.fetch_add(1, Ordering::SeqCst)
            )),
        })
    }

    async fn charge_saved(
        &self,
        saved: &SavedMethodRow,
        req: AttemptRequest,
    ) -> Result<CreatedAttempt> {
        if *self.fail_create.lock().unwrap() {
            bail!("mock charge failure");
        }
        let n = self.counter.fetch_add(1, Ordering::SeqCst);
        self.created.lock().unwrap().push(req);
        let tag = format!("charge-{}-{n}", saved.external_id);
        Ok(CreatedAttempt {
            lookup: tag.clone(),
            external_id: Some(tag),
            details: json!({"charged": saved.external_id}),
            expires: None,
            completed: true,
        })
    }
}
