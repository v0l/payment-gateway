mod common;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::get;
use bitcoin::Network;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use common::*;
use payment_gateway::db;
use payment_gateway::providers::xpub::{ChainConfig, XpubProvider};
use payment_gateway::providers::{EventSink, Provider};
use payment_gateway::service::ProviderSink;
use payment_gateway::xpub::XpubSpec;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct Chain {
    txs: Arc<Mutex<HashMap<String, Vec<Value>>>>,
    tip: Arc<Mutex<u64>>,
}

impl Chain {
    fn put(&self, address: &str, txid: &str, sats: u64, height: Option<u64>) {
        let mut all = self.txs.lock().unwrap();
        let list = all.entry(address.to_string()).or_default();
        list.retain(|t| t["txid"] != txid);
        list.push(json!({
            "txid": txid,
            "vout": [{"scriptpubkey_address": address, "value": sats}],
            "status": {"confirmed": height.is_some(), "block_height": height}
        }));
    }
    fn set_tip(&self, h: u64) {
        *self.tip.lock().unwrap() = h;
    }
}

async fn serve(chain: Chain) -> String {
    async fn txs(State(c): State<Chain>, Path(a): Path<String>) -> axum::Json<Value> {
        axum::Json(json!(
            c.txs.lock().unwrap().get(&a).cloned().unwrap_or_default()
        ))
    }
    async fn tip(State(c): State<Chain>) -> String {
        c.tip.lock().unwrap().to_string()
    }
    let app = Router::new()
        .route("/address/{a}/txs", get(txs))
        .route("/blocks/tip/height", get(tip))
        .with_state(chain);
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", l.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    url
}

fn tpub() -> String {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Regtest, &[3u8; 32]).unwrap();
    let path = DerivationPath::from_str("m/84'/1'/0'").unwrap();
    Xpub::from_priv(&secp, &master.derive_priv(&secp, &path).unwrap()).to_string()
}

fn descriptor() -> String {
    format!("wpkh({}/0/*)", tpub())
}

struct Fixture {
    h: Harness,
    t: TenantFixture,
    chain: Chain,
    id: u64,
    spec: XpubSpec,
    provider: Arc<XpubProvider>,
    sink: Arc<dyn EventSink>,
}

async fn fixture(min_conf: u32) -> Fixture {
    let h = harness().await;
    let chain = Chain::default();
    chain.set_tip(100);
    let url = serve(chain.clone()).await;
    let cfg = ChainConfig::for_network(Network::Regtest).with_esplora(Some(url));
    h.factory.set_xpub(h.pool.clone(), cfg.clone());
    let t = tenant(&h).await;
    let config = json!({"xpub": descriptor(), "min_confirmations": min_conf});
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers", t.id),
        Some(ADMIN),
        json!({"method": "onchain", "provider_type": "xpub", "name": "xp", "config": config}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let id = r["id"].as_u64().unwrap();
    let row = db::get_provider_config(&h.pool, id).await.unwrap().unwrap();
    let provider = Arc::new(
        XpubProvider::build(h.pool.clone(), &row, &config, &cfg)
            .await
            .unwrap(),
    );
    let sink: Arc<dyn EventSink> = Arc::new(ProviderSink {
        service: h.service.clone(),
        provider_id: id,
    });
    let spec = XpubSpec::parse(&descriptor(), Network::Regtest).unwrap();
    Fixture {
        h,
        t,
        chain,
        id,
        spec,
        provider,
        sink,
    }
}

async fn start(f: &Fixture, cents: u64) -> (String, String, u64) {
    let p = new_payment(&f.h, &f.t, eur("o", cents)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, r) = attempt(&f.h, &f.t, &id, json!({"method": "onchain"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let a = &r["attempts"][0];
    (
        id,
        a["details"]["address"].as_str().unwrap().to_string(),
        a["details"]["amount_sats"].as_u64().unwrap(),
    )
}

async fn status(f: &Fixture, id: &str) -> String {
    get_payment(&f.h, &f.t, id).await["status"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn attempts_get_sequential_unique_addresses_from_the_external_chain() {
    let f = fixture(1).await;
    let (_, a0, _) = start(&f, 1000).await;
    let (_, a1, _) = start(&f, 1000).await;
    assert_eq!(a0, f.spec.address(0).unwrap().to_string());
    assert_eq!(a1, f.spec.address(1).unwrap().to_string());
    assert_eq!(db::xpub_next_index(&f.h.pool, f.id).await.unwrap(), 2);
}

#[tokio::test]
async fn concurrent_attempts_never_share_an_index() {
    let f = fixture(1).await;
    let mut tasks = vec![];
    for _ in 0..12 {
        let p = f.provider.clone();
        tasks.push(tokio::spawn(
            async move { p.receive_address(0).await.unwrap() },
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for t in tasks {
        assert!(seen.insert(t.await.unwrap()));
    }
    assert_eq!(seen.len(), 12);
    assert_eq!(db::xpub_next_index(&f.h.pool, f.id).await.unwrap(), 12);
}

#[tokio::test]
async fn mempool_then_confirmed_then_paid() {
    let f = fixture(1).await;
    let (id, addr, sats) = start(&f, 5000).await;
    assert_eq!(f.provider.poll_once(&f.sink).await.unwrap(), 0);
    assert_eq!(status(&f, &id).await, "pending");
    f.chain.put(&addr, "tx1", sats, None);
    f.provider.poll_once(&f.sink).await.unwrap();
    assert_eq!(status(&f, &id).await, "pending");
    f.chain.put(&addr, "tx1", sats, Some(100));
    f.provider.poll_once(&f.sink).await.unwrap();
    assert_eq!(status(&f, &id).await, "paid");
    assert_eq!(f.provider.poll_once(&f.sink).await.unwrap(), 0);
}

#[tokio::test]
async fn confirmation_threshold_is_per_provider() {
    let f = fixture(3).await;
    let (id, addr, sats) = start(&f, 5000).await;
    f.chain.put(&addr, "tx1", sats, Some(100));
    f.provider.poll_once(&f.sink).await.unwrap();
    assert_eq!(status(&f, &id).await, "pending");
    f.chain.set_tip(102);
    f.provider.poll_once(&f.sink).await.unwrap();
    assert_eq!(status(&f, &id).await, "paid");
}

#[tokio::test]
async fn underpayment_stays_pending_and_a_top_up_settles() {
    let f = fixture(1).await;
    let (id, addr, sats) = start(&f, 5000).await;
    f.chain.put(&addr, "tx1", sats - 500, Some(100));
    f.provider.poll_once(&f.sink).await.unwrap();
    assert_eq!(status(&f, &id).await, "pending");
    f.chain.put(&addr, "tx2", 500, Some(100));
    f.provider.poll_once(&f.sink).await.unwrap();
    assert_eq!(status(&f, &id).await, "paid");
}

#[tokio::test]
async fn overpayment_settles() {
    let f = fixture(1).await;
    let (id, addr, sats) = start(&f, 5000).await;
    f.chain.put(&addr, "tx1", sats * 2, Some(100));
    f.provider.poll_once(&f.sink).await.unwrap();
    assert_eq!(status(&f, &id).await, "paid");
}

#[tokio::test]
async fn late_payment_to_an_expired_attempt_is_still_seen() {
    let f = fixture(1).await;
    let (id, addr, sats) = start(&f, 5000).await;
    sqlx::query("update payment set expires = now() - interval 1 minute where id = ?")
        .bind(&pid(&id)[..])
        .execute(&f.h.pool)
        .await
        .unwrap();
    sqlx::query("update payment_attempt set expires = now() - interval 1 minute")
        .execute(&f.h.pool)
        .await
        .unwrap();
    f.h.service.expire_due().await.unwrap();
    assert_eq!(status(&f, &id).await, "expired");
    f.chain.put(&addr, "late", sats, Some(100));
    f.provider.poll_once(&f.sink).await.unwrap();
    assert_eq!(status(&f, &id).await, "paid");
}

#[tokio::test]
async fn addresses_stop_being_watched_after_the_grace_period() {
    let f = fixture(1).await;
    let (id, addr, sats) = start(&f, 5000).await;
    sqlx::query("update payment set expires = now() - interval 5 day where id = ?")
        .bind(&pid(&id)[..])
        .execute(&f.h.pool)
        .await
        .unwrap();
    sqlx::query("update payment_attempt set expires = now() - interval 5 day")
        .execute(&f.h.pool)
        .await
        .unwrap();
    f.h.service.expire_due().await.unwrap();
    f.chain.put(&addr, "later", sats, Some(100));
    assert_eq!(f.provider.poll_once(&f.sink).await.unwrap(), 0);
    assert_eq!(status(&f, &id).await, "expired");
}

#[tokio::test]
async fn rescan_replays_issued_addresses() {
    let f = fixture(1).await;
    let (id, addr, sats) = start(&f, 5000).await;
    f.chain.put(&addr, "tx1", sats, Some(100));
    let msg = f.provider.rescan(0, 5).await.unwrap();
    assert!(msg.contains("queued"));
    f.provider.poll_once(&f.sink).await.unwrap();
    assert_eq!(status(&f, &id).await, "paid");
    assert!(f.provider.rescan(50, 5).await.is_err());
}

#[tokio::test]
async fn refund_is_marked_manual() {
    let f = fixture(1).await;
    let (id, addr, sats) = start(&f, 5000).await;
    f.chain.put(&addr, "tx1", sats, Some(100));
    f.provider.poll_once(&f.sink).await.unwrap();
    let (s, r) = call(
        &f.h.app,
        "POST",
        &format!("/v1/payments/{id}/refund"),
        Some(&f.t.key),
        json!({"destination": addr}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "completed");
    assert_eq!(r["manual"], true);
}

#[tokio::test]
async fn network_mismatch_and_bad_keys_are_refused() {
    let f = fixture(1).await;
    let mainnet = {
        let secp = Secp256k1::new();
        let m = Xpriv::new_master(Network::Bitcoin, &[3u8; 32]).unwrap();
        Xpub::from_priv(&secp, &m).to_string()
    };
    for bad in [
        format!("wpkh({mainnet}/0/*)"),
        "garbage".to_string(),
        String::new(),
    ] {
        let (s, _) = call(
            &f.h.app,
            "POST",
            &format!("/admin/tenants/{}/providers", f.t.id),
            Some(ADMIN),
            json!({"method": "onchain", "provider_type": "xpub", "name": "bad", "config": {"xpub": bad}}),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
}

#[tokio::test]
async fn a_tenant_cannot_use_another_tenants_xpub_provider() {
    let f = fixture(1).await;
    let other = tenant(&f.h).await;
    let (s, _) = attempt(
        &f.h,
        &other,
        "whatever",
        json!({"method": "onchain", "provider_id": f.id}),
    )
    .await;
    assert_ne!(s, StatusCode::CREATED);
    let p = new_payment(&f.h, &other, eur("x", 1000)).await;
    let (s, _) = attempt(
        &f.h,
        &other,
        p["id"].as_str().unwrap(),
        json!({"method": "onchain", "provider_id": f.id}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn description_shows_derivation_and_not_the_key_material_twice() {
    let f = fixture(1).await;
    start(&f, 1000).await;
    let d = f.provider.describe().await.unwrap().unwrap();
    assert_eq!(d["next_index"], 1);
    assert_eq!(d["script"], "wpkh");
    assert_eq!(d["preview"].as_array().unwrap().len(), 3);
    assert_eq!(d["preview"][0], f.spec.address(0).unwrap().to_string());
}
