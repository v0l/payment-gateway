mod common;

use chrono::{DateTime, Duration, TimeZone, Utc};
use common::*;
use payment_gateway::lease::{Clock, Lease, WORKERS_LEASE};
use std::sync::{Arc, Mutex};

struct ManualClock(Mutex<DateTime<Utc>>);

impl ManualClock {
    fn new() -> Arc<Self> {
        Arc::new(Self(Mutex::new(
            Utc.with_ymd_and_hms(2030, 1, 1, 0, 0, 0).unwrap(),
        )))
    }

    fn advance(&self, secs: i64) {
        *self.0.lock().unwrap() += Duration::seconds(secs);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> DateTime<Utc> {
        *self.0.lock().unwrap()
    }
}

fn lease(h: &Harness, holder: &str, clock: &Arc<ManualClock>) -> Lease {
    Lease::new(
        h.pool.clone(),
        WORKERS_LEASE,
        holder,
        std::time::Duration::from_secs(30),
        clock.clone(),
    )
}

#[tokio::test]
async fn only_one_holder_at_a_time() {
    let h = harness().await;
    let clock = ManualClock::new();
    let a = lease(&h, "a", &clock);
    let b = lease(&h, "b", &clock);
    assert!(a.acquire_or_renew().await.unwrap());
    assert!(!b.acquire_or_renew().await.unwrap());
    assert!(a.is_held());
    assert!(!b.is_held());
    assert_eq!(a.current().await.unwrap().unwrap().holder, "a");
}

#[tokio::test]
async fn renewal_extends_expiry_and_blocks_takeover() {
    let h = harness().await;
    let clock = ManualClock::new();
    let a = lease(&h, "a", &clock);
    let b = lease(&h, "b", &clock);
    assert!(a.acquire_or_renew().await.unwrap());
    for _ in 0..5 {
        clock.advance(20);
        assert!(a.acquire_or_renew().await.unwrap());
        assert!(!b.acquire_or_renew().await.unwrap());
    }
}

#[tokio::test]
async fn expired_lease_is_taken_over_and_old_holder_loses_it() {
    let h = harness().await;
    let clock = ManualClock::new();
    let a = lease(&h, "a", &clock);
    let b = lease(&h, "b", &clock);
    assert!(a.acquire_or_renew().await.unwrap());
    clock.advance(29);
    assert!(!b.acquire_or_renew().await.unwrap());
    clock.advance(1);
    assert!(b.acquire_or_renew().await.unwrap());
    assert!(!a.acquire_or_renew().await.unwrap());
    assert!(!a.is_held());
    assert!(b.is_held());
}

#[tokio::test]
async fn release_lets_the_next_instance_start_at_once() {
    let h = harness().await;
    let clock = ManualClock::new();
    let a = lease(&h, "a", &clock);
    let b = lease(&h, "b", &clock);
    assert!(a.acquire_or_renew().await.unwrap());
    a.release().await.unwrap();
    assert!(!a.is_held());
    assert!(b.acquire_or_renew().await.unwrap());
}

#[tokio::test]
async fn release_by_a_non_holder_changes_nothing() {
    let h = harness().await;
    let clock = ManualClock::new();
    let a = lease(&h, "a", &clock);
    let b = lease(&h, "b", &clock);
    assert!(a.acquire_or_renew().await.unwrap());
    b.release().await.unwrap();
    assert_eq!(a.current().await.unwrap().unwrap().holder, "a");
}

#[tokio::test]
async fn concurrent_acquire_has_exactly_one_winner() {
    let h = harness().await;
    let clock = ManualClock::new();
    let leases: Vec<Arc<Lease>> = (0..8)
        .map(|i| Arc::new(lease(&h, &format!("n{i}"), &clock)))
        .collect();
    let tasks: Vec<_> = leases
        .iter()
        .map(|l| {
            let l = l.clone();
            tokio::spawn(async move { l.acquire_or_renew().await.unwrap() })
        })
        .collect();
    let mut wins = 0;
    for t in tasks {
        if t.await.unwrap() {
            wins += 1;
        }
    }
    assert_eq!(wins, 1);
}

#[tokio::test]
async fn only_the_leader_runs_the_workers() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let _ = ln;
    let clock = ManualClock::new();
    let a = Arc::new(lease(&h, "a", &clock));
    let b = Arc::new(lease(&h, "b", &clock));
    let (tx, rx) = tokio::sync::watch::channel(false);
    let lead_a = tokio::spawn(payment_gateway::worker::lead(
        h.service.clone(),
        std::time::Duration::from_millis(50),
        a.clone(),
        rx.clone(),
    ));
    for _ in 0..50 {
        if a.is_held() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(a.is_held());
    let lead_b = tokio::spawn(payment_gateway::worker::lead(
        h.service.clone(),
        std::time::Duration::from_millis(50),
        b.clone(),
        rx,
    ));
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(!b.is_held());
    tx.send(true).unwrap();
    lead_a.await.unwrap();
    lead_b.await.unwrap();
    assert!(a.current().await.unwrap().is_none());
}

use serde_json::json;

#[tokio::test]
async fn only_the_leader_sends_payouts() {
    use payment_gateway::db;
    use payment_gateway::ledger::{AccountKind, AccountRef, Entry, Posting, TxKind, post};
    use payment_gateway::service::WithdrawalRequest;
    let h = harness().await;
    let t = tenant(&h).await;
    let node = Arc::new(CountingNode::default());
    h.service.payouts.set_node(Some(node.clone()));
    h.service
        .payouts
        .set_policy(payment_gateway::payouts::PayoutPolicy {
            network: bitcoin::Network::Regtest,
            ..Default::default()
        });
    let mut tx = h.pool.begin().await.unwrap();
    post(
        &mut tx,
        &Posting::new(
            TxKind::Adjustment,
            vec![
                Entry::new(
                    AccountRef::platform(AccountKind::PlatformWallet, "BTC"),
                    -9_000_000,
                ),
                Entry::new(
                    AccountRef::tenant(t.id, AccountKind::TenantBalance, "BTC"),
                    9_000_000,
                ),
            ],
        ),
        "fund",
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let tr = h.service.tenant(t.id).await.unwrap();
    let w = h
        .service
        .request_withdrawal(
            &tr,
            WithdrawalRequest {
                amount: 2_000_000,
                invoice: Some(lease_invoice(2_000_000)),
                ..Default::default()
            },
        )
        .await
        .unwrap();

    let clock = ManualClock::new();
    let a = Arc::new(lease(&h, "a", &clock));
    let b = Arc::new(lease(&h, "b", &clock));
    assert!(a.acquire_or_renew().await.unwrap());
    let (tx_stop, rx) = tokio::sync::watch::channel(false);
    let lead_b = tokio::spawn(payment_gateway::worker::lead(
        h.service.clone(),
        std::time::Duration::from_millis(50),
        b.clone(),
        rx.clone(),
    ));
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(!b.is_held());
    assert_eq!(node.pays(), 0);
    assert_eq!(
        db::get_withdrawal(&h.pool, w.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "approved"
    );
    tx_stop.send(true).unwrap();
    lead_b.await.unwrap();

    let c = Arc::new(lease(&h, "c", &clock));
    let (tx_stop, rx) = tokio::sync::watch::channel(false);
    clock.advance(31);
    let lead_c = tokio::spawn(payment_gateway::worker::lead(
        h.service.clone(),
        std::time::Duration::from_millis(50),
        c.clone(),
        rx,
    ));
    for _ in 0..60 {
        if db::get_withdrawal(&h.pool, w.id)
            .await
            .unwrap()
            .unwrap()
            .status
            == "completed"
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(node.pays(), 1);
    assert_eq!(
        db::get_withdrawal(&h.pool, w.id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "completed"
    );
    tx_stop.send(true).unwrap();
    lead_c.await.unwrap();
    assert!(
        h.service
            .metrics
            .counter("gateway_payout_sends_total", &[("result", "completed")])
            >= 1
    );
}

#[derive(Default)]
struct CountingNode {
    pays: Mutex<Vec<String>>,
    done: Mutex<std::collections::HashMap<String, payment_gateway::payouts::PaymentState>>,
}

impl CountingNode {
    fn pays(&self) -> usize {
        self.pays.lock().unwrap().len()
    }
}

#[async_trait::async_trait]
impl payment_gateway::payouts::PayoutNode for CountingNode {
    async fn pay_invoice(&self, invoice: &str, _fee: u64) -> anyhow::Result<()> {
        self.pays.lock().unwrap().push(invoice.into());
        let hash = payment_gateway::providers::lightning::parse_invoice(invoice)
            .unwrap()
            .payment_hash();
        self.done.lock().unwrap().insert(
            hash,
            payment_gateway::payouts::PaymentState::Succeeded {
                fee_msat: 0,
                reference: "p".into(),
            },
        );
        Ok(())
    }
    async fn payment_state(
        &self,
        h: &str,
    ) -> anyhow::Result<payment_gateway::payouts::PaymentState> {
        Ok(self
            .done
            .lock()
            .unwrap()
            .get(h)
            .cloned()
            .unwrap_or(payment_gateway::payouts::PaymentState::NotFound))
    }
    async fn send_coins(&self, _: &str, _: u64, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
    async fn onchain_state(
        &self,
        _: &str,
    ) -> anyhow::Result<payment_gateway::payouts::OnchainState> {
        Ok(payment_gateway::payouts::OnchainState::NotFound)
    }
}

fn lease_invoice(amount_msat: u64) -> String {
    use bitcoin::hashes::{Hash, sha256};
    use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
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
