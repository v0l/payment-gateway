mod common;
use common::*;
use payment_gateway::db;
use payment_gateway::ledger::{AccountKind, AccountRef, Entry, Posting, TxKind, post};
use payment_gateway::payouts::{DestinationKind, LndPayoutNode, PayoutPolicy};
use payment_gateway::sandbox::{LndAccess, Sandbox, SandboxConfig};
use payment_gateway::service::WithdrawalRequest;
use payments_rs::lightning::{AddInvoiceRequest, LightningNode, LndNode, setup_crypto_provider};
use std::path::Path;
use std::sync::Arc;

fn enabled() -> bool {
    std::env::var("REGTEST").is_ok_and(|v| v == "1")
}

fn dir() -> String {
    std::env::var("REGTEST_DIR").unwrap_or_else(|_| "/tmp/pgw-reg".into())
}

fn access(name: &str, port: u16) -> LndAccess {
    let dir = dir();
    LndAccess {
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

async fn wait_for<F, Fut>(what: &str, mut f: F)
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = bool>,
{
    for _ in 0..60 {
        if f().await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn real_lnd_pays_out_over_lightning_and_on_chain() {
    if !enabled() {
        eprintln!("skipped: set REGTEST=1 with `docker compose -f regtest/compose.yaml up -d`");
        return;
    }
    let h = harness().await;
    let cfg = config();
    let sb = Sandbox::from_config(cfg.clone(), h.service.clone()).unwrap();
    sb.bootstrap().await.expect("bootstrap regtest");

    let gateway = &cfg.gateway;
    let node = LndPayoutNode::connect(
        &gateway.url,
        Path::new(&gateway.cert_path),
        Path::new(&gateway.macaroon_path),
        2,
    )
    .await
    .unwrap();
    h.service.payouts.set_node(Some(Arc::new(node)));
    h.service.payouts.set_policy(PayoutPolicy {
        network: bitcoin::Network::Regtest,
        min_amount: 1_000_000,
        sending_lease: chrono::Duration::seconds(30),
        ..PayoutPolicy::default()
    });

    setup_crypto_provider();
    let customer_cfg = &cfg.customer;
    let customer = LndNode::new(
        &customer_cfg.url,
        Path::new(&customer_cfg.cert_path),
        Path::new(&customer_cfg.macaroon_path),
    )
    .await
    .unwrap();

    let t = tenant(&h).await;
    let mut tx = h.pool.begin().await.unwrap();
    post(
        &mut tx,
        &Posting::new(
            TxKind::Adjustment,
            vec![
                Entry::new(
                    AccountRef::platform(AccountKind::PlatformWallet, "BTC"),
                    -500_000_000,
                ),
                Entry::new(
                    AccountRef::tenant(t.id, AccountKind::TenantBalance, "BTC"),
                    500_000_000,
                ),
            ],
        ),
        "fund",
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let tr = h.service.tenant(t.id).await.unwrap();

    let invoice = customer
        .add_invoice(AddInvoiceRequest {
            amount: 20_000_000,
            memo: Some("payout".into()),
            expire: Some(600),
        })
        .await
        .unwrap();
    let ln = h
        .service
        .request_withdrawal(
            &tr,
            WithdrawalRequest {
                amount: 20_000_000,
                invoice: Some(invoice.pr()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(ln.status, "approved");
    h.service.run_payouts().await.unwrap();
    let done = db::get_withdrawal(&h.pool, ln.id).await.unwrap().unwrap();
    assert_eq!(done.status, "completed", "{:?}", done.error);
    assert_eq!(
        done.external_id.as_deref(),
        Some(invoice.payment_hash().as_str())
    );
    h.service.run_payouts().await.unwrap();
    assert_eq!(
        db::get_withdrawal(&h.pool, ln.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "completed"
    );

    let address = sb.customer.new_address().await.unwrap();
    let dest = h
        .service
        .add_destination(&tr, DestinationKind::Onchain, "customer", &address, None)
        .await
        .unwrap();
    sqlx::query("update payout_destination set verified = 1, usable_from = ? where id = ?")
        .bind(chrono::Utc::now() - chrono::Duration::hours(1))
        .bind(dest.id)
        .execute(&h.pool)
        .await
        .unwrap();
    let before = sb.customer.info().await.unwrap();
    let oc = h
        .service
        .request_withdrawal(
            &tr,
            WithdrawalRequest {
                amount: 50_000_000,
                destination_id: Some(dest.id),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    h.service.run_payouts().await.unwrap();
    let sent = db::get_withdrawal(&h.pool, oc.id).await.unwrap().unwrap();
    assert_eq!(sent.status, "completed", "{:?}", sent.error);
    let txid = sent.external_id.clone().unwrap();
    assert_eq!(txid.len(), 64);
    sb.mine(1).await.unwrap();
    wait_for("customer to see the on-chain payout", || async {
        sb.customer
            .info()
            .await
            .map(|i| i.onchain_sats >= before.onchain_sats + 50_000)
            .unwrap_or(false)
    })
    .await;
    h.service.run_payouts().await.unwrap();
    assert_eq!(
        db::get_withdrawal(&h.pool, oc.id)
            .await
            .unwrap()
            .unwrap()
            .external_id,
        Some(txid)
    );
    let pending = payment_gateway::ledger::balances(&h.pool, t.id)
        .await
        .unwrap()
        .into_iter()
        .find(|b| b.kind == "pending_payout")
        .map(|b| b.balance)
        .unwrap_or(0);
    assert_eq!(pending, 0);
    assert!(
        payment_gateway::ledger::reconcile(&h.pool)
            .await
            .unwrap()
            .is_empty()
    );
}
