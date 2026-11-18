mod common;
use common::*;
use payment_gateway::ops::integrity;
use payment_gateway::sandbox::Bitcoind;
use payment_gateway::sandbox::bitcoind::BitcoindRpc;
use payment_gateway::sandbox::{LndAccess, Sandbox, SandboxConfig};
use serde_json::{Value, json};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

const MSATS_PER_SAT: u64 = 1000;

fn enabled() -> bool {
    std::env::var("REGTEST").is_ok_and(|v| v == "1")
}

fn regtest_dir() -> String {
    std::env::var("REGTEST_DIR").unwrap_or_else(|_| "/tmp/pgw-reg".into())
}

fn config() -> SandboxConfig {
    let dir = regtest_dir();
    let access = |name: &str, port: u16| LndAccess {
        url: format!("https://localhost:{port}"),
        cert_path: format!("{dir}/{name}.cert"),
        macaroon_path: format!("{dir}/{name}.macaroon"),
    };
    SandboxConfig {
        bitcoind_url: "http://127.0.0.1:18443".into(),
        bitcoind_user: "regtest".into(),
        bitcoind_pass: "regtest".into(),
        gateway: access("gateway", 10009),
        customer: access("customer", 10010),
        gateway_p2p: "lnd-gateway:9735".into(),
    }
}

fn bitcoind() -> BitcoindRpc {
    BitcoindRpc::new("http://127.0.0.1:18443", "regtest", "regtest", "sandbox").unwrap()
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

struct Gateway {
    child: Option<Child>,
    base: String,
    db_url: String,
    port: u16,
    key: String,
    http: reqwest::Client,
}

impl Gateway {
    async fn start(db_url: &str, key: &str) -> Self {
        let port = free_port();
        let mut gw = Gateway {
            child: None,
            base: format!("http://127.0.0.1:{port}"),
            db_url: db_url.to_string(),
            port,
            key: key.to_string(),
            http: reqwest::Client::new(),
        };
        gw.spawn().await;
        gw
    }

    async fn spawn(&mut self) {
        let child = Command::new(env!("CARGO_BIN_EXE_payment-gateway"))
            .env("DATABASE_URL", &self.db_url)
            .env("GATEWAY_ADMIN_TOKEN", ADMIN)
            .env("GATEWAY_ENCRYPTION_KEY", "09".repeat(32))
            .env("GATEWAY_PUBLIC_URL", &self.base)
            .env("GATEWAY_LISTEN", format!("127.0.0.1:{}", self.port))
            .env("RUST_LOG", "warn")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("gateway binary");
        self.child = Some(child);
        for _ in 0..120 {
            if self
                .http
                .get(format!("{}/health", self.base))
                .send()
                .await
                .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        panic!("gateway did not come up");
    }

    fn kill(&mut self) {
        if let Some(mut c) = self.child.take() {
            c.kill().unwrap();
            c.wait().unwrap();
        }
    }

    async fn restart(&mut self) {
        self.kill();
        self.spawn().await;
    }

    async fn api(&self, method: &str, path: &str, body: Value) -> Value {
        let mut req = self
            .http
            .request(method.parse().unwrap(), format!("{}{path}", self.base))
            .bearer_auth(&self.key);
        if !body.is_null() {
            req = req.json(&body);
        }
        let rsp = req.send().await.unwrap();
        let status = rsp.status();
        let v: Value = rsp.json().await.unwrap_or(Value::Null);
        assert!(status.is_success(), "{method} {path}: {status} {v}");
        v
    }

    async fn payment(&self, id: &str) -> Value {
        self.api("GET", &format!("/v1/payments/{id}"), Value::Null)
            .await
    }

    async fn status(&self, id: &str) -> String {
        self.payment(id).await["status"]
            .as_str()
            .unwrap_or("?")
            .to_string()
    }

    async fn wait_status(&self, id: &str, want: &str, secs: u64) {
        let mut last = String::new();
        for _ in 0..secs * 2 {
            last = self.status(id).await;
            if last == want {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("payment {id} is {last}, wanted {want}");
    }

    async fn create(&self, reference: &str, sats: u64, method: &str, expires_in: i64) -> Created {
        let p = self
            .api(
                "POST",
                "/v1/payments",
                json!({"reference": reference, "amount": sats * MSATS_PER_SAT,
                       "currency": "BTC", "expires_in": expires_in}),
            )
            .await;
        let id = p["id"].as_str().unwrap().to_string();
        let r = self
            .api(
                "POST",
                &format!("/v1/payments/{id}/attempts"),
                json!({"method": method}),
            )
            .await;
        let details = r["attempts"][0]["details"].clone();
        Created { id, details, sats }
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.kill();
    }
}

struct Created {
    id: String,
    details: Value,
    sats: u64,
}

impl Created {
    fn bolt11(&self) -> String {
        self.details["bolt11"].as_str().unwrap().to_string()
    }

    fn address(&self) -> String {
        self.details["address"].as_str().unwrap().to_string()
    }

    fn required_sats(&self) -> u64 {
        self.details["amount_sats"].as_u64().unwrap_or(self.sats)
    }
}

fn compose(args: &[&str]) {
    let status = Command::new("docker")
        .args(["compose", "-f", "regtest/compose.yaml"])
        .args(args)
        .status()
        .expect("docker compose");
    assert!(status.success(), "docker compose {args:?}");
}

async fn wait_for<F, Fut>(what: &str, secs: u64, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..secs * 2 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn payment_events(h: &Harness, id: &str) -> i64 {
    let pid = payment_gateway::db::parse_payment_id(id).unwrap();
    sqlx::query_scalar(
        "select count(*) from webhook_delivery where payment_id = ? and event_type = 'payment.paid'",
    )
    .bind(pid.as_slice())
    .fetch_one(&h.pool)
    .await
    .unwrap()
}

async fn confirmed_receipts_sats(h: &Harness, id: &str) -> u64 {
    let pid = payment_gateway::db::parse_payment_id(id).unwrap();
    let msats: i64 = sqlx::query_scalar(
        "select cast(coalesce(sum(r.amount), 0) as signed) from attempt_receipt r join payment_attempt a on a.id = r.attempt_id where a.payment_id = ? and r.confirmed = 1",
    )
    .bind(pid.as_slice())
    .fetch_one(&h.pool)
    .await
    .unwrap();
    msats as u64 / MSATS_PER_SAT
}

async fn assert_settled_exactly_once(h: &Harness, gw: &Gateway, ids: &[String]) {
    for id in ids {
        assert_eq!(gw.status(id).await, "paid", "payment {id}");
        assert_eq!(
            payment_events(h, id).await,
            1,
            "payment.paid count for {id}"
        );
    }
    let findings = integrity::check(&h.pool).await.unwrap();
    assert!(findings.is_empty(), "integrity findings: {findings:?}");
}

async fn many_parallel_lightning_payments(h: &Harness, gw: &Gateway, sb: &Arc<Sandbox>) {
    let mut created = vec![];
    for i in 0..20 {
        created.push(
            gw.create(&format!("par-{i}"), 20_000, "lightning", 600)
                .await,
        );
    }
    let mut set = tokio::task::JoinSet::new();
    for c in &created {
        let (sb, bolt11) = (sb.clone(), c.bolt11());
        set.spawn(async move { sb.customer_pay_invoice(&bolt11).await });
    }
    while let Some(r) = set.join_next().await {
        r.unwrap().expect("parallel payment");
    }
    let ids: Vec<String> = created.iter().map(|c| c.id.clone()).collect();
    for id in &ids {
        gw.wait_status(id, "paid", 60).await;
    }
    assert_settled_exactly_once(h, gw, &ids).await;
}

async fn underpayment_then_top_up(h: &Harness, gw: &Gateway, btc: &BitcoindRpc, miner: &str) {
    let c = gw.create("under", 50_000, "onchain", 3600).await;
    let required = c.required_sats();
    let first = required / 2;
    btc.send(&c.address(), first).await.unwrap();
    btc.mine(1, miner).await.unwrap();
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(gw.status(&c.id).await, "pending");
    btc.send(&c.address(), required - first).await.unwrap();
    btc.mine(1, miner).await.unwrap();
    gw.wait_status(&c.id, "paid", 60).await;
    assert_eq!(confirmed_receipts_sats(h, &c.id).await, required);
    assert_settled_exactly_once(h, gw, &[c.id]).await;
}

async fn overpayment(h: &Harness, gw: &Gateway, btc: &BitcoindRpc, miner: &str) {
    let c = gw.create("over", 40_000, "onchain", 3600).await;
    let required = c.required_sats();
    btc.send(&c.address(), required * 2).await.unwrap();
    btc.mine(1, miner).await.unwrap();
    gw.wait_status(&c.id, "paid", 60).await;
    assert_eq!(confirmed_receipts_sats(h, &c.id).await, required * 2);
    assert_settled_exactly_once(h, gw, &[c.id]).await;
}

async fn rbf_replaced_transaction(h: &Harness, gw: &Gateway, btc: &BitcoindRpc, miner: &str) {
    let c = gw.create("rbf", 30_000, "onchain", 3600).await;
    let required = c.required_sats();
    let original = btc.send_replaceable(&c.address(), required).await.unwrap();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert_eq!(gw.status(&c.id).await, "pending");
    let replacement = btc.bump_fee(&original, 60).await.unwrap();
    assert_ne!(original, replacement);
    btc.mine(1, miner).await.unwrap();
    gw.wait_status(&c.id, "paid", 60).await;
    assert_eq!(confirmed_receipts_sats(h, &c.id).await, required);
    assert_settled_exactly_once(h, gw, &[c.id]).await;
}

async fn gateway_killed_mid_payment(h: &Harness, gw: &mut Gateway, sb: &Arc<Sandbox>) {
    let mut created = vec![];
    for i in 0..10 {
        created.push(
            gw.create(&format!("kill-{i}"), 15_000, "lightning", 600)
                .await,
        );
    }
    let mut set = tokio::task::JoinSet::new();
    for c in &created {
        let (sb, bolt11) = (sb.clone(), c.bolt11());
        set.spawn(async move { sb.customer_pay_invoice(&bolt11).await });
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    gw.kill();
    while let Some(r) = set.join_next().await {
        r.unwrap()
            .expect("payment settles at lnd without the gateway");
    }
    gw.spawn().await;
    let ids: Vec<String> = created.iter().map(|c| c.id.clone()).collect();
    for id in &ids {
        gw.wait_status(id, "paid", 90).await;
    }
    gw.restart().await;
    tokio::time::sleep(Duration::from_secs(8)).await;
    assert_settled_exactly_once(h, gw, &ids).await;
}

async fn lnd_restart_reconnects_the_invoice_stream(h: &Harness, gw: &Gateway, sb: &Arc<Sandbox>) {
    compose(&["restart", "lnd-gateway"]);
    let customer = sb.clone();
    wait_for("gateway lnd and channel", 120, || {
        let customer = customer.clone();
        async move {
            let Ok(chans) = customer.customer_channels().await else {
                return false;
            };
            chans.iter().any(|c| c.active)
        }
    })
    .await;
    tokio::time::sleep(Duration::from_secs(10)).await;
    let c = gw
        .create("after-lnd-restart", 25_000, "lightning", 600)
        .await;
    sb.customer_pay_invoice(&c.bolt11()).await.unwrap();
    gw.wait_status(&c.id, "paid", 90).await;
    assert_settled_exactly_once(h, gw, &[c.id]).await;
}

async fn expiry_racing_a_payment(h: &Harness, gw: &Gateway, sb: &Arc<Sandbox>) {
    let c = gw.create("race", 10_000, "lightning", 600).await;
    let pid = payment_gateway::db::parse_payment_id(&c.id).unwrap();
    sqlx::query("update payment set expires = utc_timestamp() + interval 2 second where id = ?")
        .bind(pid.as_slice())
        .execute(&h.pool)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let paid_ok = sb.customer_pay_invoice(&c.bolt11()).await.is_ok();
    tokio::time::sleep(Duration::from_secs(12)).await;
    let status = gw.status(&c.id).await;
    let events = payment_events(h, &c.id).await;
    if paid_ok {
        assert_eq!(status, "paid");
        assert_eq!(events, 1);
    } else {
        assert!(matches!(status.as_str(), "expired" | "pending"), "{status}");
        assert_eq!(events, 0);
    }
    let findings = integrity::check(&h.pool).await.unwrap();
    assert!(findings.is_empty(), "integrity findings: {findings:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn long_sequence_of_real_payments_with_restarts_settles_each_payment_once() {
    if !enabled() {
        eprintln!("skipped: set REGTEST=1 after regtest/scripts/up.sh");
        return;
    }
    let h = harness().await;
    let sb = Sandbox::from_config(config(), h.service.clone()).unwrap();
    sb.bootstrap().await.expect("bootstrap against regtest");
    sb.ensure_providers().await.unwrap();
    let key = sb.rotate_key().await.unwrap();

    let base = std::env::var("TEST_DATABASE_URL").unwrap();
    let (prefix, _) = base.rsplit_once('/').unwrap();
    let database = h.pool.connect_options().get_database().unwrap().to_string();
    let db_url = format!("{prefix}/{database}");
    let mut gw = Gateway::start(&db_url, &key).await;

    let btc = bitcoind();
    let miner = sb.miner_address().await.unwrap();

    many_parallel_lightning_payments(&h, &gw, &sb).await;
    underpayment_then_top_up(&h, &gw, &btc, &miner).await;
    overpayment(&h, &gw, &btc, &miner).await;
    rbf_replaced_transaction(&h, &gw, &btc, &miner).await;
    gateway_killed_mid_payment(&h, &mut gw, &sb).await;
    lnd_restart_reconnects_the_invoice_stream(&h, &gw, &sb).await;
    expiry_racing_a_payment(&h, &gw, &sb).await;
}
