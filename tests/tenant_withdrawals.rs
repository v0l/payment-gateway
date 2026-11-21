mod common;
use async_trait::async_trait;
use axum::http::StatusCode;
use bitcoin::hashes::{Hash, sha256};
use common::*;
use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
use payment_gateway::db;
use payment_gateway::ledger::{self, AccountKind, AccountRef, Entry, Posting, TxKind, post};
use payment_gateway::payouts::{OnchainState, PaymentState, PayoutNode, PayoutPolicy, TenantNodes};
use payment_gateway::service::webhook_signature;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

fn bolt11(amount_msat: u64) -> String {
    let secret: [u8; 32] = rand::random();
    let key = bitcoin::secp256k1::SecretKey::from_slice(&[0xcd; 32]).unwrap();
    InvoiceBuilder::new(Currency::Regtest)
        .description("t".into())
        .payment_hash(sha256::Hash::from_slice(&secret).unwrap())
        .payment_secret(PaymentSecret(secret))
        .current_timestamp()
        .min_final_cltv_expiry_delta(144)
        .amount_milli_satoshis(amount_msat)
        .build_signed(|m| bitcoin::secp256k1::Secp256k1::new().sign_ecdsa_recoverable(m, &key))
        .unwrap()
        .to_string()
}

fn hash_of(invoice: &str) -> String {
    payment_gateway::providers::lightning::parse_invoice(invoice)
        .unwrap()
        .payment_hash()
}

#[derive(Default)]
struct FakeNode {
    fail: Mutex<bool>,
    pays: Mutex<Vec<String>>,
    payments: Mutex<HashMap<String, PaymentState>>,
}

#[async_trait]
impl PayoutNode for FakeNode {
    async fn pay_invoice(&self, invoice: &str, _fee_limit_msat: u64) -> anyhow::Result<()> {
        self.pays.lock().unwrap().push(invoice.to_string());
        let state = if *self.fail.lock().unwrap() {
            PaymentState::Failed("FAILURE_REASON_NO_ROUTE".into())
        } else {
            PaymentState::Succeeded {
                fee_msat: 2000,
                reference: "preimage".into(),
            }
        };
        self.payments
            .lock()
            .unwrap()
            .insert(hash_of(invoice), state);
        Ok(())
    }

    async fn payment_state(&self, hash: &str) -> anyhow::Result<PaymentState> {
        Ok(self
            .payments
            .lock()
            .unwrap()
            .get(hash)
            .cloned()
            .unwrap_or(PaymentState::NotFound))
    }

    async fn send_coins(&self, _address: &str, _sats: u64, _label: &str) -> anyhow::Result<()> {
        anyhow::bail!("unused")
    }

    async fn onchain_state(&self, _label: &str) -> anyhow::Result<OnchainState> {
        Ok(OnchainState::NotFound)
    }
}

struct FakeNodes(Arc<FakeNode>);

#[async_trait]
impl TenantNodes for FakeNodes {
    async fn connect(&self, config: &Value) -> anyhow::Result<Arc<dyn PayoutNode>> {
        assert!(config["url"].as_str().is_some());
        Ok(self.0.clone())
    }
}

async fn setup() -> (Harness, Arc<FakeNode>) {
    let h = harness().await;
    let node = Arc::new(FakeNode::default());
    h.service
        .payouts
        .set_tenant_nodes(Some(Arc::new(FakeNodes(node.clone()))));
    h.service.payouts.set_node(Some(node.clone()));
    h.service.payouts.set_policy(PayoutPolicy {
        network: bitcoin::Network::Regtest,
        sending_lease: chrono::Duration::zero(),
        retry_delay: chrono::Duration::zero(),
        ..PayoutPolicy::default()
    });
    (h, node)
}

async fn wallet_provider(h: &Harness, tenant: u64) -> u64 {
    let sealed = h
        .service
        .sealer
        .seal(
            json!({"url": "https://lnd.example:10009"})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    db::insert_provider_config(
        &h.pool,
        db::NewProviderConfig {
            tenant_id: tenant,
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
    .unwrap()
}

async fn fund(h: &Harness, tenant: u64, msat: i64, key: &str) {
    let mut tx = h.pool.begin().await.unwrap();
    post(
        &mut tx,
        &Posting::new(
            TxKind::Adjustment,
            vec![
                Entry::new(
                    AccountRef::platform(AccountKind::PlatformWallet, "BTC"),
                    -msat,
                ),
                Entry::new(
                    AccountRef::tenant(tenant, AccountKind::TenantBalance, "BTC"),
                    msat,
                ),
            ],
        ),
        key,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

async fn balance(h: &Harness, tenant: u64, kind: &str) -> i64 {
    ledger::balances(&h.pool, tenant)
        .await
        .unwrap()
        .into_iter()
        .find(|b| b.kind == kind)
        .map(|b| b.balance)
        .unwrap_or(0)
}

async fn post_withdrawal(h: &Harness, key: &str, idem: Option<&str>, body: Value) -> Reply {
    let headers: Vec<(&str, &str)> = idem.map(|i| ("idempotency-key", i)).into_iter().collect();
    send(&h.app, "POST", "/v1/withdrawals", Some(key), &headers, body).await
}

#[tokio::test]
async fn create_is_idempotent_and_reserves_once() {
    let (h, _) = setup().await;
    let t = tenant(&h).await;
    db::set_tenant_trusted(&h.pool, t.id, true).await.unwrap();
    fund(&h, t.id, 50_000_000, "f").await;
    let body = json!({"amount": 5_000_000, "currency": "BTC", "destination": bolt11(5_000_000)});

    let first = post_withdrawal(&h, &t.key, Some("k1"), body.clone()).await;
    assert_eq!(first.status, StatusCode::CREATED, "{}", first.text);
    assert_eq!(first.json["status"], "approved");
    let reserved = balance(&h, t.id, "pending_payout").await;
    assert!(reserved >= 5_000_000);

    let again = post_withdrawal(&h, &t.key, Some("k1"), body.clone()).await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(again.json["id"], first.json["id"]);
    assert_eq!(balance(&h, t.id, "pending_payout").await, reserved);

    let other = json!({"amount": 6_000_000, "destination": bolt11(6_000_000)});
    let clash = post_withdrawal(&h, &t.key, Some("k1"), other).await;
    assert_eq!(clash.status, StatusCode::CONFLICT);

    let missing = post_withdrawal(&h, &t.key, None, body).await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn custody_withdrawal_is_paid_and_webhooks_are_signed() {
    let (h, node) = setup().await;
    let t = tenant(&h).await;
    db::set_tenant_trusted(&h.pool, t.id, true).await.unwrap();
    fund(&h, t.id, 50_000_000, "f").await;
    let r = post_withdrawal(
        &h,
        &t.key,
        Some("pay"),
        json!({"amount": 5_000_000, "destination": bolt11(5_000_000)}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    h.service.run_payouts().await.unwrap();
    assert_eq!(node.pays.lock().unwrap().len(), 1);

    let id = r.json["id"].as_u64().unwrap();
    let got = send(
        &h.app,
        "GET",
        &format!("/v1/withdrawals/{id}"),
        Some(&t.key),
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(got.json["status"], "completed");
    assert!(got.json["external_id"].as_str().is_some());
    assert_eq!(balance(&h, t.id, "pending_payout").await, 0);

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
        webhook_signature(&t.webhook_secret, ts.parse().unwrap(), body)
    );
    let event: Value = serde_json::from_str(body).unwrap();
    assert_eq!(event["data"]["withdrawal"]["id"], id);
    assert_eq!(event["data"]["withdrawal"]["status"], "completed");
}

#[tokio::test]
async fn failed_custody_payment_refunds_the_reservation_and_sends_failed_webhook() {
    let (h, node) = setup().await;
    *node.fail.lock().unwrap() = true;
    let t = tenant(&h).await;
    db::set_tenant_trusted(&h.pool, t.id, true).await.unwrap();
    fund(&h, t.id, 50_000_000, "f").await;
    let r = post_withdrawal(
        &h,
        &t.key,
        Some("bad"),
        json!({"amount": 5_000_000, "destination": bolt11(5_000_000)}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    assert!(balance(&h, t.id, "tenant_balance").await < 50_000_000);
    h.service.run_payouts().await.unwrap();

    let id = r.json["id"].as_u64().unwrap();
    let got = send(
        &h.app,
        "GET",
        &format!("/v1/withdrawals/{id}"),
        Some(&t.key),
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(got.json["status"], "failed");
    assert_eq!(balance(&h, t.id, "tenant_balance").await, 50_000_000);
    assert_eq!(balance(&h, t.id, "pending_payout").await, 0);
    h.service.deliver_due().await.unwrap();
    assert!(
        t.receiver
            .types()
            .contains(&"withdrawal.failed".to_string())
    );
}

#[tokio::test]
async fn wallet_withdrawal_pays_from_the_tenant_node_without_touching_custody() {
    let (h, node) = setup().await;
    h.service.payouts.set_node(None);
    let t = tenant(&h).await;
    let pid = wallet_provider(&h, t.id).await;
    let invoice = bolt11(3_000_000);
    let r = post_withdrawal(
        &h,
        &t.key,
        Some("w1"),
        json!({"amount": 3_000_000, "destination": invoice, "provider_id": pid}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    assert_eq!(r.json["provider_id"], pid);
    assert_eq!(balance(&h, t.id, "tenant_balance").await, 0);
    h.service.run_payouts().await.unwrap();
    assert_eq!(node.pays.lock().unwrap().as_slice(), [invoice]);
    let id = r.json["id"].as_u64().unwrap();
    let w = db::get_withdrawal(&h.pool, id).await.unwrap().unwrap();
    assert_eq!(w.status, "completed", "{:?}", w.error);
    assert_eq!(balance(&h, t.id, "tenant_balance").await, 0);
    assert_eq!(balance(&h, t.id, "pending_payout").await, 0);
    assert!(ledger::reconcile(&h.pool).await.unwrap().is_empty());
    h.service.deliver_due().await.unwrap();
    assert!(t.receiver.types().contains(&"withdrawal.paid".to_string()));
}

#[tokio::test]
async fn failed_wallet_withdrawal_reports_failure_without_ledger_movement() {
    let (h, node) = setup().await;
    *node.fail.lock().unwrap() = true;
    let t = tenant(&h).await;
    let pid = wallet_provider(&h, t.id).await;
    let r = post_withdrawal(
        &h,
        &t.key,
        Some("w2"),
        json!({"amount": 3_000_000, "destination": bolt11(3_000_000), "provider_id": pid}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
    h.service.run_payouts().await.unwrap();
    let id = r.json["id"].as_u64().unwrap();
    let w = db::get_withdrawal(&h.pool, id).await.unwrap().unwrap();
    assert_eq!(w.status, "failed");
    assert_eq!(balance(&h, t.id, "tenant_balance").await, 0);
    assert_eq!(balance(&h, t.id, "pending_payout").await, 0);
    h.service.deliver_due().await.unwrap();
    assert!(
        t.receiver
            .types()
            .contains(&"withdrawal.failed".to_string())
    );
}

#[tokio::test]
async fn wallet_provider_must_belong_to_the_tenant() {
    let (h, _) = setup().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let pid = wallet_provider(&h, a.id).await;
    let r = post_withdrawal(
        &h,
        &b.key,
        Some("x"),
        json!({"amount": 3_000_000, "destination": bolt11(3_000_000), "provider_id": pid}),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.text);
}

#[tokio::test]
async fn withdrawals_are_tenant_scoped_listed_and_scope_checked() {
    let (h, _) = setup().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let pid = wallet_provider(&h, a.id).await;
    let r = post_withdrawal(
        &h,
        &a.key,
        Some("a1"),
        json!({"amount": 3_000_000, "destination": bolt11(3_000_000), "provider_id": pid}),
    )
    .await;
    let id = r.json["id"].as_u64().unwrap();
    let uri = format!("/v1/withdrawals/{id}");
    let other = send(&h.app, "GET", &uri, Some(&b.key), &[], Value::Null).await;
    assert_eq!(other.status, StatusCode::NOT_FOUND);
    let list_b = send(
        &h.app,
        "GET",
        "/v1/withdrawals",
        Some(&b.key),
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(list_b.json.as_array().unwrap().len(), 0);
    let list_a = send(
        &h.app,
        "GET",
        "/v1/withdrawals",
        Some(&a.key),
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(list_a.json.as_array().unwrap().len(), 1);

    let reader = scoped_key(&h, a.id, "payments:read").await;
    let denied = post_withdrawal(
        &h,
        &reader,
        Some("z"),
        json!({"amount": 3_000_000, "destination": bolt11(3_000_000), "provider_id": pid}),
    )
    .await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let denied_get = send(&h.app, "GET", &uri, Some(&reader), &[], Value::Null).await;
    assert_eq!(denied_get.status, StatusCode::FORBIDDEN);
    let wr = scoped_key(&h, a.id, "withdrawals:read").await;
    let ok = send(&h.app, "GET", &uri, Some(&wr), &[], Value::Null).await;
    assert_eq!(ok.status, StatusCode::OK);
}

async fn scoped_key(h: &Harness, tenant_id: u64, scopes: &str) -> String {
    let key = payment_gateway::api_keys::new_scoped_key("test");
    db::insert_api_key(
        &h.pool,
        &db::NewApiKey {
            tenant_id,
            name: scopes,
            key_hash: &payment_gateway::crypto::hash_api_key(&key),
            key_prefix: &payment_gateway::api_keys::key_prefix(&key),
            scopes,
            mode: "test",
            expires: None,
        },
    )
    .await
    .unwrap();
    key
}
