mod common;
use axum::http::StatusCode;
use common::*;
use fedimint_tonic_lnd::lnrpc;
use payment_gateway::sandbox::{Sandbox, SandboxConfig};
use serde_json::{Value, json};

fn enabled() -> bool {
    std::env::var("REGTEST").is_ok_and(|v| v == "1")
}

fn dir() -> String {
    std::env::var("REGTEST_DIR").unwrap_or_else(|_| "/tmp/pgw-reg".into())
}

fn access(name: &str, port: u16) -> payment_gateway::sandbox::LndAccess {
    let dir = dir();
    payment_gateway::sandbox::LndAccess {
        url: format!("https://localhost:{port}"),
        cert_path: format!("{dir}/{name}.cert"),
        macaroon_path: format!("{dir}/{name}.macaroon"),
    }
}

fn config() -> SandboxConfig {
    SandboxConfig {
        bitcoind_url: "http://127.0.0.1:18443".into(),
        bitcoind_user: "regtest".into(),
        bitcoind_pass: "regtest".into(),
        gateway: access("gateway", 10009),
        customer: access("customer", 10010),
        gateway_p2p: "lnd-gateway:9735".into(),
    }
}

async fn lnd_with(macaroon: &str) -> fedimint_tonic_lnd::Client {
    let dir = dir();
    fedimint_tonic_lnd::connect(
        "https://localhost:10009".to_string(),
        format!("{dir}/gateway.cert"),
        format!("{dir}/{macaroon}.macaroon"),
    )
    .await
    .expect("connect")
}

#[tokio::test]
async fn receive_macaroon_takes_lightning_and_onchain_payments_and_cannot_spend() {
    if !enabled() {
        eprintln!("skipped: set REGTEST=1, start regtest and run regtest/scripts/macaroons.sh");
        return;
    }
    let h = harness().await;
    let sb = Sandbox::from_config(config(), h.service.clone()).unwrap();
    sb.bootstrap().await.expect("bootstrap");

    let t = tenant(&h).await;
    for method in ["lightning", "onchain"] {
        let (s, r) = call(
            &h.app,
            "POST",
            &format!("/admin/tenants/{}/providers", t.id),
            Some(ADMIN),
            json!({
                "method": method, "provider_type": "lnd", "name": method,
                "config": {
                    "url": "https://localhost:10009",
                    "cert_path": format!("{}/gateway.cert", dir()),
                    "macaroon_path": format!("{}/gateway-receive.macaroon", dir()),
                },
            }),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{r}");
    }
    let _workers =
        payment_gateway::worker::spawn(h.service.clone(), std::time::Duration::from_millis(200));

    let p = new_payment(&h, &t, eur("macaroon-ln", 1000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, r) = attempt(&h, &t, &id, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let bolt11 = r["attempts"][0]["details"]["bolt11"].as_str().unwrap();
    assert!(bolt11.starts_with("lnbcrt"), "{bolt11}");
    sb.customer_pay_invoice(bolt11)
        .await
        .expect("customer pays");
    let mut status = Value::Null;
    for _ in 0..60 {
        status = get_payment(&h, &t, &id).await["status"].clone();
        if status == "paid" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert_eq!(status, "paid");

    let p = new_payment(&h, &t, eur("macaroon-oc", 1000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, r) = attempt(&h, &t, &id, json!({"method": "onchain"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert!(
        r["attempts"][0]["details"]["address"]
            .as_str()
            .unwrap()
            .starts_with("bcrt1")
    );

    let mut receive = lnd_with("gateway-receive").await;
    let spend = receive
        .lightning()
        .send_coins(lnrpc::SendCoinsRequest {
            addr: "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080".into(),
            amount: 1000,
            ..Default::default()
        })
        .await;
    let err = spend.expect_err("receive macaroon must not spend");
    assert!(err.message().contains("permission denied"), "{err}");

    let mut payout = lnd_with("gateway-payout").await;
    let invoice = payout
        .lightning()
        .add_invoice(lnrpc::Invoice {
            value: 10,
            ..Default::default()
        })
        .await;
    let err = invoice.expect_err("payout macaroon must not create invoices");
    assert!(err.message().contains("permission denied"), "{err}");
}
