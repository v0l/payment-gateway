mod common;
use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::*;
use payment_gateway::db;
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::{ProviderSink, webhook_signature};
use serde_json::{Value, json};

async fn paid_lightning(h: &Harness, t: &TenantFixture, provider_id: u64, cents: u64) -> String {
    let p = new_payment(h, t, eur("o", cents)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: h.service.clone(),
        provider_id,
    }
    .event(
        ProviderEvent::Paid {
            lookup: hash.clone(),
            reference: hash,
            amount: None,
            confirmed: true,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();
    id
}

#[tokio::test]
async fn webhooks_are_signed_with_the_tenant_secret() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    paid_lightning(&h, &t, ln, 1000).await;
    h.service.deliver_due().await.unwrap();

    let got = t.receiver.got.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    let (headers, body) = &got[0];
    let sig = headers["x-gateway-signature"].to_str().unwrap();
    let (ts, v1) = sig.strip_prefix("t=").unwrap().split_once(",v1=").unwrap();
    assert_eq!(
        v1,
        webhook_signature(&t.webhook_secret, ts.parse().unwrap(), body)
    );
    let event: Value = serde_json::from_str(body).unwrap();
    assert_eq!(
        headers["x-gateway-event-id"].to_str().unwrap(),
        event["id"].as_str().unwrap()
    );
    assert_eq!(event["type"], "payment.paid");
    assert_eq!(event["data"]["payment"]["status"], "paid");
    assert_eq!(event["data"]["payment"]["reference"], "o");
}

#[tokio::test]
async fn failed_deliveries_back_off_then_succeed_with_the_same_event_id() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    t.receiver.fail_with(500);
    paid_lightning(&h, &t, ln, 1000).await;

    assert_eq!(h.service.deliver_due().await.unwrap(), 0);
    assert_eq!(
        h.service.deliver_due().await.unwrap(),
        0,
        "backed off, not retried immediately"
    );
    assert_eq!(t.receiver.got.lock().unwrap().len(), 1);

    let (_, list) = call(
        &h.app,
        "GET",
        "/admin/deliveries?undelivered=1",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_eq!(list[0]["attempts"], 1);
    assert!(list[0]["last_error"].as_str().unwrap().contains("500"));

    sqlx::query("update webhook_delivery set next_attempt = ?")
        .bind(Utc::now() - Duration::seconds(1))
        .execute(&h.pool)
        .await
        .unwrap();
    t.receiver.fail_with(200);
    assert_eq!(h.service.deliver_due().await.unwrap(), 1);
    let events = t.receiver.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["id"], events[1]["id"]);
}

#[tokio::test]
async fn deliveries_are_abandoned_after_three_days_and_can_be_replayed() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    t.receiver.fail_with(500);
    paid_lightning(&h, &t, ln, 1000).await;
    sqlx::query("update webhook_delivery set created = ?, next_attempt = ?")
        .bind(Utc::now() - Duration::days(4))
        .bind(Utc::now() - Duration::seconds(1))
        .execute(&h.pool)
        .await
        .unwrap();
    h.service.deliver_due().await.unwrap();

    let all = db::list_deliveries(&h.pool, Some(t.id), false)
        .await
        .unwrap();
    assert!(all[0].abandoned_at.is_some());
    assert_eq!(h.service.deliver_due().await.unwrap(), 0);

    t.receiver.fail_with(200);
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/admin/deliveries/{}/replay", all[0].id),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.service.deliver_due().await.unwrap(), 1);
}

#[tokio::test]
async fn concurrent_delivery_runs_send_each_event_once() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    paid_lightning(&h, &t, ln, 1000).await;
    let (a, b) = tokio::join!(h.service.deliver_due(), h.service.deliver_due());
    assert_eq!(a.unwrap() + b.unwrap(), 1);
    assert_eq!(t.receiver.got.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn tenants_without_a_webhook_url_queue_nothing() {
    let h = harness().await;
    let (_, t) = call(
        &h.app,
        "POST",
        "/admin/tenants",
        Some(ADMIN),
        json!({"name": unique("quiet")}),
    )
    .await;
    let key = t["api_key"].as_str().unwrap();
    let tid = t["id"].as_u64().unwrap();
    let (_, p) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{tid}/providers"),
        Some(ADMIN),
        json!({"method": "lightning", "provider_type": "mock", "name": "ln", "config": {}}),
    )
    .await;
    let (_, pay) = call(&h.app, "POST", "/v1/payments", Some(key), eur("o", 100)).await;
    let id = pay["id"].as_str().unwrap();
    let (_, r) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/attempts"),
        Some(key),
        json!({"method": "lightning"}),
    )
    .await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: h.service.clone(),
        provider_id: p["id"].as_u64().unwrap(),
    }
    .event(
        ProviderEvent::Paid {
            lookup: hash.clone(),
            reference: hash,
            amount: None,
            confirmed: true,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();
    assert!(
        db::list_deliveries(&h.pool, Some(tid), false)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn full_refund_marks_refunded_and_calls_the_provider_with_the_charged_amount() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let id = paid_lightning(&h, &t, ln, 5000).await;
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/refund"),
        Some(&t.key),
        json!({"destination": "lnbc1refund"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "completed");
    assert_eq!(r["amount"], 5000);
    let sent = mock(&h, ln).refunds.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].amount, 100_000_000);
    assert_eq!(sent[0].currency, "BTC");
    assert_eq!(sent[0].destination.as_deref(), Some("lnbc1refund"));
    assert_eq!(get_payment(&h, &t, &id).await["status"], "refunded");

    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/refund"),
        Some(&t.key),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    h.service.deliver_due().await.unwrap();
    assert_eq!(t.receiver.types(), vec!["payment.paid", "refund.completed"]);
}

#[tokio::test]
async fn partial_refunds_accumulate_and_cannot_exceed_the_payment() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let id = paid_lightning(&h, &t, ln, 5000).await;
    let uri = format!("/v1/payments/{id}/refund");

    let (s, _) = call(&h.app, "POST", &uri, Some(&t.key), json!({"amount": 2000})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        get_payment(&h, &t, &id).await["status"],
        "partially_refunded"
    );
    let (s, _) = call(&h.app, "POST", &uri, Some(&t.key), json!({"amount": 3001})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(&h.app, "POST", &uri, Some(&t.key), json!({"amount": 0})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(&h.app, "POST", &uri, Some(&t.key), json!({"amount": 3000})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(get_payment(&h, &t, &id).await["status"], "refunded");
    let total: u64 = mock(&h, ln)
        .refunds
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.amount)
        .sum();
    assert_eq!(
        total, 100_000_000,
        "provider got exactly what was charged, no more"
    );
}

#[tokio::test]
async fn failed_provider_refund_is_recorded_and_can_be_retried() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let id = paid_lightning(&h, &t, ln, 5000).await;
    let uri = format!("/v1/payments/{id}/refund");
    *mock(&h, ln).fail_refund.lock().unwrap() = true;
    let (s, r) = call(&h.app, "POST", &uri, Some(&t.key), json!({})).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["status"], "failed");
    assert_eq!(get_payment(&h, &t, &id).await["status"], "paid");

    *mock(&h, ln).fail_refund.lock().unwrap() = false;
    let (_, r) = call(&h.app, "POST", &uri, Some(&t.key), json!({})).await;
    assert_eq!(r["status"], "completed");
    assert_eq!(get_payment(&h, &t, &id).await["status"], "refunded");
}

#[tokio::test]
async fn unpaid_payments_cannot_be_refunded() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(&h, &t, eur("o", 100)).await;
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{}/refund", p["id"].as_str().unwrap()),
        Some(&t.key),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

async fn paid_with_saved_card(h: &Harness, t: &TenantFixture, stripe: u64) -> String {
    let p = new_payment(h, t, json!({"reference": "first", "amount": 1000, "currency": "EUR", "customer_ref": "cust-1", "customer_email": "a@b.c"})).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, r) = attempt(h, t, &id, json!({"method": "stripe", "save_method": true})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    let lookup = db::get_attempt(&h.pool, aid).await.unwrap().unwrap().lookup;
    let hook = send(
        &h.app,
        "POST",
        &format!("/hook/{}/{stripe}", t.id),
        None,
        &[("x-mock-secret", "mock-secret")],
        json!({"lookup": lookup, "saved": "pm_card_1"}),
    )
    .await;
    assert_eq!(hook.status, StatusCode::OK, "{}", hook.text);
    id
}

#[tokio::test]
async fn saving_a_card_needs_a_customer_and_a_card_method() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "stripe", json!({}), None).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    let anon = new_payment(&h, &t, eur("a", 100)).await;
    let (s, _) = attempt(
        &h,
        &t,
        anon["id"].as_str().unwrap(),
        json!({"method": "stripe", "save_method": true}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let known = new_payment(
        &h,
        &t,
        json!({"reference": "k", "amount": 100, "currency": "EUR", "customer_ref": "c"}),
    )
    .await;
    let (s, _) = attempt(
        &h,
        &t,
        known["id"].as_str().unwrap(),
        json!({"method": "lightning", "save_method": true}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn saved_card_is_listed_and_charged_off_session_and_settles_at_once() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    paid_with_saved_card(&h, &t, st).await;

    let (_, saved) = call(
        &h.app,
        "GET",
        "/v1/saved-methods?customer_ref=cust-1",
        Some(&t.key),
        Value::Null,
    )
    .await;
    assert_eq!(saved.as_array().unwrap().len(), 1);
    assert_eq!(saved[0]["last4"], "4242");
    assert!(
        !saved.to_string().contains("pm_card_1"),
        "provider ids stay private"
    );
    let sid = saved[0]["id"].as_u64().unwrap();

    let (s, r) = send_charge(
        &h,
        &t,
        sid,
        "renew-1",
        json!({"reference": "renewal", "amount": 700, "currency": "EUR"}),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    assert_eq!(r["status"], "paid");
    assert_eq!(r["customer_ref"], "cust-1");

    let (s, again) = send_charge(
        &h,
        &t,
        sid,
        "renew-1",
        json!({"reference": "renewal", "amount": 700, "currency": "EUR"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(again["id"], r["id"]);
    assert_eq!(
        mock(&h, st).created.lock().unwrap().len(),
        2,
        "charged once"
    );
}

async fn send_charge(
    h: &Harness,
    t: &TenantFixture,
    sid: u64,
    key: &str,
    body: Value,
) -> (StatusCode, Value) {
    let r = send(
        &h.app,
        "POST",
        &format!("/v1/saved-methods/{sid}/charge"),
        Some(&t.key),
        &[("idempotency-key", key)],
        body,
    )
    .await;
    (r.status, r.json)
}

#[tokio::test]
async fn failed_saved_charge_cancels_the_payment() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    paid_with_saved_card(&h, &t, st).await;
    let (_, saved) = call(
        &h.app,
        "GET",
        "/v1/saved-methods?customer_ref=cust-1",
        Some(&t.key),
        Value::Null,
    )
    .await;
    let sid = saved[0]["id"].as_u64().unwrap();
    *mock(&h, st).fail_create.lock().unwrap() = true;
    let (s, _) = send_charge(
        &h,
        &t,
        sid,
        "k",
        json!({"reference": "r", "amount": 100, "currency": "EUR"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    let (_, found) = call(
        &h.app,
        "GET",
        "/v1/payments?reference=r",
        Some(&t.key),
        Value::Null,
    )
    .await;
    assert_eq!(found[0]["status"], "cancelled");
}

#[tokio::test]
async fn saved_methods_are_private_to_their_tenant_and_deletable() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let st = provider(&h, &a, "stripe", json!({}), None).await;
    provider(&h, &b, "stripe", json!({}), None).await;
    paid_with_saved_card(&h, &a, st).await;
    let (_, saved) = call(
        &h.app,
        "GET",
        "/v1/saved-methods?customer_ref=cust-1",
        Some(&a.key),
        Value::Null,
    )
    .await;
    let sid = saved[0]["id"].as_u64().unwrap();

    let (s, _) = send_charge(&h, &b, sid, "k", eur("x", 100)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, none) = call(
        &h.app,
        "GET",
        "/v1/saved-methods?customer_ref=cust-1",
        Some(&b.key),
        Value::Null,
    )
    .await;
    assert!(none.as_array().unwrap().is_empty());
    let (s, _) = call(
        &h.app,
        "DELETE",
        &format!("/v1/saved-methods/{sid}"),
        Some(&b.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, _) = call(
        &h.app,
        "DELETE",
        &format!("/v1/saved-methods/{sid}"),
        Some(&a.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = send_charge(&h, &a, sid, "k2", eur("x", 100)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
