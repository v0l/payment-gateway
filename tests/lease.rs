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
