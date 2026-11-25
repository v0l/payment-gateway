mod common;
use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::*;
use payment_gateway::providers::EventSink;
use payment_gateway::providers::ProviderEvent;
use payment_gateway::service::ProviderSink;
use serde_json::{Value, json};

fn paid(lookup: &str, reference: &str, amount: Option<u64>, confirmed: bool) -> ProviderEvent {
    ProviderEvent::Paid {
        lookup: lookup.into(),
        reference: reference.into(),
        amount,
        confirmed,
        saved: None,
    }
}

async fn sink_for(h: &Harness, provider_id: u64) -> ProviderSink {
    ProviderSink {
        service: h.service.clone(),
        provider_id,
    }
}

async fn ln_attempt(h: &Harness, t: &TenantFixture, cents: u64) -> (String, String, u64) {
    let p = new_payment(h, t, eur("o", cents)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
    let lookup = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    (id, lookup, aid)
}

#[tokio::test]
async fn lightning_settlement_marks_paid_and_sends_one_webhook() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let (id, hash, _) = ln_attempt(&h, &t, 1000).await;

    let sink = sink_for(&h, ln).await;
    sink.event(paid(&hash, &hash, None, true), Some(hash.clone()))
        .await
        .unwrap();
    sink.event(paid(&hash, &hash, None, true), Some(hash.clone()))
        .await
        .unwrap();

    let got = get_payment(&h, &t, &id).await;
    assert_eq!(got["status"], "paid");
    assert!(got["paid_at"].is_string());
    assert_eq!(got["attempts"][0]["status"], "paid");
    assert_eq!(db_cursor(&h, ln).await.as_deref(), Some(hash.as_str()));

    assert_eq!(h.service.deliver_due().await.unwrap(), 1);
    assert_eq!(t.receiver.types(), vec!["payment.paid"]);
    assert_eq!(h.service.deliver_due().await.unwrap(), 0);
}

async fn db_cursor(h: &Harness, provider: u64) -> Option<String> {
    payment_gateway::db::get_cursor(&h.pool, provider)
        .await
        .unwrap()
}

#[tokio::test]
async fn unknown_lookups_are_ignored() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let sink = sink_for(&h, ln).await;
    sink.event(paid("deadbeef", "deadbeef", None, true), None)
        .await
        .unwrap();
    assert_eq!(h.service.deliver_due().await.unwrap(), 0);
}

#[tokio::test]
async fn watcher_resumes_from_the_stored_cursor_after_restart() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let (id, hash, _) = ln_attempt(&h, &t, 1000).await;
    let m = mock(&h, ln);
    m.emit(paid(&hash, &hash, None, true), Some("cursor-1"));

    let workers =
        payment_gateway::worker::spawn(h.service.clone(), std::time::Duration::from_millis(100));
    for _ in 0..100 {
        if get_payment(&h, &t, &id).await["status"] == "paid" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(get_payment(&h, &t, &id).await["status"], "paid");
    assert_eq!(m.watch_starts.lock().unwrap()[0], None);
    for _ in 0..50 {
        if db_cursor(&h, ln).await.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(db_cursor(&h, ln).await.as_deref(), Some("cursor-1"));
    drop(workers);

    let (id2, hash2, _) = ln_attempt(&h, &t, 2000).await;
    m.emit(paid(&hash2, &hash2, None, true), Some("cursor-2"));
    let _workers =
        payment_gateway::worker::spawn(h.service.clone(), std::time::Duration::from_millis(100));
    for _ in 0..100 {
        if get_payment(&h, &t, &id2).await["status"] == "paid" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(get_payment(&h, &t, &id2).await["status"], "paid");
    let starts = m.watch_starts.lock().unwrap().clone();
    assert_eq!(starts.len(), 2, "old watcher must have stopped: {starts:?}");
    assert_eq!(starts[1].as_deref(), Some("cursor-1"));
}

#[tokio::test]
async fn expiry_marks_payment_and_attempts_expired_and_notifies_once() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let (id, hash, _) = ln_attempt(&h, &t, 1000).await;
    sqlx::query("update payment set expires = ?")
        .bind(Utc::now() - Duration::seconds(5))
        .execute(&h.pool)
        .await
        .unwrap();

    assert_eq!(h.service.expire_due().await.unwrap(), 1);
    assert_eq!(h.service.expire_due().await.unwrap(), 0);
    let got = get_payment(&h, &t, &id).await;
    assert_eq!(got["status"], "expired");
    assert_eq!(got["attempts"][0]["status"], "expired");
    assert!(mock(&h, ln).cancelled.lock().unwrap().contains(&hash));
    h.service.deliver_due().await.unwrap();
    assert_eq!(t.receiver.types(), vec!["payment.expired"]);
}

#[tokio::test]
async fn late_payment_after_expiry_is_recorded_without_reopening() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let (id, hash, _) = ln_attempt(&h, &t, 1000).await;
    sqlx::query("update payment set expires = ?")
        .bind(Utc::now() - Duration::seconds(5))
        .execute(&h.pool)
        .await
        .unwrap();
    h.service.expire_due().await.unwrap();

    sink_for(&h, ln)
        .await
        .event(paid(&hash, &hash, None, true), None)
        .await
        .unwrap();
    assert_eq!(get_payment(&h, &t, &id).await["status"], "paid");
    h.service.deliver_due().await.unwrap();
    assert_eq!(t.receiver.types(), vec!["payment.expired", "payment.paid"]);
}

#[tokio::test]
async fn paying_one_attempt_cancels_the_others() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    let p = new_payment(&h, &t, eur("o", 1000)).await;
    let id = p["id"].as_str().unwrap();
    let (_, r) = attempt(&h, &t, id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, r) = attempt(&h, &t, id, json!({"method": "stripe"})).await;
    assert_eq!(r["attempts"].as_array().unwrap().len(), 2);

    sink_for(&h, ln)
        .await
        .event(paid(&hash, &hash, None, true), None)
        .await
        .unwrap();
    let got = get_payment(&h, &t, id).await;
    let statuses: Vec<&str> = got["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["status"].as_str().unwrap())
        .collect();
    assert_eq!(statuses, vec!["paid", "cancelled"]);
    assert_eq!(mock(&h, st).cancelled.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn concurrent_duplicate_settlements_pay_once() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let (id, hash, _) = ln_attempt(&h, &t, 1000).await;
    let mut tasks = vec![];
    for _ in 0..6 {
        let service = h.service.clone();
        let hash = hash.clone();
        tasks.push(tokio::spawn(async move {
            ProviderSink {
                service,
                provider_id: ln,
            }
            .event(paid(&hash, &hash, None, true), None)
            .await
        }));
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    assert_eq!(get_payment(&h, &t, &id).await["status"], "paid");
    for _ in 0..60 {
        h.service.deliver_due().await.unwrap();
        if !t.receiver.types().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(t.receiver.types(), vec!["payment.paid"]);
}

async fn onchain_attempt(h: &Harness, t: &TenantFixture, cents: u64) -> (String, String, u64) {
    let p = new_payment(h, t, eur("oc", cents)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, t, &id, json!({"method": "onchain"})).await;
    let address = r["attempts"][0]["details"]["address"]
        .as_str()
        .unwrap()
        .to_string();
    let required = r["attempts"][0]["amount"].as_u64().unwrap();
    (id, address, required)
}

#[tokio::test]
async fn onchain_pays_only_when_confirmed_and_sufficient() {
    let h = harness().await;
    let t = tenant(&h).await;
    let oc = provider(&h, &t, "onchain", json!({}), None).await;
    let (id, addr, required) = onchain_attempt(&h, &t, 5000).await;
    let sink = sink_for(&h, oc).await;

    sink.event(paid(&addr, "tx1:0", Some(required), false), None)
        .await
        .unwrap();
    assert_eq!(get_payment(&h, &t, &id).await["status"], "pending");

    sink.event(paid(&addr, "tx1:0", Some(required / 2), true), None)
        .await
        .unwrap();
    assert_eq!(
        get_payment(&h, &t, &id).await["status"],
        "pending",
        "underpayment"
    );

    sink.event(paid(&addr, "tx2:1", Some(required / 2), true), None)
        .await
        .unwrap();
    assert_eq!(
        get_payment(&h, &t, &id).await["status"],
        "paid",
        "two outputs add up"
    );
}

async fn drain_types(h: &Harness, t: &TenantFixture) -> Vec<String> {
    for _ in 0..5 {
        h.service.deliver_due().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    t.receiver.types()
}

#[tokio::test]
async fn unconfirmed_deposit_emits_attempt_received_once_then_paid() {
    let h = harness().await;
    let t = tenant(&h).await;
    let oc = provider(&h, &t, "onchain", json!({}), None).await;
    let (id, addr, required) = onchain_attempt(&h, &t, 5000).await;
    let sink = sink_for(&h, oc).await;
    sink.event(paid(&addr, "tx1:0", Some(required), false), None)
        .await
        .unwrap();
    sink.event(paid(&addr, "tx1:0", Some(required), false), None)
        .await
        .unwrap();
    assert_eq!(drain_types(&h, &t).await, vec!["attempt.received"]);
    let events = t.receiver.events();
    let detail = &events[0]["data"]["detail"];
    assert_eq!(detail["txid"], "tx1");
    assert_eq!(detail["reference"], "tx1:0");
    assert_eq!(detail["method"], "onchain");
    assert_eq!(detail["amount"], required);
    assert_eq!(get_payment(&h, &t, &id).await["status"], "pending");

    sink.event(paid(&addr, "tx1:0", Some(required), true), None)
        .await
        .unwrap();
    assert_eq!(get_payment(&h, &t, &id).await["status"], "paid");
    assert_eq!(
        drain_types(&h, &t).await,
        vec!["attempt.received", "payment.paid"]
    );
}

#[tokio::test]
async fn confirmed_first_deposit_emits_only_payment_paid() {
    let h = harness().await;
    let t = tenant(&h).await;
    let oc = provider(&h, &t, "onchain", json!({}), None).await;
    let (_, addr, required) = onchain_attempt(&h, &t, 5000).await;
    sink_for(&h, oc)
        .await
        .event(paid(&addr, "tx9:0", Some(required), true), None)
        .await
        .unwrap();
    assert_eq!(drain_types(&h, &t).await, vec!["payment.paid"]);
}

#[tokio::test]
async fn onchain_confirmation_of_the_same_output_is_not_double_counted() {
    let h = harness().await;
    let t = tenant(&h).await;
    let oc = provider(&h, &t, "onchain", json!({}), None).await;
    let (id, addr, required) = onchain_attempt(&h, &t, 5000).await;
    let sink = sink_for(&h, oc).await;
    let half = required / 2;
    sink.event(paid(&addr, "tx1:0", Some(half), false), None)
        .await
        .unwrap();
    sink.event(paid(&addr, "tx1:0", Some(half), true), None)
        .await
        .unwrap();
    sink.event(paid(&addr, "tx1:0", Some(half), true), None)
        .await
        .unwrap();
    assert_eq!(get_payment(&h, &t, &id).await["status"], "pending");
}

#[tokio::test]
async fn unconfirmed_onchain_funds_hold_the_payment_open_past_expiry() {
    let h = harness().await;
    let t = tenant(&h).await;
    let oc = provider(&h, &t, "onchain", json!({}), None).await;
    let (id, addr, required) = onchain_attempt(&h, &t, 5000).await;
    sink_for(&h, oc)
        .await
        .event(paid(&addr, "tx1:0", Some(required), false), None)
        .await
        .unwrap();
    sqlx::query("update payment set expires = ?")
        .bind(Utc::now() - Duration::seconds(5))
        .execute(&h.pool)
        .await
        .unwrap();

    assert_eq!(h.service.expire_due().await.unwrap(), 0);
    assert_eq!(get_payment(&h, &t, &id).await["status"], "pending");

    sink_for(&h, oc)
        .await
        .event(paid(&addr, "tx1:0", Some(required), true), None)
        .await
        .unwrap();
    assert_eq!(get_payment(&h, &t, &id).await["status"], "paid");
}

#[tokio::test]
async fn unconfirmed_funds_cannot_be_cancelled() {
    let h = harness().await;
    let t = tenant(&h).await;
    let oc = provider(&h, &t, "onchain", json!({}), None).await;
    let (id, addr, required) = onchain_attempt(&h, &t, 5000).await;
    sink_for(&h, oc)
        .await
        .event(paid(&addr, "tx1:0", Some(required), false), None)
        .await
        .unwrap();
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/cancel"),
        Some(&t.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn overpayment_is_accepted() {
    let h = harness().await;
    let t = tenant(&h).await;
    let oc = provider(&h, &t, "onchain", json!({}), None).await;
    let (id, addr, required) = onchain_attempt(&h, &t, 5000).await;
    sink_for(&h, oc)
        .await
        .event(paid(&addr, "tx1:0", Some(required * 2), true), None)
        .await
        .unwrap();
    assert_eq!(get_payment(&h, &t, &id).await["status"], "paid");
}

#[tokio::test]
async fn fixedfloat_settles_when_the_swap_lands_on_our_onchain_address() {
    let h = harness().await;
    let t = tenant(&h).await;
    let oc = provider(&h, &t, "onchain", json!({}), None).await;
    provider(&h, &t, "fixedfloat", json!({"coins": ["XMR"]}), None).await;
    let p = new_payment(&h, &t, eur("o", 5000)).await;
    let id = p["id"].as_str().unwrap();
    let (s, r) = attempt(&h, &t, id, json!({"method": "fixedfloat", "option": "XMR"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let receive = r["attempts"][0]["details"]["receive_address"]
        .as_str()
        .unwrap()
        .to_string();
    let required = r["attempts"][0]["amount"].as_u64().unwrap();

    sink_for(&h, oc)
        .await
        .event(paid(&receive, "swap:0", Some(required), true), None)
        .await
        .unwrap();
    let got = get_payment(&h, &t, id).await;
    assert_eq!(got["status"], "paid");
    assert_eq!(got["attempts"][0]["method"], "fixedfloat");
}

#[tokio::test]
async fn cancel_cancels_attempts_and_notifies() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let (id, hash, _) = ln_attempt(&h, &t, 1000).await;
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/cancel"),
        Some(&t.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["status"], "cancelled");
    assert_eq!(r["attempts"][0]["status"], "cancelled");
    assert!(mock(&h, ln).cancelled.lock().unwrap().contains(&hash));
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/cancel"),
        Some(&t.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    h.service.deliver_due().await.unwrap();
    assert_eq!(t.receiver.types(), vec!["payment.cancelled"]);
}

#[tokio::test]
async fn failed_attempt_notifies_but_keeps_the_payment_open() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    let p = new_payment(&h, &t, eur("o", 1000)).await;
    let id = p["id"].as_str().unwrap();
    let (_, r) = attempt(&h, &t, id, json!({"method": "stripe"})).await;
    let lookup = r["attempts"][0]["details"]["checkout_url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    let _ = lookup;
    let a = &r["attempts"][0];
    let ext = payment_gateway::db::get_attempt(&h.pool, a["id"].as_u64().unwrap())
        .await
        .unwrap()
        .unwrap()
        .lookup;
    sink_for(&h, st)
        .await
        .event(
            ProviderEvent::Failed {
                lookup: ext,
                reason: "declined".into(),
            },
            None,
        )
        .await
        .unwrap();
    let got = get_payment(&h, &t, id).await;
    assert_eq!(got["status"], "pending");
    assert_eq!(got["attempts"][0]["status"], "failed");
    h.service.deliver_due().await.unwrap();
    assert_eq!(t.receiver.types(), vec!["attempt.failed"]);
}

#[tokio::test]
async fn provider_webhook_endpoint_verifies_and_settles() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({"secret": "s3cret"}), None).await;
    let p = new_payment(&h, &t, eur("o", 1000)).await;
    let id = p["id"].as_str().unwrap();
    let (_, r) = attempt(&h, &t, id, json!({"method": "stripe"})).await;
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    let lookup = payment_gateway::db::get_attempt(&h.pool, aid)
        .await
        .unwrap()
        .unwrap()
        .lookup;
    let uri = format!("/hook/{}/{st}", t.id);

    let bad = send(
        &h.app,
        "POST",
        &uri,
        None,
        &[("x-mock-secret", "wrong")],
        json!({"lookup": lookup}),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert_eq!(get_payment(&h, &t, id).await["status"], "pending");

    let wrong_tenant = send(
        &h.app,
        "POST",
        &format!("/hook/{}/{st}", t.id + 999),
        None,
        &[("x-mock-secret", "s3cret")],
        json!({"lookup": lookup}),
    )
    .await;
    assert_eq!(wrong_tenant.status, StatusCode::NOT_FOUND);

    let ok = send(
        &h.app,
        "POST",
        &uri,
        None,
        &[("x-mock-secret", "s3cret")],
        json!({"lookup": lookup, "amount": 1000}),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert_eq!(get_payment(&h, &t, id).await["status"], "paid");
}

#[tokio::test]
async fn provider_webhook_for_another_tenants_provider_is_not_found() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let st = provider(&h, &a, "stripe", json!({}), None).await;
    let r = send(
        &h.app,
        "POST",
        &format!("/hook/{}/{st}", b.id),
        None,
        &[("x-mock-secret", "mock-secret")],
        json!({"lookup": "x"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invoice_settled_while_no_watcher_was_running_is_still_paid() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let (id, hash, _) = ln_attempt(&h, &t, 1000).await;
    mock(&h, ln).mark_settled(&hash);

    let _workers =
        payment_gateway::worker::spawn(h.service.clone(), std::time::Duration::from_millis(100));
    for _ in 0..60 {
        if get_payment(&h, &t, &id).await["status"] == "paid" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(get_payment(&h, &t, &id).await["status"], "paid");
    for _ in 0..60 {
        h.service.deliver_due().await.unwrap();
        if !t.receiver.types().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(t.receiver.types(), vec!["payment.paid"]);
}
