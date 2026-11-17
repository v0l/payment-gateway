mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::api::{self, AppState};
use payment_gateway::sandbox::{Sandbox, SandboxConfig};
use serde_json::{Value, json};
use std::sync::Arc;

fn enabled() -> bool {
    std::env::var("REGTEST").is_ok_and(|v| v == "1")
}

fn config() -> SandboxConfig {
    let dir = std::env::var("REGTEST_DIR").unwrap_or_else(|_| "/tmp/pgw-reg".into());
    let access = |name: &str, port: u16| payment_gateway::sandbox::LndAccess {
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

async fn wait_status(h: &Harness, t: &TenantKey, id: &str, want: &str) -> String {
    for _ in 0..90 {
        let (_, p) = call(
            &h.app,
            "GET",
            &format!("/v1/payments/{id}"),
            Some(&t.0),
            Value::Null,
        )
        .await;
        if p["status"] == want {
            return want.to_string();
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let (_, p) = call(
        &h.app,
        "GET",
        &format!("/v1/payments/{id}"),
        Some(&t.0),
        Value::Null,
    )
    .await;
    p["status"].as_str().unwrap_or("?").to_string()
}

struct TenantKey(String);

#[tokio::test]
async fn real_lnd_lightning_and_onchain_payments_settle_through_the_gateway() {
    if !enabled() {
        eprintln!("skipped: set REGTEST=1 with `docker compose -f regtest/compose.yaml up -d`");
        return;
    }
    let h = harness().await;
    let sb = Sandbox::from_config(config(), h.service.clone()).unwrap();
    let app = api::router(AppState {
        service: h.service.clone(),
        admin_token: ADMIN.into(),
        allow_mock: true,
        sandbox: Some(sb.clone()),
    });
    let log = sb
        .bootstrap()
        .await
        .expect("bootstrap against real regtest");
    let key = log
        .iter()
        .find_map(|l| l.split("api key ").nth(1))
        .map(|k| k.split_whitespace().next().unwrap().to_string())
        .unwrap_or_else(|| futures_key(&sb));
    let t = TenantKey(key);
    let _workers =
        payment_gateway::worker::spawn(h.service.clone(), std::time::Duration::from_millis(200));
    let h = Harness { app, ..h };

    let (s, ln) = call(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.0),
        json!({"reference": "e2e-ln", "amount": 1000, "currency": "EUR"}),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{ln}");
    let ln_id = ln["id"].as_str().unwrap();
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{ln_id}/attempts"),
        Some(&t.0),
        json!({"method": "lightning"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let bolt11 = r["attempts"][0]["details"]["bolt11"].as_str().unwrap();
    assert!(
        bolt11.starts_with("lnbcrt"),
        "regtest invoice, got {bolt11}"
    );
    sb.customer_pay_invoice(bolt11)
        .await
        .expect("customer pays over the real channel");
    assert_eq!(wait_status(&h, &t, ln_id, "paid").await, "paid");

    let (_, oc) = call(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.0),
        json!({"reference": "e2e-oc", "amount": 1000, "currency": "EUR"}),
    )
    .await;
    let oc_id = oc["id"].as_str().unwrap();
    let (_, r) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{oc_id}/attempts"),
        Some(&t.0),
        json!({"method": "onchain"}),
    )
    .await;
    let address = r["attempts"][0]["details"]["address"].as_str().unwrap();
    assert!(address.starts_with("bcrt1"), "{address}");
    let sats = r["attempts"][0]["details"]["amount_sats"].as_u64().unwrap();

    let paid_short = sb.customer_pay_invoice("lnbcrt1invalid").await;
    assert!(paid_short.is_err());

    let pid = payment_gateway::db::parse_payment_id(oc_id).unwrap();
    let message = sb
        .pay_from_customer(&pid)
        .await
        .expect("customer sends on-chain");
    assert!(message.contains(&sats.to_string()), "{message}");
    assert_eq!(wait_status(&h, &t, oc_id, "paid").await, "paid");
}

fn futures_key(sb: &Arc<Sandbox>) -> String {
    let rt = tokio::runtime::Handle::current();
    tokio::task::block_in_place(|| rt.block_on(sb.rotate_key())).unwrap()
}
