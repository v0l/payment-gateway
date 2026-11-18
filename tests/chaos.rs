mod common;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::*;
use payment_gateway::ops::integrity;
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::ProviderSink;
use rand::seq::SliceRandom;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn paid(lookup: &str, reference: &str, amount: Option<u64>, confirmed: bool) -> ProviderEvent {
    ProviderEvent::Paid {
        lookup: lookup.into(),
        reference: reference.into(),
        amount,
        confirmed,
        saved: None,
    }
}

fn sink(h: &Harness, provider_id: u64) -> ProviderSink {
    ProviderSink {
        service: h.service.clone(),
        provider_id,
    }
}

struct Made {
    id: String,
    lookup: String,
    required: u64,
}

async fn make(h: &Harness, t: &TenantFixture, method: &str, n: usize) -> Made {
    let p = new_payment(h, t, eur(&format!("chaos-{n}"), 1000 + n as u64)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, r) = attempt(h, t, &id, json!({"method": method})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let a = &r["attempts"][0];
    let lookup: String = sqlx::query_scalar("select lookup from payment_attempt where id = ?")
        .bind(a["id"].as_u64().unwrap())
        .fetch_one(&h.pool)
        .await
        .unwrap();
    Made {
        id,
        lookup,
        required: a["amount"].as_u64().unwrap(),
    }
}

async fn drain_deliveries(h: &Harness) {
    for _ in 0..20 {
        if h.service.deliver_due().await.unwrap() == 0 {
            break;
        }
    }
}

async fn assert_invariants(h: &Harness) {
    let findings = integrity::check(&h.pool).await.unwrap();
    assert!(findings.is_empty(), "integrity findings: {findings:?}");

    let paid_events: Vec<(Vec<u8>, i64)> = sqlx::query_as(
        "select payment_id, count(*) from webhook_delivery where event_type = 'payment.paid' group by payment_id",
    )
    .fetch_all(&h.pool)
    .await
    .unwrap();
    for (_, n) in &paid_events {
        assert_eq!(*n, 1, "a payment emitted more than one payment.paid");
    }
    let with_event: std::collections::HashSet<Vec<u8>> =
        paid_events.into_iter().map(|(p, _)| p).collect();

    let settled: Vec<(Vec<u8>, String)> = sqlx::query_as(
        "select id, status from payment where status in ('paid','refunded','partially_refunded')",
    )
    .fetch_all(&h.pool)
    .await
    .unwrap();
    for (id, status) in &settled {
        assert!(
            with_event.contains(id),
            "{status} payment {} has no payment.paid event",
            hex::encode(id)
        );
    }
    let not_settled: Vec<(Vec<u8>,)> = sqlx::query_as(
        "select id from payment where status not in ('paid','refunded','partially_refunded') and id in (select payment_id from webhook_delivery where event_type = 'payment.paid')",
    )
    .fetch_all(&h.pool)
    .await
    .unwrap();
    assert!(
        not_settled.is_empty(),
        "payment.paid emitted for an unsettled payment"
    );

    let double_paid: Vec<(Vec<u8>, i64)> = sqlx::query_as(
        "select payment_id, count(*) from payment_attempt where status = 'paid' group by payment_id having count(*) > 1",
    )
    .fetch_all(&h.pool)
    .await
    .unwrap();
    assert!(
        double_paid.is_empty(),
        "payments with several paid attempts"
    );

    let attempts: Vec<(u64, u64, String)> = sqlx::query_as(
        "select id, charge_amount, charge_currency from payment_attempt where status = 'paid'",
    )
    .fetch_all(&h.pool)
    .await
    .unwrap();
    for (id, charge, currency) in attempts {
        let required = if currency == "BTC" {
            charge.div_ceil(1000) * 1000
        } else {
            charge
        };
        let received: i64 = sqlx::query_scalar(
            "select cast(coalesce(sum(amount), 0) as signed) from attempt_receipt where attempt_id = ? and confirmed = 1",
        )
        .bind(id)
        .fetch_one(&h.pool)
        .await
        .unwrap();
        assert!(
            received as u64 >= required,
            "paid attempt {id} received {received} of {required}"
        );
    }

    let over_refunded: Vec<(Vec<u8>,)> = sqlx::query_as(
        "select p.id from payment p join refund r on r.payment_id = p.id where r.status <> 'failed' group by p.id, p.amount having sum(r.amount) > p.amount",
    )
    .fetch_all(&h.pool)
    .await
    .unwrap();
    assert!(over_refunded.is_empty(), "refunds exceed payment amount");
}

async fn status_of(h: &Harness, t: &TenantFixture, id: &str) -> String {
    get_payment(h, t, id).await["status"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn duplicate_and_out_of_order_events_settle_each_payment_once() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let oc = provider(&h, &t, "onchain", json!({}), None).await;
    let mut lightning = vec![];
    let mut onchain = vec![];
    for n in 0..20 {
        lightning.push(make(&h, &t, "lightning", n).await);
        onchain.push(make(&h, &t, "onchain", 100 + n).await);
    }

    let mut jobs: Vec<(u64, ProviderEvent)> = vec![];
    for m in &lightning {
        for _ in 0..3 {
            jobs.push((ln, paid(&m.lookup, &m.lookup, None, true)));
        }
        jobs.push((
            ln,
            ProviderEvent::Failed {
                lookup: m.lookup.clone(),
                reason: "late failure".into(),
            },
        ));
    }
    for (i, m) in onchain.iter().enumerate() {
        let half = m.required / 2;
        let tx = format!("tx{i}");
        jobs.push((oc, paid(&m.lookup, &format!("{tx}:0"), Some(half), true)));
        jobs.push((oc, paid(&m.lookup, &format!("{tx}:0"), Some(half), false)));
        jobs.push((
            oc,
            paid(&m.lookup, &format!("{tx}:1"), Some(m.required - half), true),
        ));
        jobs.push((
            oc,
            paid(&m.lookup, &format!("{tx}:1"), Some(m.required - half), true),
        ));
    }
    jobs.shuffle(&mut rand::thread_rng());

    let mut tasks = vec![];
    for chunk in jobs.chunks(7) {
        let sinks: Vec<(ProviderSink, ProviderEvent)> = chunk
            .iter()
            .map(|(p, e)| (sink(&h, *p), e.clone()))
            .collect();
        tasks.push(tokio::spawn(async move {
            for (s, e) in sinks {
                s.event(e, None).await.unwrap();
            }
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }

    for m in lightning.iter().chain(onchain.iter()) {
        assert_eq!(status_of(&h, &t, &m.id).await, "paid");
    }
    drain_deliveries(&h).await;
    let paid_hooks = t
        .receiver
        .types()
        .iter()
        .filter(|ty| *ty == "payment.paid")
        .count();
    assert_eq!(paid_hooks, 40);
    assert_invariants(&h).await;
}

#[tokio::test]
async fn provider_errors_do_not_leave_half_created_payments() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let first = make(&h, &t, "lightning", 0).await;
    let flaky = mock(&h, ln);
    *flaky.fail_create.lock().unwrap() = true;
    let q = new_payment(&h, &t, eur("flaky2", 2500)).await;
    let (s, _) = attempt(
        &h,
        &t,
        q["id"].as_str().unwrap(),
        json!({"method": "lightning"}),
    )
    .await;
    assert_ne!(
        s,
        StatusCode::OK,
        "create must fail while the provider is down"
    );
    assert_eq!(status_of(&h, &t, &first.id).await, "pending");
    let got = get_payment(&h, &t, q["id"].as_str().unwrap()).await;
    assert_eq!(got["attempts"].as_array().unwrap().len(), 0);

    *flaky.fail_create.lock().unwrap() = false;
    let (s, r) = attempt(
        &h,
        &t,
        q["id"].as_str().unwrap(),
        json!({"method": "lightning"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap();
    sink(&h, ln)
        .event(paid(hash, hash, None, true), None)
        .await
        .unwrap();
    assert_eq!(status_of(&h, &t, q["id"].as_str().unwrap()).await, "paid");
    assert_invariants(&h).await;
}

#[tokio::test]
async fn failing_refunds_never_exceed_the_paid_amount() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    let m = make(&h, &t, "stripe", 0).await;
    let amount = 1000;
    sink(&h, st)
        .event(paid(&m.lookup, &m.lookup, None, true), None)
        .await
        .unwrap();
    assert_eq!(status_of(&h, &t, &m.id).await, "paid");

    let flaky = mock(&h, st);
    *flaky.fail_refund.lock().unwrap() = true;
    for _ in 0..3 {
        let (s, r) = call(
            &h.app,
            "POST",
            &format!("/v1/payments/{}/refund", m.id),
            Some(&t.key),
            json!({"amount": amount}),
        )
        .await;
        assert_eq!(s, StatusCode::OK, "{r}");
        assert_eq!(r["status"], "failed");
    }
    *flaky.fail_refund.lock().unwrap() = false;

    let mut tasks = vec![];
    for _ in 0..6 {
        let app = h.app.clone();
        let key = t.key.clone();
        let id = m.id.clone();
        tasks.push(tokio::spawn(async move {
            let (s, r) = call(
                &app,
                "POST",
                &format!("/v1/payments/{id}/refund"),
                Some(&key),
                json!({"amount": 400}),
            )
            .await;
            s == StatusCode::OK && r["status"] == "completed"
        }));
    }
    let mut ok = 0;
    for task in tasks {
        if task.await.unwrap() {
            ok += 1;
        }
    }
    assert_eq!(ok, 2, "only 800 of 1000 can be refunded in 400 steps");
    assert_invariants(&h).await;
}

struct SlowReceiver {
    url: String,
    hits: Arc<AtomicUsize>,
    seen: Arc<std::sync::Mutex<HashMap<String, usize>>>,
}

async fn tenant_with_hook(h: &Harness, url: &str) -> TenantFixture {
    let (s, r) = call(
        &h.app,
        "POST",
        "/admin/tenants",
        Some(ADMIN),
        json!({"name": unique("tenant"), "webhook_url": url}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    TenantFixture {
        id: r["id"].as_u64().unwrap(),
        key: r["api_key"].as_str().unwrap().to_string(),
        webhook_secret: r["webhook_secret"].as_str().unwrap().to_string(),
        receiver: Receiver::start().await,
    }
}

async fn slow_receiver(delay_ms: u64, fail_first: usize) -> SlowReceiver {
    #[derive(Clone)]
    struct S {
        hits: Arc<AtomicUsize>,
        seen: Arc<std::sync::Mutex<HashMap<String, usize>>>,
        delay_ms: u64,
        fail_first: usize,
    }
    async fn handle(
        State(s): State<S>,
        headers: axum::http::HeaderMap,
        _body: String,
    ) -> StatusCode {
        tokio::time::sleep(std::time::Duration::from_millis(s.delay_ms)).await;
        let n = s.hits.fetch_add(1, Ordering::SeqCst);
        let id = headers["x-gateway-event-id"].to_str().unwrap().to_string();
        if n < s.fail_first {
            return StatusCode::SERVICE_UNAVAILABLE;
        }
        *s.seen.lock().unwrap().entry(id).or_default() += 1;
        StatusCode::OK
    }
    let hits = Arc::new(AtomicUsize::new(0));
    let seen: Arc<std::sync::Mutex<HashMap<String, usize>>> = Default::default();
    let app = Router::new()
        .route("/", axum::routing::post(handle))
        .with_state(S {
            hits: hits.clone(),
            seen: seen.clone(),
            delay_ms,
            fail_first,
        });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    SlowReceiver { url, hits, seen }
}

#[tokio::test]
async fn slow_and_failing_webhooks_deliver_every_event_exactly_once_to_a_receiver() {
    let h = harness().await;
    let slow = slow_receiver(150, 3).await;
    let t = tenant_with_hook(&h, &slow.url).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let mut ids = vec![];
    for n in 0..10 {
        let m = make(&h, &t, "lightning", n).await;
        sink(&h, ln)
            .event(paid(&m.lookup, &m.lookup, None, true), None)
            .await
            .unwrap();
        ids.push(m.id);
    }

    let mut tasks = vec![];
    for _ in 0..4 {
        let service = h.service.clone();
        tasks.push(tokio::spawn(async move { service.deliver_due().await }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    sqlx::query("update webhook_delivery set next_attempt = ? where delivered_at is null")
        .bind(Utc::now() - Duration::seconds(1))
        .execute(&h.pool)
        .await
        .unwrap();
    let mut tasks = vec![];
    for _ in 0..4 {
        let service = h.service.clone();
        tasks.push(tokio::spawn(async move { service.deliver_due().await }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }

    let seen = slow.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 10, "every event reaches the receiver");
    assert!(
        seen.values().all(|n| *n == 1),
        "concurrent workers sent an event twice: {seen:?}"
    );
    assert!(slow.hits.load(Ordering::SeqCst) >= 13);
    let undelivered: i64 =
        sqlx::query_scalar("select count(*) from webhook_delivery where delivered_at is null")
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(undelivered, 0);
    assert_invariants(&h).await;
}

#[tokio::test]
async fn settlement_racing_expiry_cancel_and_attempt_creation_stays_consistent() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let _st = provider(&h, &t, "stripe", json!({}), None).await;
    let mut made = vec![];
    for n in 0..16 {
        made.push(make(&h, &t, "lightning", n).await);
    }
    sqlx::query("update payment set expires = ?")
        .bind(Utc::now() - Duration::seconds(5))
        .execute(&h.pool)
        .await
        .unwrap();

    let errors = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let mut tasks = vec![];
    for m in &made {
        let s = sink(&h, ln);
        let lookup = m.lookup.clone();
        let errors1 = errors.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = s.event(paid(&lookup, &lookup, None, true), None).await {
                errors1.lock().unwrap().push(format!("settle: {e:#}"));
            }
        }));
        let service = h.service.clone();
        let errors2 = errors.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = service.expire_due().await {
                errors2.lock().unwrap().push(format!("expire: {e:?}"));
            }
        }));
        let app = h.app.clone();
        let key = t.key.clone();
        let id = m.id.clone();
        tasks.push(tokio::spawn(async move {
            let _ = call(
                &app,
                "POST",
                &format!("/v1/payments/{id}/cancel"),
                Some(&key),
                Value::Null,
            )
            .await;
        }));
    }
    for task in tasks {
        task.await.unwrap();
    }
    let errors = errors.lock().unwrap().clone();
    assert!(
        errors.is_empty(),
        "concurrent operations failed: {errors:?}"
    );

    for m in &made {
        let status = status_of(&h, &t, &m.id).await;
        assert!(
            matches!(status.as_str(), "paid" | "cancelled" | "expired"),
            "{status}"
        );
    }
    drain_deliveries(&h).await;
    assert_invariants(&h).await;
}

#[tokio::test]
async fn reconcile_replays_after_the_watcher_misses_events() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let mut made = vec![];
    for n in 0..8 {
        made.push(make(&h, &t, "lightning", n).await);
    }
    let provider_mock = mock(&h, ln);
    for m in &made {
        provider_mock.mark_settled(&m.lookup);
    }
    for _ in 0..3 {
        let a = h.service.reconcile_provider(ln);
        let b = h.service.reconcile_provider(ln);
        let (a, b) = tokio::join!(a, b);
        a.unwrap();
        b.unwrap();
    }
    for m in &made {
        assert_eq!(status_of(&h, &t, &m.id).await, "paid");
    }
    drain_deliveries(&h).await;
    assert_invariants(&h).await;
}

#[tokio::test]
async fn a_database_deadlock_is_retried_and_other_errors_are_not() {
    use payment_gateway::service::{Error, is_deadlock, retry_deadlock};
    let h = harness().await;
    sqlx::query("create table dl (id int primary key, v int)")
        .execute(&h.pool)
        .await
        .unwrap();
    sqlx::query("insert into dl values (1, 0), (2, 0)")
        .execute(&h.pool)
        .await
        .unwrap();

    let attempts = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = vec![];
    for first in [1, 2] {
        let pool = h.pool.clone();
        let attempts = attempts.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            let second = 3 - first;
            let first_try = Arc::new(std::sync::atomic::AtomicBool::new(true));
            retry_deadlock(|| {
                let pool = pool.clone();
                let attempts = attempts.clone();
                let barrier = barrier.clone();
                let first_try = first_try.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    let mut tx = pool.begin().await?;
                    sqlx::query("update dl set v = v + 1 where id = ?")
                        .bind(first)
                        .execute(&mut *tx)
                        .await?;
                    if first_try.swap(false, Ordering::SeqCst) {
                        barrier.wait().await;
                    }
                    sqlx::query("update dl set v = v + 1 where id = ?")
                        .bind(second)
                        .execute(&mut *tx)
                        .await?;
                    tx.commit().await?;
                    Ok(())
                }
            })
            .await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    assert!(
        attempts.load(Ordering::SeqCst) >= 3,
        "one side must have been retried after the deadlock"
    );
    let sum: i64 = sqlx::query_scalar("select cast(sum(v) as signed) from dl")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(sum, 4);

    let calls = AtomicUsize::new(0);
    let err = retry_deadlock(|| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Err::<(), _>(Error::Conflict("nope".into())) }
    })
    .await
    .unwrap_err();
    assert!(!is_deadlock(&err));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

async fn shared_payment_hash(h: &Harness, t: &TenantFixture, cents: u64) -> (String, u64) {
    let p = new_payment(h, t, eur(&unique("dl"), cents)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    (
        r["attempts"][0]["details"]["payment_hash"]
            .as_str()
            .unwrap()
            .to_string(),
        r["attempts"][0]["id"].as_u64().unwrap(),
    )
}

async fn innodb_deadlocks(h: &Harness) -> u64 {
    let (_, v): (String, String) = sqlx::query_as("show global status like 'Innodb_deadlocks'")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    v.parse().unwrap()
}

async fn try_to_deadlock_settlement(
    h: &Harness,
    tenant_id: u64,
    shared_id: u64,
    hash: &str,
    attempt_id: u64,
) {
    use payment_gateway::ledger::AccountKind;
    use payment_gateway::shared::shared_key;
    let mut blocker = h.pool.begin().await.unwrap();
    for _ in 0..200 {
        sqlx::query("insert into dl_weight (v) values (1)")
            .execute(&mut *blocker)
            .await
            .unwrap();
    }
    sqlx::query("select id from ledger_account where tenant_id = ? and kind = ? for update")
        .bind(tenant_id)
        .bind(AccountKind::TenantBalance.as_str())
        .fetch_all(&mut *blocker)
        .await
        .unwrap();

    let settling = {
        let s = sink(h, shared_key(shared_id));
        let hash = hash.to_string();
        tokio::spawn(async move { s.event(paid(&hash, &hash, None, true), None).await })
    };
    for _ in 0..400 {
        let waiting: i64 = sqlx::query_scalar(
            "select count(*) from information_schema.innodb_trx where trx_state = 'LOCK WAIT' and trx_query like '%ledger_account%'",
        )
        .fetch_one(&h.pool)
        .await
        .unwrap();
        if waiting > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    let cycle = sqlx::query("select id from payment_attempt where id = ? for update")
        .bind(attempt_id)
        .fetch_all(&mut *blocker)
        .await;
    drop(blocker);
    let _ = cycle;
    settling.await.unwrap().unwrap();
}

#[tokio::test]
async fn a_deadlock_retry_on_a_shared_settlement_credits_exactly_once() {
    use payment_gateway::ledger;
    use payment_gateway::shared::shared_key;
    let h = harness().await;
    let t = tenant(&h).await;
    let (s, r) = call(
        &h.app,
        "POST",
        "/admin/shared",
        Some(ADMIN),
        json!({"name": "dl", "method": "lightning", "provider_type": "mock", "network": "regtest",
               "config": {}, "fee_bps": 100, "fee_base_msat": 0, "allow_untrusted": true}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let sid = r["id"].as_u64().unwrap();
    let tenant_row = payment_gateway::db::get_tenant(&h.pool, t.id)
        .await
        .unwrap()
        .unwrap();
    h.service.enable_shared(&tenant_row, sid).await.unwrap();

    let (first_hash, _) = shared_payment_hash(&h, &t, 5000).await;
    sink(&h, shared_key(sid))
        .event(paid(&first_hash, &first_hash, None, true), None)
        .await
        .unwrap();
    assert_eq!(
        ledger::tenant_balance(&h.pool, t.id, "BTC").await.unwrap(),
        99_000_000
    );

    sqlx::query("create table dl_weight (id int auto_increment primary key, v int)")
        .execute(&h.pool)
        .await
        .unwrap();
    let mut attempts = vec![];
    let mut hit = false;
    for _ in 0..5 {
        let (hash, attempt_id) = shared_payment_hash(&h, &t, 5000).await;
        let before = innodb_deadlocks(&h).await;
        try_to_deadlock_settlement(&h, t.id, sid, &hash, attempt_id).await;
        hit |= innodb_deadlocks(&h).await > before;
        sink(&h, shared_key(sid))
            .event(paid(&hash, &hash, None, true), None)
            .await
            .unwrap();
        attempts.push(attempt_id);
        if hit {
            break;
        }
    }
    assert!(hit, "no attempt produced a deadlock in 5 tries");
    let settled = attempts.len() as i64 + 1;
    assert_eq!(
        ledger::tenant_balance(&h.pool, t.id, "BTC").await.unwrap(),
        99_000_000 * settled
    );
    let fees: i64 = sqlx::query_scalar(
        "select cast(coalesce(sum(balance), 0) as signed) from ledger_account where tenant_id is null and kind = 'platform_fees'",
    )
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!(fees, 1_000_000 * settled);
    for attempt_id in attempts {
        let postings: i64 =
            sqlx::query_scalar("select count(*) from ledger_transaction where idempotency_key = ?")
                .bind(format!("settle:{attempt_id}"))
                .fetch_one(&h.pool)
                .await
                .unwrap();
        assert_eq!(postings, 1);
    }
    assert!(ledger::reconcile(&h.pool).await.unwrap().is_empty());
}
