mod common;
use async_trait::async_trait;
use axum::http::StatusCode;
use axum::routing::get;
use bitcoin::hashes::{Hash, sha256};
use common::*;
use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
use payment_gateway::accounts;
use payment_gateway::db::{self, Tenant};
use payment_gateway::ledger::{self, AccountKind, AccountRef, Entry, Posting, TxKind, post};
use payment_gateway::payouts::{
    DestinationKind, OnchainState, PaymentState, PayoutNode, PayoutPolicy,
};
use payment_gateway::service::{Error, WithdrawalRequest, verify_challenge};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
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

fn regtest_address() -> String {
    let secp = bitcoin::secp256k1::Secp256k1::new();
    let sk = bitcoin::secp256k1::SecretKey::from_slice(&rand::random::<[u8; 32]>()).unwrap();
    let pk = bitcoin::PublicKey::new(sk.public_key(&secp));
    bitcoin::Address::p2wpkh(
        &bitcoin::CompressedPublicKey::try_from(pk).unwrap(),
        bitcoin::Network::Regtest,
    )
    .to_string()
}

#[derive(Default)]
struct FakeNode {
    pay_calls: Mutex<Vec<String>>,
    payments: Mutex<HashMap<String, PaymentState>>,
    sends: Mutex<Vec<(String, u64, String)>>,
    onchain: Mutex<HashMap<String, (String, u64)>>,
    reject_pay: AtomicBool,
    lose_pay_result: AtomicBool,
    lookup_down: AtomicBool,
    fee_msat: Mutex<u64>,
}

impl FakeNode {
    fn pays(&self) -> usize {
        self.pay_calls.lock().unwrap().len()
    }
}

#[async_trait]
impl PayoutNode for FakeNode {
    async fn pay_invoice(&self, invoice: &str, _fee_limit_msat: u64) -> anyhow::Result<()> {
        if self.reject_pay.load(Ordering::SeqCst) {
            anyhow::bail!("no route");
        }
        self.pay_calls.lock().unwrap().push(invoice.to_string());
        let fee = *self.fee_msat.lock().unwrap();
        self.payments.lock().unwrap().insert(
            hash_of(invoice),
            PaymentState::Succeeded {
                fee_msat: fee,
                reference: "preimage".into(),
            },
        );
        if self.lose_pay_result.load(Ordering::SeqCst) {
            anyhow::bail!("connection lost");
        }
        Ok(())
    }

    async fn payment_state(&self, hash: &str) -> anyhow::Result<PaymentState> {
        if self.lookup_down.load(Ordering::SeqCst) {
            anyhow::bail!("lnd unreachable");
        }
        Ok(self
            .payments
            .lock()
            .unwrap()
            .get(hash)
            .cloned()
            .unwrap_or(PaymentState::NotFound))
    }

    async fn send_coins(&self, address: &str, sats: u64, label: &str) -> anyhow::Result<()> {
        self.sends
            .lock()
            .unwrap()
            .push((address.into(), sats, label.into()));
        self.onchain
            .lock()
            .unwrap()
            .insert(label.into(), (format!("tx-{label}"), 154));
        Ok(())
    }

    async fn onchain_state(&self, label: &str) -> anyhow::Result<OnchainState> {
        Ok(match self.onchain.lock().unwrap().get(label) {
            Some((txid, fee)) => OnchainState::Sent {
                txid: txid.clone(),
                fee_sat: *fee,
            },
            None => OnchainState::NotFound,
        })
    }
}

fn policy() -> PayoutPolicy {
    PayoutPolicy {
        network: bitcoin::Network::Regtest,
        sending_lease: chrono::Duration::zero(),
        retry_delay: chrono::Duration::zero(),
        allow_private_lnurl: true,
        ..PayoutPolicy::default()
    }
}

async fn setup() -> (Harness, Arc<FakeNode>) {
    let h = harness().await;
    let node = Arc::new(FakeNode::default());
    h.service.payouts.set_node(Some(node.clone()));
    h.service.payouts.set_policy(policy());
    (h, node)
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

async fn tenant_row(h: &Harness, id: u64) -> Tenant {
    h.service.tenant(id).await.unwrap()
}

async fn untrust(h: &Harness, id: u64) {
    db::set_tenant_trusted(&h.pool, id, false).await.unwrap();
}

fn invoice_req(amount: u64) -> WithdrawalRequest {
    WithdrawalRequest {
        amount,
        invoice: Some(bolt11(amount)),
        ..Default::default()
    }
}

async fn status_of(h: &Harness, id: u64) -> String {
    db::get_withdrawal(&h.pool, id)
        .await
        .unwrap()
        .unwrap()
        .status
}

async fn ready_destination(h: &Harness, t: &Tenant, kind: DestinationKind, value: &str) -> u64 {
    let d = h
        .service
        .add_destination(t, kind, "main", value, None)
        .await
        .unwrap();
    sqlx::query("update payout_destination set verified = 1, usable_from = ? where id = ?")
        .bind(chrono::Utc::now() - chrono::Duration::hours(1))
        .bind(d.id)
        .execute(&h.pool)
        .await
        .unwrap();
    d.id
}

#[tokio::test]
async fn request_reserves_and_cancel_releases() {
    let (h, _) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 5_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let w = h
        .service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await
        .unwrap();
    assert_eq!(w.status, "approved");
    let reserve = (w.amount + w.max_fee) as i64;
    assert_eq!(balance(&h, t.id, "pending_payout").await, reserve);
    assert_eq!(
        balance(&h, t.id, "tenant_balance").await,
        5_000_000 - reserve
    );
    untrust(&h, t.id).await;
    let tr = tenant_row(&h, t.id).await;
    let w2 = h
        .service
        .request_withdrawal(&tr, invoice_req(1_000_000))
        .await
        .unwrap();
    assert_eq!(w2.status, "requested");
    let cancelled = h.service.cancel_withdrawal(&tr, w2.id).await.unwrap();
    assert_eq!(cancelled.status, "cancelled");
    assert_eq!(balance(&h, t.id, "pending_payout").await, reserve);
    let again = h.service.cancel_withdrawal(&tr, w2.id).await;
    assert!(matches!(again, Err(Error::Conflict(_))));
    assert!(ledger::reconcile(&h.pool).await.unwrap().is_empty());
    let findings = payment_gateway::ops::integrity::check(&h.pool)
        .await
        .unwrap();
    assert!(findings.is_empty(), "integrity findings: {findings:?}");
}

#[tokio::test]
async fn cannot_withdraw_more_than_balance_or_below_minimum() {
    let (h, _) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 2_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let r = h
        .service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await;
    assert!(matches!(r, Err(Error::Conflict(m)) if m.contains("insufficient")));
    let r = h.service.request_withdrawal(&tr, invoice_req(5_000)).await;
    assert!(matches!(r, Err(Error::Invalid(_))));
    assert_eq!(balance(&h, t.id, "tenant_balance").await, 2_000_000);
    let (n,): (i64,) = sqlx::query_as("select count(*) from withdrawal")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn twenty_parallel_requests_cannot_overdraw() {
    let (h, _) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 10_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let mut joins = vec![];
    for _ in 0..20 {
        let s = h.service.clone();
        let tr = tr.clone();
        joins.push(tokio::spawn(async move {
            s.request_withdrawal(&tr, invoice_req(1_000_000)).await
        }));
    }
    let mut ok = 0;
    for j in joins {
        if j.await.unwrap().is_ok() {
            ok += 1;
        }
    }
    assert_eq!(ok, 9);
    assert!(balance(&h, t.id, "tenant_balance").await >= 0);
    assert_eq!(
        balance(&h, t.id, "tenant_balance").await + balance(&h, t.id, "pending_payout").await,
        10_000_000
    );
    assert!(ledger::reconcile(&h.pool).await.unwrap().is_empty());
    let findings = payment_gateway::ops::integrity::check(&h.pool)
        .await
        .unwrap();
    assert!(findings.is_empty(), "integrity findings: {findings:?}");
}

#[tokio::test]
async fn daily_maximum_is_enforced() {
    let (h, _) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 50_000_000, "f").await;
    db::set_payout_limits(
        &h.pool,
        t.id,
        db::PayoutLimitRow {
            daily_max: Some(3_000_000),
            auto_threshold: None,
        },
    )
    .await
    .unwrap();
    let tr = tenant_row(&h, t.id).await;
    h.service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await
        .unwrap();
    let r = h
        .service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await;
    assert!(matches!(r, Err(Error::LimitExceeded(l)) if l.name == "payout_daily_max"));
    h.service
        .request_withdrawal(&tr, invoice_req(1_000_000))
        .await
        .unwrap();
}

#[tokio::test]
async fn auto_approval_depends_on_trust_and_threshold() {
    let (h, _) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 100_000_000, "f").await;
    db::set_payout_limits(
        &h.pool,
        t.id,
        db::PayoutLimitRow {
            daily_max: None,
            auto_threshold: Some(2_000_000),
        },
    )
    .await
    .unwrap();
    let tr = tenant_row(&h, t.id).await;
    let small = h
        .service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await
        .unwrap();
    assert_eq!(
        (small.status.as_str(), small.auto_approved),
        ("approved", true)
    );
    let big = h
        .service
        .request_withdrawal(&tr, invoice_req(3_000_000))
        .await
        .unwrap();
    assert_eq!(big.status, "requested");
    untrust(&h, t.id).await;
    let tr = tenant_row(&h, t.id).await;
    let new_project = h
        .service
        .request_withdrawal(&tr, invoice_req(1_000_000))
        .await
        .unwrap();
    assert_eq!(new_project.status, "requested");
}

#[tokio::test]
async fn worker_sends_approved_lightning_withdrawal_once_and_posts_to_wallet() {
    let (h, node) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 5_000_000, "f").await;
    *node.fee_msat.lock().unwrap() = 3_000;
    let tr = tenant_row(&h, t.id).await;
    let req = invoice_req(2_000_000);
    let inv_w = req.invoice.clone().unwrap();
    let w = h.service.request_withdrawal(&tr, req).await.unwrap();
    assert_eq!(h.service.run_payouts().await.unwrap(), 1);
    assert_eq!(h.service.run_payouts().await.unwrap(), 0);
    assert_eq!(node.pays(), 1);
    let done = db::get_withdrawal(&h.pool, w.id).await.unwrap().unwrap();
    assert_eq!(done.status, "completed");
    assert_eq!(done.fee, Some(3_000));
    assert_eq!(balance(&h, t.id, "pending_payout").await, 0);
    assert_eq!(
        balance(&h, t.id, "tenant_balance").await,
        5_000_000 - 2_000_000 - 3_000
    );
    let wallet: (i64,) = sqlx::query_as(
        "select balance from ledger_account where tenant_id is null and kind = 'platform_wallet'",
    )
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!(wallet.0, -5_000_000 + 2_003_000);
    assert!(ledger::reconcile(&h.pool).await.unwrap().is_empty());
    let findings = payment_gateway::ops::integrity::check(&h.pool)
        .await
        .unwrap();
    assert!(findings.is_empty(), "integrity findings: {findings:?}");
    h.service.deliver_due().await.unwrap();
    assert_eq!(
        t.receiver.types(),
        vec![
            "withdrawal.requested",
            "withdrawal.completed",
            "withdrawal.paid"
        ]
    );
    let body = &t.receiver.events()[1];
    assert_eq!(body["data"]["withdrawal"]["status"], "completed");
    assert!(!body.to_string().contains(&inv_w));
}

#[tokio::test]
async fn routing_failure_retries_then_fails_and_releases_reserve() {
    let (h, node) = setup().await;
    h.service.payouts.set_policy(PayoutPolicy {
        max_attempts: 2,
        ..policy()
    });
    let t = tenant(&h).await;
    fund(&h, t.id, 5_000_000, "f").await;
    node.reject_pay.store(true, Ordering::SeqCst);
    let tr = tenant_row(&h, t.id).await;
    let w = h
        .service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await
        .unwrap();
    h.service.run_payouts().await.unwrap();
    assert_eq!(status_of(&h, w.id).await, "approved");
    h.service.run_payouts().await.unwrap();
    let failed = db::get_withdrawal(&h.pool, w.id).await.unwrap().unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.error.unwrap().contains("no route"));
    assert_eq!(balance(&h, t.id, "pending_payout").await, 0);
    assert_eq!(balance(&h, t.id, "tenant_balance").await, 5_000_000);
    h.service.deliver_due().await.unwrap();
    assert_eq!(
        t.receiver.types(),
        vec!["withdrawal.requested", "withdrawal.failed"]
    );
}

#[tokio::test]
async fn node_reported_failure_releases_reserve_without_retry() {
    let (h, node) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 5_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let req = invoice_req(2_000_000);
    let hash = hash_of(req.invoice.as_ref().unwrap());
    node.payments
        .lock()
        .unwrap()
        .insert(hash, PaymentState::Failed("FAILURE_REASON_NO_ROUTE".into()));
    let w = h.service.request_withdrawal(&tr, req).await.unwrap();
    db::claim_withdrawal(&h.pool, w.id, chrono::Utc::now())
        .await
        .unwrap();
    h.service.run_payouts().await.unwrap();
    assert_eq!(status_of(&h, w.id).await, "failed");
    assert_eq!(node.pays(), 0);
    assert_eq!(balance(&h, t.id, "tenant_balance").await, 5_000_000);
}

#[tokio::test]
async fn crash_after_claim_before_call_does_not_double_send() {
    let (h, node) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 5_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let w = h
        .service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await
        .unwrap();
    assert!(
        db::claim_withdrawal(&h.pool, w.id, chrono::Utc::now())
            .await
            .unwrap()
    );
    assert!(
        !db::claim_withdrawal(&h.pool, w.id, chrono::Utc::now())
            .await
            .unwrap()
    );
    assert_eq!(status_of(&h, w.id).await, "sending");
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    h.service.run_payouts().await.unwrap();
    assert_eq!(node.pays(), 1);
    assert_eq!(status_of(&h, w.id).await, "completed");
    h.service.run_payouts().await.unwrap();
    assert_eq!(node.pays(), 1);
}

#[tokio::test]
async fn crash_after_call_completes_from_node_state_without_paying_again() {
    let (h, node) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 5_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let req = invoice_req(2_000_000);
    let hash = hash_of(req.invoice.as_ref().unwrap());
    let w = h.service.request_withdrawal(&tr, req).await.unwrap();
    db::claim_withdrawal(&h.pool, w.id, chrono::Utc::now())
        .await
        .unwrap();
    node.payments.lock().unwrap().insert(
        hash,
        PaymentState::Succeeded {
            fee_msat: 1_000,
            reference: "p".into(),
        },
    );
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    h.service.run_payouts().await.unwrap();
    assert_eq!(node.pays(), 0);
    let done = db::get_withdrawal(&h.pool, w.id).await.unwrap().unwrap();
    assert_eq!((done.status.as_str(), done.fee), ("completed", Some(1_000)));
}

#[tokio::test]
async fn unknown_outcome_is_reconciled_before_any_retry() {
    let (h, node) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 5_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let w = h
        .service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await
        .unwrap();
    node.lose_pay_result.store(true, Ordering::SeqCst);
    node.lookup_down.store(true, Ordering::SeqCst);
    h.service.run_payouts().await.unwrap();
    assert_eq!(status_of(&h, w.id).await, "sending");
    assert!(balance(&h, t.id, "pending_payout").await > 0);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    h.service.run_payouts().await.unwrap();
    assert_eq!(status_of(&h, w.id).await, "sending");
    assert_eq!(node.pays(), 1);
    node.lookup_down.store(false, Ordering::SeqCst);
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    h.service.run_payouts().await.unwrap();
    assert_eq!(status_of(&h, w.id).await, "completed");
    assert_eq!(node.pays(), 1);
}

#[tokio::test]
async fn onchain_withdrawal_sends_once_and_reconciles_by_label() {
    let (h, node) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 50_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let addr = regtest_address();
    let did = ready_destination(&h, &tr, DestinationKind::Onchain, &addr).await;
    let w = h
        .service
        .request_withdrawal(
            &tr,
            WithdrawalRequest {
                amount: 20_000_000,
                destination_id: Some(did),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(w.status, "approved");
    db::claim_withdrawal(&h.pool, w.id, chrono::Utc::now())
        .await
        .unwrap();
    node.onchain
        .lock()
        .unwrap()
        .insert(format!("pgw-withdrawal-{}", w.id), ("abc".into(), 200));
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    h.service.run_payouts().await.unwrap();
    assert!(node.sends.lock().unwrap().is_empty());
    let done = db::get_withdrawal(&h.pool, w.id).await.unwrap().unwrap();
    assert_eq!(done.status, "completed");
    assert_eq!(done.external_id.as_deref(), Some("abc"));
    assert_eq!(done.fee, Some(200_000));

    let w2 = h
        .service
        .request_withdrawal(
            &tr,
            WithdrawalRequest {
                amount: 10_000_000,
                destination_id: Some(did),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    h.service.run_payouts().await.unwrap();
    h.service.run_payouts().await.unwrap();
    let sends = node.sends.lock().unwrap().clone();
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].0, addr);
    assert_eq!(sends[0].1, 10_000);
    assert_eq!(status_of(&h, w2.id).await, "completed");
    assert_eq!(balance(&h, t.id, "pending_payout").await, 0);
    assert!(ledger::reconcile(&h.pool).await.unwrap().is_empty());
    let findings = payment_gateway::ops::integrity::check(&h.pool)
        .await
        .unwrap();
    assert!(findings.is_empty(), "integrity findings: {findings:?}");
}

#[tokio::test]
async fn new_destination_needs_verification_and_waits_out_the_delay() {
    let (h, node) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 50_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let addr = regtest_address();
    let d = h
        .service
        .add_destination(&tr, DestinationKind::Onchain, "cold", &addr, Some(1))
        .await
        .unwrap();
    let ask = |id| WithdrawalRequest {
        amount: 10_000_000,
        destination_id: Some(id),
        ..Default::default()
    };
    let r = h.service.request_withdrawal(&tr, ask(d.id)).await;
    assert!(matches!(r, Err(Error::Conflict(m)) if m.contains("not verified")));

    let test = h
        .service
        .verify_destination_payout(&tr, d.id, None)
        .await
        .unwrap();
    assert_eq!(test.purpose, "verify");
    assert_eq!(test.status, "approved");
    h.service.run_payouts().await.unwrap();
    assert_eq!(node.sends.lock().unwrap().len(), 1);
    let after = db::get_destination(&h.pool, t.id, d.id)
        .await
        .unwrap()
        .unwrap();
    assert!(after.verified);

    let r = h.service.request_withdrawal(&tr, ask(d.id)).await;
    assert!(matches!(r, Err(Error::Conflict(m)) if m.contains("can be used from")));

    sqlx::query("update payout_destination set usable_from = ? where id = ?")
        .bind(chrono::Utc::now() - chrono::Duration::seconds(1))
        .bind(d.id)
        .execute(&h.pool)
        .await
        .unwrap();
    assert!(h.service.request_withdrawal(&tr, ask(d.id)).await.is_ok());
    h.service.deliver_due().await.unwrap();
    assert!(
        !t.receiver
            .types()
            .iter()
            .any(|t| t == "withdrawal.completed"),
        "verification payouts are not announced"
    );
}

#[tokio::test]
async fn destination_delay_is_configurable() {
    let (h, _) = setup().await;
    h.service.payouts.set_policy(PayoutPolicy {
        destination_delay: chrono::Duration::hours(72),
        ..policy()
    });
    let t = tenant(&h).await;
    let tr = tenant_row(&h, t.id).await;
    let d = h
        .service
        .add_destination(&tr, DestinationKind::Onchain, "x", &regtest_address(), None)
        .await
        .unwrap();
    let wait = d.usable_from - chrono::Utc::now();
    assert!(wait > chrono::Duration::hours(71) && wait <= chrono::Duration::hours(72));
}

#[tokio::test]
async fn signed_message_verifies_a_p2pkh_destination() {
    use bitcoin::secp256k1::{Message, Secp256k1, SecretKey};
    use bitcoin::sign_message::{MessageSignature, signed_msg_hash};
    let (h, _) = setup().await;
    let t = tenant(&h).await;
    let tr = tenant_row(&h, t.id).await;
    let secp = Secp256k1::new();
    let sk = SecretKey::from_slice(&[0x42; 32]).unwrap();
    let pk = bitcoin::PublicKey::new(sk.public_key(&secp));
    let addr = bitcoin::Address::p2pkh(pk, bitcoin::Network::Regtest).to_string();
    let d = h
        .service
        .add_destination(&tr, DestinationKind::Onchain, "signed", &addr, None)
        .await
        .unwrap();
    let challenge = verify_challenge(&d);
    let sign = |msg: &str| {
        let m = Message::from_digest(signed_msg_hash(msg).to_byte_array());
        MessageSignature::new(secp.sign_ecdsa_recoverable(&m, &sk), true).to_base64()
    };
    let wrong = h
        .service
        .verify_destination_signature(&tr, d.id, &sign("something else"))
        .await;
    assert!(matches!(wrong, Err(Error::Invalid(_))));
    let ok = h
        .service
        .verify_destination_signature(&tr, d.id, &sign(&challenge))
        .await
        .unwrap();
    assert!(ok.verified);
}

#[tokio::test]
async fn lightning_address_destination_resolves_through_lnurl_pay() {
    let (h, node) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 50_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let base = format!("http://{host}");
    let app = axum::Router::new()
        .route(
            "/.well-known/lnurlp/alice",
            get({
                let base = base.clone();
                move || async move {
                    axum::Json(json!({
                        "tag": "payRequest",
                        "callback": format!("{base}/cb"),
                        "minSendable": 1000,
                        "maxSendable": 100_000_000_000u64,
                    }))
                }
            }),
        )
        .route(
            "/cb",
            get(
                |axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>| async move {
                    let amount: u64 = q["amount"].parse().unwrap();
                    axum::Json(json!({"pr": bolt11(amount)}))
                },
            ),
        );
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let did = ready_destination(
        &h,
        &tr,
        DestinationKind::LightningAddress,
        &format!("alice@{host}"),
    )
    .await;
    let w = h
        .service
        .request_withdrawal(
            &tr,
            WithdrawalRequest {
                amount: 3_000_000,
                destination_id: Some(did),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    h.service.run_payouts().await.unwrap();
    assert_eq!(status_of(&h, w.id).await, "completed");
    assert_eq!(node.pays(), 1);
}

#[tokio::test]
async fn bolt11_must_match_amount_and_not_be_reused() {
    let (h, _) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 50_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    let wrong = WithdrawalRequest {
        amount: 2_000_000,
        invoice: Some(bolt11(3_000_000)),
        ..Default::default()
    };
    assert!(matches!(
        h.service.request_withdrawal(&tr, wrong).await,
        Err(Error::Invalid(_))
    ));
    let junk = WithdrawalRequest {
        amount: 2_000_000,
        invoice: Some("lnbcrt1junk".into()),
        ..Default::default()
    };
    assert!(matches!(
        h.service.request_withdrawal(&tr, junk).await,
        Err(Error::Invalid(_))
    ));
    let inv = bolt11(2_000_000);
    let first = WithdrawalRequest {
        amount: 2_000_000,
        invoice: Some(inv.clone()),
        ..Default::default()
    };
    h.service
        .request_withdrawal(&tr, first.clone())
        .await
        .unwrap();
    assert!(h.service.request_withdrawal(&tr, first).await.is_err());
}

#[tokio::test]
async fn destinations_are_sealed_and_never_echoed() {
    let (h, _) = setup().await;
    let t = tenant(&h).await;
    let tr = tenant_row(&h, t.id).await;
    let addr = regtest_address();
    h.service
        .add_destination(&tr, DestinationKind::Onchain, "cold", &addr, None)
        .await
        .unwrap();
    let (raw,): (Vec<u8>,) = sqlx::query_as("select value from payout_destination")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&raw).contains(&addr));
    assert!(
        h.service
            .add_destination(&tr, DestinationKind::Onchain, "again", &addr, None)
            .await
            .is_err()
    );
}

struct Console {
    cookie: String,
    csrf: String,
}

async fn login(h: &Harness, email: &str, operator: bool) -> Console {
    let hash = accounts::hash_password("correct horse battery").unwrap();
    accounts::insert_account(&h.pool, email, &hash, operator)
        .await
        .unwrap();
    let r = send(
        &h.app,
        "POST",
        "/console/api/login",
        None,
        &[],
        json!({"email": email, "password": "correct horse battery"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    Console {
        cookie: r.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .trim_start_matches("pgw_session=")
            .to_string(),
        csrf: r.json["csrf"].as_str().unwrap().to_string(),
    }
}

async fn console(
    h: &Harness,
    c: &Console,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let cookie = format!("pgw_session={}", c.cookie);
    let r = send(
        &h.app,
        method,
        &format!("/console/api{path}"),
        None,
        &[("cookie", &cookie), ("x-csrf-token", &c.csrf)],
        body,
    )
    .await;
    (r.status, r.json)
}

async fn new_project(h: &Harness, c: &Console, name: &str) -> u64 {
    let (s, r) = console(h, c, "POST", "/projects", json!({"name": name})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    r["id"].as_u64().unwrap()
}

#[tokio::test]
async fn operator_approval_flow_and_isolation_over_the_console() {
    let (h, node) = setup().await;
    let owner = login(&h, "owner@example.com", false).await;
    let other = login(&h, "other@example.com", false).await;
    let op = login(&h, "op@example.com", true).await;
    let a = new_project(&h, &owner, "shop-a").await;
    let b = new_project(&h, &other, "shop-b").await;
    fund(&h, a, 10_000_000, "fa").await;
    fund(&h, b, 10_000_000, "fb").await;

    let inv_a = bolt11(2_000_000);
    let (s, w) = console(
        &h,
        &owner,
        "POST",
        &format!("/projects/{a}/withdrawals"),
        json!({"amount": 2_000_000, "invoice": inv_a}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{w}");
    assert_eq!(w["status"], "requested");
    let wid = w["id"].as_u64().unwrap();
    assert!(!w.to_string().contains(&inv_a));
    h.service.run_payouts().await.unwrap();
    assert_eq!(node.pays(), 0);

    let (s, _) = console(
        &h,
        &other,
        "GET",
        &format!("/projects/{a}/payouts"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = console(
        &h,
        &other,
        "POST",
        &format!("/projects/{b}/withdrawals/{wid}/cancel"),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = console(
        &h,
        &other,
        "POST",
        &format!("/projects/{a}/withdrawals/{wid}/cancel"),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = console(
        &h,
        &owner,
        "POST",
        &format!("/admin/withdrawals/{wid}/approve"),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = console(&h, &owner, "GET", "/admin/withdrawals", Value::Null).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = console(
        &h,
        &owner,
        "POST",
        &format!("/admin/withdrawals/{wid}/reject"),
        json!({"reason": "x"}),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (s, q) = console(&h, &op, "GET", "/admin/withdrawals", Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    let q = q.as_array().unwrap();
    assert_eq!(q.len(), 1);
    assert_eq!(q[0]["project"], "shop-a");
    assert_eq!(q[0]["trusted"], false);
    assert!(
        q[0]["balances"]
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["kind"] == "pending_payout")
    );

    let (s, ap) = console(
        &h,
        &op,
        "POST",
        &format!("/admin/withdrawals/{wid}/approve"),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{ap}");
    assert_eq!(ap["status"], "approved");
    h.service.run_payouts().await.unwrap();
    assert_eq!(status_of(&h, wid).await, "completed");
    let (s, _) = console(
        &h,
        &op,
        "POST",
        &format!("/admin/withdrawals/{wid}/approve"),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);

    let (_, w2) = console(
        &h,
        &owner,
        "POST",
        &format!("/projects/{a}/withdrawals"),
        json!({"amount": 1_000_000, "invoice": bolt11(1_000_000)}),
    )
    .await;
    let w2id = w2["id"].as_u64().unwrap();
    let (s, r) = console(
        &h,
        &op,
        "POST",
        &format!("/admin/withdrawals/{w2id}/reject"),
        json!({"reason": "unknown customer"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "rejected");
    assert_eq!(balance(&h, a, "pending_payout").await, 0);

    let (s, ov) = console(
        &h,
        &owner,
        "GET",
        &format!("/projects/{a}/payouts"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(ov["withdrawals"].as_array().unwrap().len(), 2);
    let (s, ov_b) = console(
        &h,
        &other,
        "GET",
        &format!("/projects/{b}/payouts"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(ov_b["withdrawals"].as_array().unwrap().is_empty());

    let (s, c) = console(
        &h,
        &owner,
        "POST",
        &format!("/projects/{a}/withdrawals"),
        json!({"amount": 1_000_000, "invoice": bolt11(1_000_000)}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let cid = c["id"].as_u64().unwrap();
    let (s, c) = console(
        &h,
        &owner,
        "POST",
        &format!("/projects/{a}/withdrawals/{cid}/cancel"),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(c["status"], "cancelled");
}

#[tokio::test]
async fn destinations_over_the_console_mask_values_and_are_owner_only() {
    let (h, _) = setup().await;
    let owner = login(&h, "owner@example.com", false).await;
    let a = new_project(&h, &owner, "shop-a").await;
    let addr = regtest_address();
    let (s, d) = console(
        &h,
        &owner,
        "POST",
        &format!("/projects/{a}/payout-destinations"),
        json!({"kind": "onchain", "label": "cold", "value": addr}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["usable"], false);
    assert!(!d.to_string().contains(&addr));
    let (_, ov) = console(
        &h,
        &owner,
        "GET",
        &format!("/projects/{a}/payouts"),
        Value::Null,
    )
    .await;
    assert!(!ov.to_string().contains(&addr));
    assert!(
        ov["destinations"][0]["challenge"]
            .as_str()
            .unwrap()
            .contains("payout destination")
    );
    let (s, _) = console(
        &h,
        &owner,
        "POST",
        &format!("/projects/{a}/payout-destinations"),
        json!({"kind": "bolt11", "label": "x", "value": "lnbc1"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let did = d["id"].as_u64().unwrap();
    let (s, _) = console(
        &h,
        &owner,
        "DELETE",
        &format!("/projects/{a}/payout-destinations/{did}"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn payout_metrics_count_sends_failures_and_oldest_sending_age() {
    let (h, node) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 20_000_000, "f").await;
    let tr = tenant_row(&h, t.id).await;
    h.service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await
        .unwrap();
    h.service.run_payouts().await.unwrap();
    assert_eq!(
        h.service
            .metrics
            .counter("gateway_payout_sends_total", &[("result", "completed")]),
        1
    );
    node.reject_pay.store(true, Ordering::SeqCst);
    let w = h
        .service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await
        .unwrap();
    h.service.run_payouts().await.unwrap();
    assert_eq!(
        h.service
            .metrics
            .counter("gateway_payout_failures_total", &[]),
        1
    );
    assert_eq!(status_of(&h, w.id).await, "approved");
    node.reject_pay.store(false, Ordering::SeqCst);
    node.lookup_down.store(true, Ordering::SeqCst);
    node.lose_pay_result.store(true, Ordering::SeqCst);
    h.service.run_payouts().await.unwrap();
    assert_eq!(status_of(&h, w.id).await, "sending");
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    node.lookup_down.store(true, Ordering::SeqCst);
    h.service.run_payouts().await.unwrap();
    let age = h
        .service
        .metrics
        .gauges("gateway_payout_oldest_sending_age_seconds");
    assert_eq!(age.len(), 1);
    assert!(age[0].1 >= 1.0, "{age:?}");
    assert!(
        h.service
            .metrics
            .render()
            .contains("gateway_payout_oldest_sending_age_seconds")
    );
}

#[tokio::test]
async fn withdrawal_events_follow_endpoint_filters_and_appear_in_the_event_feed() {
    let (h, _) = setup().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 20_000_000, "f").await;
    let only_withdrawals = Receiver::start().await;
    let only_payments = Receiver::start().await;
    let sealed = h.service.sealer.seal(b"whsec").unwrap();
    db::insert_endpoint(
        &h.pool,
        t.id,
        &only_withdrawals.url,
        &sealed,
        None,
        Some("withdrawal.*"),
    )
    .await
    .unwrap();
    db::insert_endpoint(
        &h.pool,
        t.id,
        &only_payments.url,
        &sealed,
        None,
        Some("payment.paid"),
    )
    .await
    .unwrap();
    let tr = tenant_row(&h, t.id).await;
    h.service
        .request_withdrawal(&tr, invoice_req(2_000_000))
        .await
        .unwrap();
    h.service.run_payouts().await.unwrap();
    h.service.deliver_due().await.unwrap();
    let types = only_withdrawals.types();
    assert!(
        types.contains(&"withdrawal.requested".to_string()),
        "{types:?}"
    );
    assert!(
        types.contains(&"withdrawal.completed".to_string()),
        "{types:?}"
    );
    assert!(!types.iter().any(|t| t == "payment.paid"));
    assert!(
        only_payments.types().is_empty(),
        "{:?}",
        only_payments.types()
    );

    let (s, feed) = call(
        &h.app,
        "GET",
        "/v1/events?type=withdrawal.completed",
        Some(&t.key),
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{feed}");
    assert_eq!(feed.as_array().unwrap().len(), 1);
    assert_eq!(feed[0]["data"]["withdrawal"]["status"], "completed");

    let payment = new_payment(&h, &t, eur("w-1", 1000)).await;
    let id = payment["id"].as_str().unwrap();
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/cancel"),
        Some(&t.key),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    h.service.deliver_due().await.unwrap();
    assert!(
        !only_withdrawals
            .types()
            .iter()
            .any(|t| t.starts_with("payment.")),
        "{:?}",
        only_withdrawals.types()
    );
    assert!(only_payments.types().is_empty());
    let (_, all) = call(
        &h.app,
        "GET",
        "/v1/events",
        Some(&t.key),
        serde_json::Value::Null,
    )
    .await;
    assert!(
        all.as_array()
            .unwrap()
            .iter()
            .any(|e| e["type"] == "payment.cancelled")
    );
    let (s, spec) = call(
        &h.app,
        "GET",
        "/v1/openapi.json",
        None,
        serde_json::Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let events = spec["components"]["schemas"]["EventType"]["enum"]
        .as_array()
        .unwrap();
    for e in [
        "withdrawal.requested",
        "withdrawal.completed",
        "withdrawal.failed",
    ] {
        assert!(events.iter().any(|v| v == e), "{e}");
    }
}
