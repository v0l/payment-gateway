mod common;
use async_trait::async_trait;
use axum::http::StatusCode;
use common::*;
use payment_gateway::api::{self, AppState};
use payment_gateway::sandbox::{Bitcoind, ChannelInfo, LnNode, LndAccess, NodeInfo, Sandbox};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Chain {
    height: Mutex<u64>,
    sent: Mutex<Vec<(String, u64)>>,
    wallet: Mutex<bool>,
}

#[async_trait]
impl Bitcoind for Chain {
    async fn ensure_wallet(&self) -> anyhow::Result<()> {
        *self.wallet.lock().unwrap() = true;
        Ok(())
    }
    async fn new_address(&self) -> anyhow::Result<String> {
        Ok("bcrt1qminer".into())
    }
    async fn mine(&self, blocks: u32, _to: &str) -> anyhow::Result<u64> {
        let mut h = self.height.lock().unwrap();
        *h += blocks as u64;
        Ok(*h)
    }
    async fn send(&self, address: &str, sats: u64) -> anyhow::Result<String> {
        self.sent.lock().unwrap().push((address.to_string(), sats));
        Ok(format!("tx{}", self.sent.lock().unwrap().len()))
    }
    async fn height(&self) -> anyhow::Result<u64> {
        Ok(*self.height.lock().unwrap())
    }
    async fn balance_sats(&self) -> anyhow::Result<u64> {
        Ok(if *self.height.lock().unwrap() > 100 {
            5_000_000_000
        } else {
            0
        })
    }
}

struct Node {
    key: String,
    chain: Arc<Chain>,
    funded: Mutex<u64>,
    channels: Mutex<Vec<ChannelInfo>>,
    paid: Mutex<Vec<String>>,
    coins: Mutex<Vec<(String, u64)>>,
    connected: Mutex<Vec<(String, String)>>,
    counter: Mutex<u32>,
}

impl Node {
    fn new(key: &str, chain: Arc<Chain>) -> Arc<Self> {
        Arc::new(Self {
            key: key.into(),
            chain,
            funded: Mutex::new(0),
            channels: Default::default(),
            paid: Default::default(),
            coins: Default::default(),
            connected: Default::default(),
            counter: Default::default(),
        })
    }
}

#[async_trait]
impl LnNode for Node {
    async fn info(&self) -> anyhow::Result<NodeInfo> {
        let sent = self.chain.sent.lock().unwrap().clone();
        let mine: u64 = sent
            .iter()
            .filter(|(a, _)| a.starts_with(&format!("addr-{}", self.key)))
            .map(|(_, s)| s)
            .sum();
        *self.funded.lock().unwrap() = mine;
        Ok(NodeInfo {
            pubkey: format!("pub-{}", self.key),
            synced: true,
            height: *self.chain.height.lock().unwrap(),
            active_channels: self
                .channels
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.active)
                .count() as u32,
            onchain_sats: mine,
            unconfirmed_sats: 0,
        })
    }
    async fn new_address(&self) -> anyhow::Result<String> {
        let mut c = self.counter.lock().unwrap();
        *c += 1;
        Ok(format!("addr-{}-{}", self.key, *c))
    }
    async fn connect(&self, pubkey: &str, host: &str) -> anyhow::Result<()> {
        self.connected
            .lock()
            .unwrap()
            .push((pubkey.into(), host.into()));
        Ok(())
    }
    async fn open_channel(&self, pubkey: &str, sats: u64, push: u64) -> anyhow::Result<()> {
        self.channels.lock().unwrap().push(ChannelInfo {
            remote_pubkey: pubkey.into(),
            capacity_sats: sats,
            local_sats: sats - push,
            remote_sats: push,
            active: true,
        });
        Ok(())
    }
    async fn channels(&self) -> anyhow::Result<Vec<ChannelInfo>> {
        Ok(self.channels.lock().unwrap().clone())
    }
    async fn pay_invoice(&self, bolt11: &str) -> anyhow::Result<String> {
        self.paid.lock().unwrap().push(bolt11.into());
        Ok("preimage".into())
    }
    async fn send_coins(&self, address: &str, sats: u64) -> anyhow::Result<String> {
        self.coins.lock().unwrap().push((address.into(), sats));
        Ok("coin-tx".into())
    }
}

struct Rig {
    h: Harness,
    app: axum::Router,
    sb: Arc<Sandbox>,
    chain: Arc<Chain>,
    customer: Arc<Node>,
    gateway: Arc<Node>,
}

async fn rig() -> Rig {
    let h = harness().await;
    let chain = Arc::new(Chain::default());
    let gateway = Node::new("gateway", chain.clone());
    let customer = Node::new("customer", chain.clone());
    let mut sb = Sandbox::new(
        chain.clone(),
        gateway.clone(),
        customer.clone(),
        LndAccess {
            url: "https://lnd:10009".into(),
            cert_path: "/c".into(),
            macaroon_path: "/m".into(),
        },
        "lnd-gateway:9735".into(),
        h.service.clone(),
    );
    sb.poll = std::time::Duration::from_millis(5);
    sb.polls = 20;
    let sb = Arc::new(sb);
    let app = api::router(AppState {
        service: h.service.clone(),
        admin_token: ADMIN.into(),
        allow_mock: true,
        sandbox: Some(sb.clone()),
    });
    Rig {
        h,
        app,
        sb,
        chain,
        customer,
        gateway,
    }
}

async fn admin(app: &axum::Router, method: &str, uri: &str, body: Value) -> (StatusCode, Value) {
    call(app, method, uri, Some(ADMIN), body).await
}

#[tokio::test]
async fn sandbox_routes_do_not_exist_when_the_sandbox_is_not_configured() {
    let h = harness().await;
    for uri in ["/sandbox", "/sandbox/app.js", "/sandbox/api/status"] {
        let r = send(&h.app, "GET", uri, Some(ADMIN), &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{uri}");
    }
    let r = send(&h.app, "POST", "/sandbox/hook", None, &[], json!({})).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sandbox_api_needs_the_admin_token_but_the_page_shell_is_public() {
    let r = rig().await;
    let page = send(&r.app, "GET", "/sandbox", None, &[], Value::Null).await;
    assert_eq!(page.status, StatusCode::OK);
    let csp = page.headers["content-security-policy"].to_str().unwrap();
    assert!(
        csp.contains("frame-ancestors 'none'") && csp.contains("script-src 'self'"),
        "{csp}"
    );
    assert!(!page.text.contains(ADMIN));
    for (m, uri) in [
        ("GET", "/sandbox/api/status"),
        ("POST", "/sandbox/api/bootstrap"),
        ("POST", "/sandbox/api/mine"),
        ("POST", "/sandbox/api/fund"),
        ("POST", "/sandbox/api/faucet"),
        ("POST", "/sandbox/api/pay-invoice"),
        ("POST", "/sandbox/api/payments"),
        ("GET", "/sandbox/api/hooks"),
        ("POST", "/sandbox/api/rotate-key"),
    ] {
        let none = send(&r.app, m, uri, None, &[], json!({})).await;
        assert_eq!(none.status, StatusCode::UNAUTHORIZED, "{uri}");
        let wrong = send(&r.app, m, uri, Some("nope"), &[], json!({})).await;
        assert_eq!(wrong.status, StatusCode::UNAUTHORIZED, "{uri}");
    }
}

#[tokio::test]
async fn bootstrap_funds_both_nodes_opens_a_channel_and_creates_the_tenant_once() {
    let r = rig().await;
    let (s, first) = admin(&r.app, "POST", "/sandbox/api/bootstrap", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(first["ok"], true, "{first}");
    let log = first["log"].to_string();
    assert!(log.contains("api key pgw_"));

    assert!(*r.chain.wallet.lock().unwrap());
    assert!(*r.chain.height.lock().unwrap() >= 101);
    assert_eq!(
        r.customer.connected.lock().unwrap()[0],
        ("pub-gateway".to_string(), "lnd-gateway:9735".to_string())
    );
    let channels = r.customer.channels.lock().unwrap().clone();
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].remote_pubkey, "pub-gateway");

    let (_, status) = admin(&r.app, "GET", "/sandbox/api/status", Value::Null).await;
    assert_eq!(status["ready"], true, "{status}");
    assert_eq!(
        status["tenant"]["providers"],
        json!(["lightning", "onchain"])
    );

    let (_, again) = admin(&r.app, "POST", "/sandbox/api/bootstrap", json!({})).await;
    assert_eq!(again["ok"], true, "{again}");
    assert!(again["log"].to_string().contains("already exists"));
    assert_eq!(
        r.customer.channels.lock().unwrap().len(),
        1,
        "no second channel"
    );
    let (_, status) = admin(&r.app, "GET", "/sandbox/api/status", Value::Null).await;
    assert_eq!(
        status["tenant"]["providers"].as_array().unwrap().len(),
        2,
        "providers not duplicated"
    );
}

#[tokio::test]
async fn provider_config_points_at_the_gateway_node_and_stays_sealed() {
    let r = rig().await;
    admin(&r.app, "POST", "/sandbox/api/bootstrap", json!({})).await;
    let tenant = r.sb.tenant().await.unwrap().unwrap();
    let rows = payment_gateway::db::list_provider_configs(&r.h.pool, tenant.id, true)
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row.provider_type, "lnd");
        assert!(
            !String::from_utf8_lossy(&row.config).contains("lnd:10009"),
            "config must be encrypted"
        );
        let cfg = r.h.service.registry.open_config(row).unwrap();
        assert_eq!(cfg["url"], "https://lnd:10009");
        assert_eq!(cfg["macaroon_path"], "/m");
    }
}

#[tokio::test]
async fn tools_validate_input_and_hit_the_right_wallet() {
    let r = rig().await;
    let (s, _) = admin(&r.app, "POST", "/sandbox/api/mine", json!({"blocks": 0})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = admin(&r.app, "POST", "/sandbox/api/mine", json!({"blocks": 500})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, m) = admin(&r.app, "POST", "/sandbox/api/mine", json!({"blocks": 3})).await;
    assert_eq!((s, m["height"].as_u64()), (StatusCode::OK, Some(3)));

    let (s, _) = admin(
        &r.app,
        "POST",
        "/sandbox/api/fund",
        json!({"target": "nobody", "sats": 10}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = admin(
        &r.app,
        "POST",
        "/sandbox/api/fund",
        json!({"target": "customer", "sats": 0}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = admin(
        &r.app,
        "POST",
        "/sandbox/api/fund",
        json!({"target": "customer", "sats": 777}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        r.chain.sent.lock().unwrap().last().unwrap(),
        &("addr-customer-1".to_string(), 777)
    );

    let (s, _) = admin(
        &r.app,
        "POST",
        "/sandbox/api/faucet",
        json!({"address": " ", "sats": 5}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = admin(
        &r.app,
        "POST",
        "/sandbox/api/faucet",
        json!({"address": "bcrt1qxyz", "sats": 123}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        r.chain.sent.lock().unwrap().last().unwrap(),
        &("bcrt1qxyz".to_string(), 123)
    );

    let (s, p) = admin(
        &r.app,
        "POST",
        "/sandbox/api/pay-invoice",
        json!({"bolt11": "lightning:lnbcrt1abc"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{p}");
    assert_eq!(r.customer.paid.lock().unwrap()[0], "lightning:lnbcrt1abc");
    assert!(
        r.gateway.paid.lock().unwrap().is_empty(),
        "the gateway node never pays"
    );
}

async fn bootstrapped() -> Rig {
    let r = rig().await;
    let (_, b) = admin(&r.app, "POST", "/sandbox/api/bootstrap", json!({})).await;
    assert_eq!(b["ok"], true, "{b}");
    r
}

#[tokio::test]
async fn payments_need_bootstrap_first() {
    let r = rig().await;
    let (s, e) = admin(
        &r.app,
        "POST",
        "/sandbox/api/payments",
        json!({"reference": "x", "amount": 1, "currency": "EUR"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{e}");
    let (s, list) = admin(&r.app, "GET", "/sandbox/api/payments", Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    assert!(list.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn pay_from_customer_needs_an_attempt_and_refuses_finished_payments() {
    let r = bootstrapped().await;
    let (_, p) = admin(
        &r.app,
        "POST",
        "/sandbox/api/payments",
        json!({"reference": "o", "amount": 1000, "currency": "EUR"}),
    )
    .await;
    let id = p["id"].as_str().unwrap();
    let (s, _) = admin(
        &r.app,
        "POST",
        &format!("/sandbox/api/payments/{id}/pay"),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "no attempt yet");
    let (s, _) = admin(
        &r.app,
        "POST",
        &format!("/sandbox/api/payments/{id}/attempt"),
        json!({"method": "stripe"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "no stripe provider");
    let (s, _) = admin(&r.app, "POST", "/sandbox/api/payments/zz/pay", json!({})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn customer_pays_the_real_invoice_the_gateway_created() {
    let r = bootstrapped().await;
    let (_, p) = admin(
        &r.app,
        "POST",
        "/sandbox/api/payments",
        json!({"reference": "o", "amount": 1000, "currency": "EUR"}),
    )
    .await;
    let id = p["id"].as_str().unwrap();
    let (s, e) = admin(
        &r.app,
        "POST",
        &format!("/sandbox/api/payments/{id}/attempt"),
        json!({"method": "lightning"}),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::BAD_GATEWAY,
        "fake LND cannot mint invoices, so the gateway reports the provider error: {e}"
    );
}

#[tokio::test]
async fn onchain_payment_sends_the_exact_sats_to_the_attempt_address_and_mines() {
    let r = bootstrapped().await;
    let tenant = r.sb.tenant().await.unwrap().unwrap();
    let provider_id = payment_gateway::db::list_provider_configs(&r.h.pool, tenant.id, true)
        .await
        .unwrap()
        .into_iter()
        .find(|p| p.method == "onchain")
        .unwrap()
        .id;
    let _ = provider_id;
    let (_, p) = admin(
        &r.app,
        "POST",
        "/sandbox/api/payments",
        json!({"reference": "o", "amount": 1000, "currency": "EUR"}),
    )
    .await;
    let pid = payment_gateway::db::parse_payment_id(p["id"].as_str().unwrap()).unwrap();
    payment_gateway::db::insert_attempt(
        &r.h.pool,
        &payment_gateway::db::NewAttempt {
            payment_id: pid,
            provider_config_id: provider_id,
            watch_provider_id: provider_id,
            method: "onchain",
            option_key: "",
            base_amount: 1000,
            processing_fee: 0,
            currency: "EUR",
            charge_amount: 21_000_000,
            charge_currency: "BTC",
            rate: 1.0,
            lookup: "bcrt1qtarget",
            external_id: None,
            details: &json!({"address": "bcrt1qtarget", "amount_sats": 21_000}).to_string(),
            created: chrono::Utc::now(),
            expires: chrono::Utc::now() + chrono::Duration::hours(1),
            save_method: false,
        },
    )
    .await
    .unwrap();
    let before = *r.chain.height.lock().unwrap();
    let (s, m) = admin(
        &r.app,
        "POST",
        &format!("/sandbox/api/payments/{}/pay", p["id"].as_str().unwrap()),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{m}");
    assert_eq!(
        r.customer.coins.lock().unwrap()[0],
        ("bcrt1qtarget".to_string(), 21_000)
    );
    assert_eq!(*r.chain.height.lock().unwrap(), before + 1);
}

fn signed(secret: &str, body: &str) -> String {
    let ts = chrono::Utc::now().timestamp();
    format!(
        "t={ts},v1={}",
        payment_gateway::service::webhook_signature(secret, ts, body)
    )
}

#[tokio::test]
async fn webhook_inspector_records_events_and_checks_the_signature() {
    let r = bootstrapped().await;
    let tenant = r.sb.tenant().await.unwrap().unwrap();
    let secret =
        String::from_utf8(r.h.service.sealer.open(&tenant.webhook_secret).unwrap()).unwrap();
    let body = json!({"id": "evt1", "type": "payment.paid", "data": {"payment": {"id": "abc", "status": "paid"}}}).to_string();

    let good = send(
        &r.app,
        "POST",
        "/sandbox/hook",
        None,
        &[("x-gateway-signature", &signed(&secret, &body))],
        serde_json::from_str(&body).unwrap(),
    )
    .await;
    assert_eq!(good.status, StatusCode::OK);
    let bad = send(
        &r.app,
        "POST",
        "/sandbox/hook",
        None,
        &[("x-gateway-signature", &signed("wrong", &body))],
        serde_json::from_str(&body).unwrap(),
    )
    .await;
    assert_eq!(bad.status, StatusCode::OK);
    let missing = send(
        &r.app,
        "POST",
        "/sandbox/hook",
        None,
        &[],
        serde_json::from_str(&body).unwrap(),
    )
    .await;
    assert_eq!(missing.status, StatusCode::OK);
    let stale_ts = chrono::Utc::now().timestamp() - 3600;
    let stale_sig = format!(
        "t={stale_ts},v1={}",
        payment_gateway::service::webhook_signature(&secret, stale_ts, &body)
    );
    send(
        &r.app,
        "POST",
        "/sandbox/hook",
        None,
        &[("x-gateway-signature", &stale_sig)],
        serde_json::from_str(&body).unwrap(),
    )
    .await;

    let (_, hooks) = admin(&r.app, "GET", "/sandbox/api/hooks", Value::Null).await;
    let flags: Vec<bool> = hooks
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["signature_valid"].as_bool().unwrap())
        .collect();
    assert_eq!(
        flags,
        vec![false, false, false, true],
        "newest first: stale, missing, bad, good"
    );
    assert_eq!(hooks[3]["event_type"], "payment.paid");
    assert_eq!(hooks[3]["status"], "paid");
}

#[tokio::test]
async fn webhook_inspector_keeps_only_the_most_recent_events() {
    let r = bootstrapped().await;
    for i in 0..60 {
        r.sb.record_hook(
            None,
            &json!({"id": format!("e{i}"), "type": "t"}).to_string(),
        )
        .await;
    }
    let hooks = r.sb.hooks();
    assert_eq!(hooks.len(), 50);
    assert_eq!(hooks[0].event_id, "e59");
}

#[tokio::test]
async fn rotating_the_key_replaces_the_old_one() {
    let r = bootstrapped().await;
    let (s, k) = admin(&r.app, "POST", "/sandbox/api/rotate-key", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let key = k["api_key"].as_str().unwrap();
    let (s, _) = call(&r.app, "GET", "/v1/methods", Some(key), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn status_reports_node_errors_without_failing_the_whole_call() {
    struct Dead;
    #[async_trait]
    impl LnNode for Dead {
        async fn info(&self) -> anyhow::Result<NodeInfo> {
            anyhow::bail!("connection refused")
        }
        async fn new_address(&self) -> anyhow::Result<String> {
            anyhow::bail!("down")
        }
        async fn connect(&self, _: &str, _: &str) -> anyhow::Result<()> {
            anyhow::bail!("down")
        }
        async fn open_channel(&self, _: &str, _: u64, _: u64) -> anyhow::Result<()> {
            anyhow::bail!("down")
        }
        async fn channels(&self) -> anyhow::Result<Vec<ChannelInfo>> {
            anyhow::bail!("down")
        }
        async fn pay_invoice(&self, _: &str) -> anyhow::Result<String> {
            anyhow::bail!("down")
        }
        async fn send_coins(&self, _: &str, _: u64) -> anyhow::Result<String> {
            anyhow::bail!("down")
        }
    }
    let h = harness().await;
    let chain = Arc::new(Chain::default());
    let sb = Arc::new(Sandbox::new(
        chain,
        Arc::new(Dead),
        Arc::new(Dead),
        LndAccess {
            url: "x".into(),
            cert_path: "x".into(),
            macaroon_path: "x".into(),
        },
        "x:1".into(),
        h.service.clone(),
    ));
    let app = api::router(AppState {
        service: h.service.clone(),
        admin_token: ADMIN.into(),
        allow_mock: true,
        sandbox: Some(sb),
    });
    let (s, st) = admin(&app, "GET", "/sandbox/api/status", Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(st["ready"], false);
    assert_eq!(st["gateway"]["ok"], false);
    assert!(
        st["gateway"]["error"]
            .as_str()
            .unwrap()
            .contains("connection refused")
    );
    assert_eq!(st["chain"]["ok"], true);
}

#[tokio::test]
async fn bootstrap_reports_a_timeout_instead_of_hanging_when_a_node_never_syncs() {
    struct Stuck;
    #[async_trait]
    impl LnNode for Stuck {
        async fn info(&self) -> anyhow::Result<NodeInfo> {
            Ok(NodeInfo {
                pubkey: "p".into(),
                synced: false,
                height: 0,
                active_channels: 0,
                onchain_sats: 0,
                unconfirmed_sats: 0,
            })
        }
        async fn new_address(&self) -> anyhow::Result<String> {
            Ok("a".into())
        }
        async fn connect(&self, _: &str, _: &str) -> anyhow::Result<()> {
            Ok(())
        }
        async fn open_channel(&self, _: &str, _: u64, _: u64) -> anyhow::Result<()> {
            Ok(())
        }
        async fn channels(&self) -> anyhow::Result<Vec<ChannelInfo>> {
            Ok(vec![])
        }
        async fn pay_invoice(&self, _: &str) -> anyhow::Result<String> {
            Ok(String::new())
        }
        async fn send_coins(&self, _: &str, _: u64) -> anyhow::Result<String> {
            Ok(String::new())
        }
    }
    let h = harness().await;
    let mut sb = Sandbox::new(
        Arc::new(Chain::default()),
        Arc::new(Stuck),
        Arc::new(Stuck),
        LndAccess {
            url: "x".into(),
            cert_path: "x".into(),
            macaroon_path: "x".into(),
        },
        "x:1".into(),
        h.service.clone(),
    );
    sb.poll = std::time::Duration::from_millis(1);
    sb.polls = 3;
    let app = api::router(AppState {
        service: h.service.clone(),
        admin_token: ADMIN.into(),
        allow_mock: true,
        sandbox: Some(Arc::new(sb)),
    });
    let (s, r) = admin(&app, "POST", "/sandbox/api/bootstrap", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["ok"], false);
    assert!(r["error"].as_str().unwrap().contains("timed out"), "{r}");
}
