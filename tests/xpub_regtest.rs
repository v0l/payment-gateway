mod common;
use bitcoin::Network;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use common::*;
use payment_gateway::db;
use payment_gateway::providers::EventSink;
use payment_gateway::providers::xpub::{BitcoindAccess, ChainConfig, XpubProvider};
use payment_gateway::sandbox::Bitcoind;
use payment_gateway::sandbox::bitcoind::BitcoindRpc;
use payment_gateway::service::ProviderSink;
use serde_json::json;
use std::str::FromStr;
use std::sync::Arc;

fn enabled() -> bool {
    std::env::var("REGTEST_BITCOIND_URL").is_ok()
}

fn tpub() -> String {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Regtest, &[5u8; 32]).unwrap();
    let path = DerivationPath::from_str("m/84'/1'/0'").unwrap();
    Xpub::from_priv(&secp, &master.derive_priv(&secp, &path).unwrap()).to_string()
}

#[tokio::test]
async fn bitcoind_watch_only_wallet_sees_a_payment_from_the_regtest_wallet() {
    if !enabled() {
        return;
    }
    let url = std::env::var("REGTEST_BITCOIND_URL").unwrap();
    let rpc = BitcoindRpc::new(&url, "regtest", "regtest", "pgw_funder").unwrap();
    rpc.ensure_wallet().await.unwrap();
    let mine_to = rpc.new_address().await.unwrap();
    if rpc.balance_sats().await.unwrap() < 1_000_000 {
        rpc.mine(101, &mine_to).await.unwrap();
    }

    let h = harness().await;
    let chain = ChainConfig::for_network(Network::Regtest)
        .with_esplora(None)
        .with_bitcoind(Some(BitcoindAccess {
            url: url.clone(),
            user: "regtest".into(),
            pass: "regtest".into(),
        }));
    h.factory.set_xpub(h.pool.clone(), chain.clone());
    let t = tenant(&h).await;
    let config = json!({"xpub": format!("wpkh({}/0/*)", tpub()), "source": "bitcoind", "min_confirmations": 1});
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers", t.id),
        Some(ADMIN),
        json!({"method": "onchain", "provider_type": "xpub", "name": "xp", "config": config}),
    )
    .await;
    assert_eq!(s, axum::http::StatusCode::OK, "{r}");
    let id = r["id"].as_u64().unwrap();
    let row = db::get_provider_config(&h.pool, id).await.unwrap().unwrap();
    let provider = XpubProvider::build(h.pool.clone(), &row, &config, &chain)
        .await
        .unwrap();
    let sink: Arc<dyn EventSink> = Arc::new(ProviderSink {
        service: h.service.clone(),
        provider_id: id,
    });

    let p = new_payment(&h, &t, eur("o", 1000)).await;
    let pid = p["id"].as_str().unwrap().to_string();
    let (_, a) = attempt(&h, &t, &pid, json!({"method": "onchain"})).await;
    let address = a["attempts"][0]["details"]["address"].as_str().unwrap();
    let sats = a["attempts"][0]["details"]["amount_sats"].as_u64().unwrap();

    provider.poll_once(&sink).await.unwrap();
    rpc.send(address, sats).await.unwrap();
    provider.poll_once(&sink).await.unwrap();
    assert_eq!(get_payment(&h, &t, &pid).await["status"], "pending");
    rpc.mine(1, &mine_to).await.unwrap();
    provider.poll_once(&sink).await.unwrap();
    assert_eq!(get_payment(&h, &t, &pid).await["status"], "paid");
}
