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

fn unroutable_invoice(amount_msat: u64) -> String {
    use bitcoin::hashes::{Hash, sha256};
    use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
    let secret: [u8; 32] = rand::random();
    let key = bitcoin::secp256k1::SecretKey::from_slice(&[0xcd; 32]).unwrap();
    InvoiceBuilder::new(Currency::Regtest)
        .description("nowhere".into())
        .payment_hash(sha256::Hash::from_slice(&secret).unwrap())
        .payment_secret(PaymentSecret(secret))
        .current_timestamp()
        .min_final_cltv_expiry_delta(144)
        .amount_milli_satoshis(amount_msat)
        .build_signed(|m| bitcoin::secp256k1::Secp256k1::new().sign_ecdsa_recoverable(m, &key))
        .unwrap()
        .to_string()
}

async fn balance_of(h: &Harness, tenant: u64, kind: &str) -> i64 {
    payment_gateway::ledger::balances(&h.pool, tenant)
        .await
        .unwrap()
        .into_iter()
        .find(|b| b.kind == kind)
        .map(|b| b.balance)
        .unwrap_or(0)
}

#[tokio::test]
async fn real_lnd_pays_a_tenant_wallet_withdrawal_and_reports_failures() {
    if !enabled() {
        eprintln!("skipped: set REGTEST=1 with `docker compose -f regtest/compose.yaml up -d`");
        return;
    }
    let h = harness().await;
    let cfg = config();
    let sb = Sandbox::from_config(cfg.clone(), h.service.clone()).unwrap();
    sb.bootstrap().await.expect("bootstrap regtest");
    h.service
        .payouts
        .set_tenant_nodes(Some(Arc::new(payment_gateway::payouts::LndTenantNodes {
            sat_per_vbyte: 2,
        })));
    h.service.payouts.set_policy(PayoutPolicy {
        network: bitcoin::Network::Regtest,
        min_amount: 1_000_000,
        sending_lease: chrono::Duration::seconds(30),
        retry_delay: chrono::Duration::zero(),
        max_attempts: 1,
        ..PayoutPolicy::default()
    });

    setup_crypto_provider();
    let customer = LndNode::new(
        &cfg.customer.url,
        Path::new(&cfg.customer.cert_path),
        Path::new(&cfg.customer.macaroon_path),
    )
    .await
    .unwrap();

    let t = tenant(&h).await;
    let sealed = h
        .service
        .sealer
        .seal(
            serde_json::json!({
                "url": cfg.gateway.url,
                "cert_path": cfg.gateway.cert_path,
                "macaroon_path": cfg.gateway.macaroon_path,
            })
            .to_string()
            .as_bytes(),
        )
        .unwrap();
    let provider_id = db::insert_provider_config(
        &h.pool,
        db::NewProviderConfig {
            tenant_id: t.id,
            method: "lightning",
            provider_type: "lnd",
            name: "own lnd",
            sealed_config: &sealed,
            fee_rate: None,
            fee_base: None,
            fee_currency: None,
        },
    )
    .await
    .unwrap();

    let invoice = customer
        .add_invoice(AddInvoiceRequest {
            amount: 20_000_000,
            memo: Some("wallet payout".into()),
            expire: Some(600),
        })
        .await
        .unwrap();
    let created = common::send(
        &h.app,
        "POST",
        "/v1/withdrawals",
        Some(&t.key),
        &[("idempotency-key", "wallet-1")],
        serde_json::json!({"amount": 20_000_000, "destination": invoice.pr(), "provider_id": provider_id}),
    )
    .await;
    assert_eq!(created.status, 201, "{}", created.text);
    h.service.run_payouts().await.unwrap();
    let id = created.json["id"].as_u64().unwrap();
    let done = db::get_withdrawal(&h.pool, id).await.unwrap().unwrap();
    assert_eq!(done.status, "completed", "{:?}", done.error);
    assert_eq!(
        done.external_id.as_deref(),
        Some(invoice.payment_hash().as_str())
    );
    assert_eq!(balance_of(&h, t.id, "tenant_balance").await, 0);
    assert_eq!(balance_of(&h, t.id, "pending_payout").await, 0);

    h.service.deliver_due().await.unwrap();
    let delivered = t.receiver.got.lock().unwrap().clone();
    let paid: Vec<_> = delivered
        .iter()
        .filter(|(_, b)| b.contains("\"withdrawal.paid\""))
        .collect();
    assert_eq!(paid.len(), 1);
    let (headers, body) = paid[0];
    let sig = headers["x-gateway-signature"].to_str().unwrap();
    let (ts, v1) = sig.strip_prefix("t=").unwrap().split_once(",v1=").unwrap();
    assert_eq!(
        v1,
        payment_gateway::service::webhook_signature(&t.webhook_secret, ts.parse().unwrap(), body)
    );

    let bad = common::send(
        &h.app,
        "POST",
        "/v1/withdrawals",
        Some(&t.key),
        &[("idempotency-key", "wallet-2")],
        serde_json::json!({"amount": 20_000_000, "destination": unroutable_invoice(20_000_000), "provider_id": provider_id}),
    )
    .await;
    assert_eq!(bad.status, 201, "{}", bad.text);
    h.service.run_payouts().await.unwrap();
    let failed_id = bad.json["id"].as_u64().unwrap();
    let failed = db::get_withdrawal(&h.pool, failed_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(failed.status, "failed", "{:?}", failed.error);
    assert_eq!(balance_of(&h, t.id, "tenant_balance").await, 0);
    assert_eq!(balance_of(&h, t.id, "pending_payout").await, 0);
    h.service.deliver_due().await.unwrap();
    assert!(
        t.receiver
            .types()
            .contains(&"withdrawal.failed".to_string())
    );
    assert!(
        payment_gateway::ledger::reconcile(&h.pool)
            .await
            .unwrap()
            .is_empty()
    );
}
