use super::{
    AttemptRequest, CreatedAttempt, EventSink, HealthRecorder, Provider, ProviderEvent,
    RefundOutcome, RefundRequest, SavedMethodInfo,
};
use crate::db::{AttemptRow, SavedMethodRow};
use crate::metrics::Metrics;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(45);

pub struct Guarded {
    inner: Arc<dyn Provider>,
    metrics: Arc<Metrics>,
    timeout: Duration,
    health: Option<(u64, HealthCell)>,
}

pub type HealthCell = Arc<OnceLock<Weak<dyn HealthRecorder>>>;

const HEALTH_OPS: &[&str] = &[
    "create",
    "setup",
    "reconcile",
    "receive_address",
    "test_connection",
];

impl Guarded {
    pub fn new(inner: Arc<dyn Provider>, metrics: Arc<Metrics>, timeout: Duration) -> Self {
        Self {
            inner,
            metrics,
            timeout,
            health: None,
        }
    }

    pub fn with_health(mut self, provider_id: u64, recorder: HealthCell) -> Self {
        self.health = Some((provider_id, recorder));
        self
    }

    async fn record_health(&self, op: &str, outcome: Result<(), String>) {
        let Some((id, cell)) = &self.health else {
            return;
        };
        if !HEALTH_OPS.contains(&op) {
            return;
        }
        if let Some(recorder) = cell.get().and_then(Weak::upgrade) {
            recorder.provider_call(*id, outcome).await;
        }
    }

    async fn run<T>(&self, op: &str, fut: impl Future<Output = Result<T>>) -> Result<T> {
        let started = Instant::now();
        let result = match tokio::time::timeout(self.timeout, fut).await {
            Ok(r) => r.map(|v| (v, "ok")).map_err(|e| (e, "error")),
            Err(_) => Err((
                anyhow!(
                    "{} {op} timed out after {:?}",
                    self.inner.method(),
                    self.timeout
                ),
                "timeout",
            )),
        };
        let outcome = match &result {
            Ok((_, o)) => *o,
            Err((_, o)) => *o,
        };
        let labels = [
            ("method", self.inner.method()),
            ("op", op),
            ("outcome", outcome),
        ];
        self.metrics.inc("gateway_provider_calls_total", &labels);
        self.metrics.observe(
            "gateway_provider_call_duration_seconds",
            &[("method", self.inner.method()), ("op", op)],
            started.elapsed().as_secs_f64(),
        );
        let health = match &result {
            Ok(_) => Ok(()),
            Err((e, _)) => Err(format!("{e:#}")),
        };
        self.record_health(op, health).await;
        result.map(|(v, _)| v).map_err(|(e, _)| e)
    }
}

#[async_trait]
impl Provider for Guarded {
    fn method(&self) -> &'static str {
        self.inner.method()
    }

    fn options(&self) -> Vec<String> {
        self.inner.options()
    }

    async fn create(&self, req: AttemptRequest) -> Result<CreatedAttempt> {
        self.run("create", self.inner.create(req)).await
    }

    async fn cancel(&self, attempt: &AttemptRow) -> Result<()> {
        self.run("cancel", self.inner.cancel(attempt)).await
    }

    async fn setup(&self, hook_url: &str) -> Result<Option<Value>> {
        self.run("setup", self.inner.setup(hook_url)).await
    }

    fn watches(&self) -> bool {
        self.inner.watches()
    }

    async fn watch(&self, sink: Arc<dyn EventSink>, cursor: Option<String>) -> Result<()> {
        self.inner.watch(sink, cursor).await
    }

    async fn handle_webhook(
        &self,
        headers: &HashMap<String, String>,
        body: &[u8],
    ) -> Result<Vec<ProviderEvent>> {
        self.run("webhook", self.inner.handle_webhook(headers, body))
            .await
    }

    async fn reconcile(&self, attempt: &AttemptRow) -> Result<Option<ProviderEvent>> {
        self.run("reconcile", self.inner.reconcile(attempt)).await
    }

    async fn receive_address(&self, amount_msat: u64) -> Result<String> {
        self.run("receive_address", self.inner.receive_address(amount_msat))
            .await
    }

    async fn refund(&self, attempt: &AttemptRow, req: RefundRequest) -> Result<RefundOutcome> {
        self.run("refund", self.inner.refund(attempt, req)).await
    }

    async fn charge_saved(
        &self,
        saved: &SavedMethodRow,
        req: AttemptRequest,
    ) -> Result<CreatedAttempt> {
        self.run("charge_saved", self.inner.charge_saved(saved, req))
            .await
    }

    fn event_id(&self, body: &[u8]) -> Option<String> {
        self.inner.event_id(body)
    }

    async fn test_connection(&self) -> Result<String> {
        self.run("test_connection", self.inner.test_connection())
            .await
    }

    async fn remove_saved(&self, saved: &SavedMethodRow) -> Result<()> {
        self.run("remove_saved", self.inner.remove_saved(saved))
            .await
    }

    async fn list_saved(&self, customer_id: &str) -> Result<Vec<SavedMethodInfo>> {
        self.run("list_saved", self.inner.list_saved(customer_id))
            .await
    }

    async fn capture_return(&self, attempt: &AttemptRow) -> Result<Option<ProviderEvent>> {
        self.run("capture_return", self.inner.capture_return(attempt))
            .await
    }

    async fn wallet_balances(&self) -> Result<Vec<super::WalletBalance>> {
        self.run("wallet_balances", self.inner.wallet_balances())
            .await
    }

    async fn describe(&self) -> Result<Option<Value>> {
        self.inner.describe().await
    }

    async fn rescan(&self, from: u32, count: u32) -> Result<String> {
        self.run("rescan", self.inner.rescan(from, count)).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::mock::MockProvider;

    struct Slow(MockProvider);

    #[async_trait]
    impl Provider for Slow {
        fn method(&self) -> &'static str {
            "lightning"
        }

        async fn create(&self, _req: AttemptRequest) -> Result<CreatedAttempt> {
            tokio::time::sleep(Duration::from_secs(30)).await;
            unreachable!()
        }

        async fn refund(&self, a: &AttemptRow, r: RefundRequest) -> Result<RefundOutcome> {
            self.0.refund(a, r).await
        }
    }

    fn request() -> AttemptRequest {
        AttemptRequest {
            payment_id: [0; 32],
            amount: 1,
            currency: "BTC".into(),
            description: String::new(),
            expires: chrono::Utc::now(),
            success_url: None,
            cancel_url: None,
            customer_ref: None,
            customer_email: None,
            option: None,
            save_method: false,
            receive: None,
            lines: vec![],
        }
    }

    #[tokio::test]
    async fn slow_upstream_times_out_and_is_counted() {
        let metrics = Arc::new(Metrics::default());
        let guarded = Guarded::new(
            Arc::new(Slow(MockProvider::new("lightning", Value::Null))),
            metrics.clone(),
            Duration::from_millis(100),
        );
        let started = Instant::now();
        let err = guarded.create(request()).await.unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(err.to_string().contains("timed out"), "{err}");
        assert_eq!(
            metrics.counter(
                "gateway_provider_calls_total",
                &[
                    ("method", "lightning"),
                    ("op", "create"),
                    ("outcome", "timeout")
                ]
            ),
            1
        );
    }
}
