mod common;
use async_trait::async_trait;
use payment_gateway::providers::lightning::LightningProvider;
use payment_gateway::providers::{AttemptRequest, Provider};
use payment_gateway::ssrf::{Resolver, pin_url, pin_url_allowing};
use serde_json::json;
use std::net::{IpAddr, SocketAddr};

struct Fixed(Vec<SocketAddr>);

#[async_trait]
impl Resolver for Fixed {
    async fn lookup(&self, _host: &str, _port: u16) -> anyhow::Result<Vec<SocketAddr>> {
        Ok(self.0.clone())
    }
}

fn enabled() -> bool {
    std::env::var("REGTEST").is_ok_and(|v| v == "1")
}

#[tokio::test]
async fn lnd_provider_connects_through_the_pinned_address_and_creates_an_invoice() {
    if !enabled() {
        eprintln!("skipped: set REGTEST=1 with the regtest stack up");
        return;
    }
    let dir = std::env::var("REGTEST_DIR").unwrap_or_else(|_| "/tmp/pgw-reg".into());
    let regtest_ip: IpAddr = "127.0.0.1".parse().unwrap();
    let resolver = Fixed(vec![SocketAddr::new(regtest_ip, 10009)]);
    let url = "https://lnd-gateway.test:10009";

    assert!(pin_url(&resolver, url).await.is_err());

    let pinned = pin_url_allowing(&resolver, url, &[regtest_ip])
        .await
        .unwrap();
    assert_eq!(pinned, "https://127.0.0.1:10009/");

    let cert = std::fs::read_to_string(format!("{dir}/gateway.cert")).unwrap();
    let macaroon = hex::encode(std::fs::read(format!("{dir}/gateway.macaroon")).unwrap());
    let config = json!({"url": pinned, "cert": cert, "macaroon": macaroon});
    let provider = LightningProvider::lnd(&config)
        .await
        .expect("connect to the pinned address");
    let created = provider
        .create(AttemptRequest {
            payment_id: [1u8; 32],
            amount: 1_000_000,
            currency: "BTC".into(),
            description: "pinned".into(),
            expires: chrono::Utc::now() + chrono::Duration::minutes(10),
            success_url: None,
            cancel_url: None,
            customer_ref: None,
            customer_email: None,
            option: None,
            save_method: false,
            receive: None,
        })
        .await
        .expect("invoice created over the pinned connection");
    let bolt11 = created.details["bolt11"].as_str().unwrap();
    assert!(bolt11.starts_with("lnbcrt"), "{bolt11}");
}
