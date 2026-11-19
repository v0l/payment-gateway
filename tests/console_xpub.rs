mod common;
use axum::http::StatusCode;
use bitcoin::Network;
use bitcoin::bip32::{DerivationPath, Xpriv, Xpub};
use bitcoin::secp256k1::Secp256k1;
use common::console::*;
use common::*;
use payment_gateway::providers::xpub::ChainConfig;
use serde_json::json;
use std::str::FromStr;

fn zpub_descriptor() -> String {
    let secp = Secp256k1::new();
    let master = Xpriv::new_master(Network::Regtest, &[8u8; 32]).unwrap();
    let path = DerivationPath::from_str("m/84'/1'/0'").unwrap();
    let x = Xpub::from_priv(&secp, &master.derive_priv(&secp, &path).unwrap());
    format!("wpkh({x}/0/*)")
}

async fn setup() -> (Harness, Client, Project, u64) {
    let h = harness().await;
    h.factory.set_xpub(
        h.pool.clone(),
        ChainConfig::for_network(Network::Regtest).with_esplora(Some("http://127.0.0.1:1".into())),
    );
    let c = signed_in(&h, "owner@example.com", false).await;
    let p = project(&c, "shop").await;
    let (s, r) = c
        .req(
            "POST",
            &format!("/projects/{}/providers", p.id),
            json!({"method": "onchain", "provider_type": "xpub", "name": "mine", "config": {"xpub": zpub_descriptor()}}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["preview_addresses"].as_array().unwrap().len(), 3);
    assert!(
        r["preview_addresses"][0]
            .as_str()
            .unwrap()
            .starts_with("bcrt1q")
    );
    let id = r["id"].as_u64().unwrap();
    (h, c, p, id)
}

#[tokio::test]
async fn adding_a_key_returns_three_addresses_and_refuses_the_wrong_network() {
    let (_h, c, p, _) = setup().await;
    let mainnet = {
        let secp = Secp256k1::new();
        let m = Xpriv::new_master(Network::Bitcoin, &[8u8; 32]).unwrap();
        Xpub::from_priv(&secp, &m).to_string()
    };
    let (s, r) = c
        .req(
            "POST",
            &format!("/projects/{}/providers", p.id),
            json!({"method": "onchain", "provider_type": "xpub", "name": "main", "config": {"xpub": format!("wpkh({mainnet}/0/*)")}}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
}

#[tokio::test]
async fn the_key_is_not_a_secret_but_the_list_never_returns_private_material() {
    let (_h, c, p, _) = setup().await;
    let (_, list) = c
        .req("GET", &format!("/projects/{}/providers", p.id), json!(null))
        .await;
    assert_eq!(list[0]["provider_type"], "xpub");
    assert!(!list.to_string().contains("xprv"));
}

#[tokio::test]
async fn status_and_rescan_work_for_the_owner_and_are_hidden_from_other_projects() {
    let (h, c, p, id) = setup().await;
    let (s, st) = c
        .req(
            "GET",
            &format!("/projects/{}/providers/{id}/xpub", p.id),
            json!(null),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{st}");
    assert_eq!(st["next_index"], 0);
    assert_eq!(st["chain"]["ok"], false);
    assert_eq!(st["preview"].as_array().unwrap().len(), 3);

    let (s, r) = c
        .req(
            "POST",
            &format!("/projects/{}/providers/{id}/rescan", p.id),
            json!({"from": 0, "count": 10}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (s, _) = c
        .req(
            "POST",
            &format!("/projects/{}/providers/{id}/rescan", p.id),
            json!({"from": 500, "count": 10}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let other = signed_in(&h, "other@example.com", false).await;
    let q = project(&other, "other").await;
    for (m, path, body) in [
        (
            "GET",
            format!("/projects/{}/providers/{id}/xpub", q.id),
            json!(null),
        ),
        (
            "POST",
            format!("/projects/{}/providers/{id}/rescan", q.id),
            json!({"from": 0, "count": 1}),
        ),
    ] {
        let (s, _) = other.req(m, &path, body).await;
        assert_eq!(s, StatusCode::NOT_FOUND);
    }
    let (s, _) = other
        .req(
            "GET",
            &format!("/projects/{}/providers/{id}/xpub", p.id),
            json!(null),
        )
        .await;
    assert_ne!(s, StatusCode::OK);
}

#[tokio::test]
async fn customer_projects_cannot_aim_the_chain_source_at_internal_hosts() {
    let (_h, c, p, _) = setup().await;
    let (s, r) = c
        .req(
            "POST",
            &format!("/projects/{}/providers", p.id),
            json!({"method": "onchain", "provider_type": "xpub", "name": "ssrf", "config": {"xpub": zpub_descriptor(), "esplora_url": "https://127.0.0.1/api"}}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
}
