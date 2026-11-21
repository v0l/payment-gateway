mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::db;
use serde_json::{Value, json};

struct Fixture {
    h: Harness,
    t: TenantFixture,
    clock: std::sync::Arc<ManualClock>,
    stripe: u64,
    saved: u64,
}

async fn fixture(start: (i32, u32, u32)) -> Fixture {
    let clock = ManualClock::at(start.0, start.1, start.2);
    let h = harness_with_clock(clock.clone()).await;
    let t = tenant(&h).await;
    let stripe = provider(&h, &t, "stripe", json!({}), None).await;
    let saved = save_card(&h, &t, stripe, "cust-1").await;
    Fixture {
        h,
        t,
        clock,
        stripe,
        saved,
    }
}

async fn save_card(h: &Harness, t: &TenantFixture, stripe: u64, customer: &str) -> u64 {
    let p = new_payment(
        h,
        t,
        json!({"reference": format!("first-{customer}"), "amount": 100, "currency": "EUR", "customer_ref": customer}),
    )
    .await;
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
        json!({"lookup": lookup, "saved": format!("pm_{customer}")}),
    )
    .await;
    assert_eq!(hook.status, StatusCode::OK, "{}", hook.text);
    let (_, saved) = call(
        &h.app,
        "GET",
        &format!("/v1/saved-methods?customer_ref={customer}"),
        Some(&t.key),
        Value::Null,
    )
    .await;
    saved[0]["id"].as_u64().unwrap()
}

async fn plan(f: &Fixture, body: Value) -> u64 {
    let (s, r) = call(&f.h.app, "POST", "/v1/plans", Some(&f.t.key), body).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    r["id"].as_u64().unwrap()
}

fn monthly(amount: u64) -> Value {
    json!({"name": "Pro", "amount": amount, "currency": "EUR", "interval": "month"})
}

async fn subscribe(f: &Fixture, plan_id: u64, saved: Option<u64>) -> Value {
    let mut body = json!({"customer_ref": "cust-1", "plan_id": plan_id});
    if let Some(s) = saved {
        body["saved_method_id"] = json!(s);
    }
    let (s, r) = call(&f.h.app, "POST", "/v1/subscriptions", Some(&f.t.key), body).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    r
}

async fn sub(f: &Fixture, id: u64) -> Value {
    let (s, r) = call(
        &f.h.app,
        "GET",
        &format!("/v1/subscriptions/{id}"),
        Some(&f.t.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    r
}

async fn act(f: &Fixture, id: u64, action: &str, body: Value) -> (StatusCode, Value) {
    call(
        &f.h.app,
        "POST",
        &format!("/v1/subscriptions/{id}/{action}"),
        Some(&f.t.key),
        body,
    )
    .await
}

async fn events(f: &Fixture) -> Vec<String> {
    for _ in 0..6 {
        f.h.service.deliver_due().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    f.t.receiver
        .types()
        .into_iter()
        .filter(|t| t.starts_with("subscription.") || t.starts_with("invoice."))
        .collect()
}

async fn charges(f: &Fixture) -> usize {
    let (_, r) = call(
        &f.h.app,
        "GET",
        "/v1/payments?status=paid&limit=100",
        Some(&f.t.key),
        Value::Null,
    )
    .await;
    r.as_array()
        .unwrap()
        .iter()
        .filter(|p| p["reference"].as_str().unwrap().starts_with("sub_"))
        .count()
}

#[tokio::test]
async fn billing_run_metrics_move_after_a_run() {
    let f = fixture((2030, 1, 31)).await;
    let m = &f.h.service.metrics;
    let p = plan(&f, monthly(1500)).await;
    subscribe(&f, p, Some(f.saved)).await;
    f.clock.advance_days(29);
    let runs = m.counter("gateway_billing_runs_total", &[]);
    let charged = m.counter("gateway_billing_charges_total", &[("outcome", "charged")]);
    f.h.service.bill_due().await.unwrap();
    assert_eq!(m.counter("gateway_billing_runs_total", &[]), runs + 1);
    assert_eq!(
        m.counter("gateway_billing_charges_total", &[("outcome", "charged")]),
        charged + 1
    );
    assert!(m.counter("gateway_billing_charges_total", &[("outcome", "created")]) >= 1);
}

#[tokio::test]
async fn saved_method_subscription_is_charged_at_once_and_each_period() {
    let f = fixture((2030, 1, 31)).await;
    let p = plan(&f, monthly(1500)).await;
    let s = subscribe(&f, p, Some(f.saved)).await;
    let id = s["id"].as_u64().unwrap();
    let s = sub(&f, id).await;
    assert_eq!(s["status"], "active");
    assert_eq!(charges(&f).await, 1);
    assert!(
        s["current_period_end"]
            .as_str()
            .unwrap()
            .starts_with("2030-02-28")
    );
    f.clock.advance_days(29);
    assert_eq!(f.h.service.bill_due().await.unwrap(), 1);
    assert_eq!(charges(&f).await, 2);
    let s = sub(&f, id).await;
    assert!(
        s["current_period_end"]
            .as_str()
            .unwrap()
            .starts_with("2030-03-31")
    );
    assert_eq!(s["periods"].as_array().unwrap().len(), 2);
    assert_eq!(
        events(&f).await,
        vec!["subscription.created", "invoice.paid", "invoice.paid"]
    );
}

#[tokio::test]
async fn leap_year_february_period_ends_on_the_29th() {
    let f = fixture((2032, 1, 31)).await;
    let p = plan(&f, monthly(500)).await;
    let id = subscribe(&f, p, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    let s = sub(&f, id).await;
    assert!(
        s["current_period_end"]
            .as_str()
            .unwrap()
            .starts_with("2032-02-29")
    );
}

#[tokio::test]
async fn concurrent_workers_charge_a_period_exactly_once() {
    let f = fixture((2030, 1, 10)).await;
    let p = plan(&f, monthly(1500)).await;
    let id = subscribe(&f, p, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    f.clock.advance_days(31);
    let mut tasks = vec![];
    for _ in 0..6 {
        let svc = f.h.service.clone();
        tasks.push(tokio::spawn(async move { svc.bill_due().await }));
    }
    for t in tasks {
        let _ = t.await.unwrap();
    }
    assert_eq!(charges(&f).await, 2);
    assert_eq!(sub(&f, id).await["periods"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn failed_charge_retries_on_schedule_then_goes_past_due_then_cancels() {
    let f = fixture((2030, 1, 1)).await;
    let p = plan(&f, monthly(1500)).await;
    let id = subscribe(&f, p, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    f.clock.advance_days(31);
    *mock(&f.h, f.stripe).fail_create.lock().unwrap() = true;
    f.h.service.bill_due().await.unwrap();
    assert_eq!(charges(&f).await, 1);
    f.h.service.bill_due().await.unwrap();
    let before = charges(&f).await;
    for day in [1, 2, 2] {
        f.clock.advance_days(day);
        f.h.service.bill_due().await.unwrap();
    }
    assert_eq!(charges(&f).await, before);
    let s = sub(&f, id).await;
    assert_eq!(s["status"], "past_due", "{s}");
    f.clock.advance_days(8);
    f.h.service.bill_due().await.unwrap();
    let s = sub(&f, id).await;
    assert_eq!(s["status"], "cancelled");
    assert_eq!(s["cancel_reason"], "payment_failed");
    let ev = events(&f).await;
    assert_eq!(ev.iter().filter(|e| *e == "invoice.failed").count(), 4);
    assert!(ev.contains(&"subscription.past_due".to_string()));
    assert_eq!(ev.last().unwrap(), "subscription.cancelled");
}

#[tokio::test]
async fn a_recovered_retry_charges_once_and_never_twice() {
    let f = fixture((2030, 1, 1)).await;
    let p = plan(&f, monthly(1500)).await;
    let id = subscribe(&f, p, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    f.clock.advance_days(31);
    *mock(&f.h, f.stripe).fail_create.lock().unwrap() = true;
    f.h.service.bill_due().await.unwrap();
    assert_eq!(charges(&f).await, 1);
    *mock(&f.h, f.stripe).fail_create.lock().unwrap() = false;
    f.clock.advance_days(2);
    f.h.service.bill_due().await.unwrap();
    f.h.service.bill_due().await.unwrap();
    assert_eq!(charges(&f).await, 2);
    let s = sub(&f, id).await;
    assert_eq!(s["status"], "active");
    assert_eq!(s["periods"][0]["status"], "paid");
    assert_eq!(s["periods"][0]["failures"], 1);
}

#[tokio::test]
async fn trial_delays_the_first_charge() {
    let f = fixture((2030, 1, 1)).await;
    let mut body = monthly(1500);
    body["trial_days"] = json!(14);
    let p = plan(&f, body).await;
    let id = subscribe(&f, p, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    assert_eq!(sub(&f, id).await["status"], "trialing");
    assert_eq!(charges(&f).await, 0);
    f.clock.advance_days(13);
    f.h.service.bill_due().await.unwrap();
    assert_eq!(charges(&f).await, 0);
    f.clock.advance_days(2);
    f.h.service.bill_due().await.unwrap();
    assert_eq!(charges(&f).await, 1);
    assert_eq!(sub(&f, id).await["status"], "active");
}

#[tokio::test]
async fn cancel_at_period_end_keeps_access_then_stops() {
    let f = fixture((2030, 1, 1)).await;
    let p = plan(&f, monthly(1500)).await;
    let id = subscribe(&f, p, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    let (s, r) = act(&f, id, "cancel", json!({"at_period_end": true})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "active");
    assert_eq!(r["cancel_at_period_end"], true);
    f.clock.advance_days(32);
    f.h.service.bill_due().await.unwrap();
    let s = sub(&f, id).await;
    assert_eq!(s["status"], "cancelled");
    assert_eq!(charges(&f).await, 1);
    assert!(
        events(&f)
            .await
            .contains(&"subscription.cancelled".to_string())
    );
}

#[tokio::test]
async fn cancel_now_and_pause_resume() {
    let f = fixture((2030, 1, 1)).await;
    let p = plan(&f, monthly(1500)).await;
    let id = subscribe(&f, p, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    let (s, r) = act(&f, id, "pause", Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    f.clock.advance_days(40);
    f.h.service.bill_due().await.unwrap();
    assert_eq!(charges(&f).await, 1);
    let (_, r) = act(&f, id, "resume", Value::Null).await;
    assert_eq!(r["status"], "active");
    f.h.service.bill_due().await.unwrap();
    assert_eq!(charges(&f).await, 2);
    let (_, r) = act(&f, id, "cancel", Value::Null).await;
    assert_eq!(r["status"], "cancelled");
    let (s, _) = act(&f, id, "cancel", Value::Null).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = act(&f, id, "resume", Value::Null).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn upgrade_with_proration_charges_the_remaining_difference() {
    let f = fixture((2030, 1, 1)).await;
    let cheap = plan(&f, monthly(3100)).await;
    let dear = plan(&f, monthly(6200)).await;
    let id = subscribe(&f, cheap, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    f.clock.advance_days(10);
    let (s, r) = act(
        &f,
        id,
        "change-plan",
        json!({"plan_id": dear, "proration": "prorate"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["plan_id"], dear);
    let (_, found) = call(
        &f.h.app,
        "GET",
        "/v1/payments?status=paid&limit=100",
        Some(&f.t.key),
        Value::Null,
    )
    .await;
    let change = found
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["reference"].as_str().unwrap().ends_with("_change"))
        .expect("proration payment");
    assert_eq!(change["amount"], 3100 * 21 / 31);
    f.clock.advance_days(21);
    f.h.service.bill_due().await.unwrap();
    let (_, found) = call(
        &f.h.app,
        "GET",
        "/v1/payments?status=paid&limit=100",
        Some(&f.t.key),
        Value::Null,
    )
    .await;
    assert!(
        found
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["reference"] == "sub_1_1" && p["amount"] == 6200)
            || found
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["amount"] == 6200)
    );
}

#[tokio::test]
async fn downgrade_with_proration_becomes_credit_on_the_next_invoice() {
    let f = fixture((2030, 1, 1)).await;
    let dear = plan(&f, monthly(3100)).await;
    let cheap = plan(&f, monthly(1550)).await;
    let id = subscribe(&f, dear, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    f.clock.advance_days(15);
    let (_, r) = act(
        &f,
        id,
        "change-plan",
        json!({"plan_id": cheap, "proration": "prorate"}),
    )
    .await;
    let credit = r["credit"].as_i64().unwrap();
    assert_eq!(credit, 1550 * 16 / 31);
    f.clock.advance_days(17);
    f.h.service.bill_due().await.unwrap();
    let (_, found) = call(
        &f.h.app,
        "GET",
        "/v1/payments?status=paid&limit=100",
        Some(&f.t.key),
        Value::Null,
    )
    .await;
    assert!(
        found
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["amount"] == 1550 - credit)
    );
}

#[tokio::test]
async fn plan_change_without_proration_applies_next_period() {
    let f = fixture((2030, 1, 1)).await;
    let a = plan(&f, monthly(1000)).await;
    let b = plan(&f, monthly(2000)).await;
    let id = subscribe(&f, a, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    f.clock.advance_days(5);
    let (_, r) = act(&f, id, "change-plan", json!({"plan_id": b})).await;
    assert_eq!(r["credit"], 0);
    assert_eq!(charges(&f).await, 1);
    f.clock.advance_days(30);
    f.h.service.bill_due().await.unwrap();
    assert_eq!(sub(&f, id).await["periods"][0]["amount"], 2000);
}

#[tokio::test]
async fn first_payment_subscription_activates_when_the_card_is_saved() {
    let f = fixture((2030, 1, 1)).await;
    let p = plan(&f, monthly(1500)).await;
    let (s, r) = call(
        &f.h.app,
        "POST",
        "/v1/subscriptions",
        Some(&f.t.key),
        json!({"customer_ref": "cust-2", "plan_id": p}),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    assert_eq!(r["status"], "incomplete");
    let id = r["id"].as_u64().unwrap();
    let payment_id = r["payment_id"].as_str().unwrap().to_string();
    assert!(r["pay_url"].as_str().unwrap().contains(&payment_id));
    let (s, a) = attempt(
        &f.h,
        &f.t,
        &payment_id,
        json!({"method": "stripe", "save_method": true}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{a}");
    let aid = a["attempts"][0]["id"].as_u64().unwrap();
    let lookup = db::get_attempt(&f.h.pool, aid)
        .await
        .unwrap()
        .unwrap()
        .lookup;
    let hook = send(
        &f.h.app,
        "POST",
        &format!("/hook/{}/{}", f.t.id, f.stripe),
        None,
        &[("x-mock-secret", "mock-secret")],
        json!({"lookup": lookup, "saved": "pm_cust2"}),
    )
    .await;
    assert_eq!(hook.status, StatusCode::OK);
    let s = sub(&f, id).await;
    assert_eq!(s["status"], "active", "{s}");
    assert!(s["saved_method_id"].is_u64());
    f.clock.advance_days(32);
    f.h.service.bill_due().await.unwrap();
    assert_eq!(sub(&f, id).await["periods"].as_array().unwrap().len(), 2);
    let ev = events(&f).await;
    assert_eq!(ev[0], "subscription.created");
    assert!(ev.contains(&"invoice.paid".to_string()));
}

#[tokio::test]
async fn methods_that_cannot_be_charged_get_a_pay_link_event_and_a_grace_period() {
    let f = fixture((2030, 1, 1)).await;
    let mut body = monthly(1500);
    body["grace_days"] = json!(3);
    let p = plan(&f, body).await;
    let (_, r) = call(
        &f.h.app,
        "POST",
        "/v1/subscriptions",
        Some(&f.t.key),
        json!({"customer_ref": "cust-ln", "plan_id": p}),
    )
    .await;
    let id = r["id"].as_u64().unwrap();
    let first = r["payment_id"].as_str().unwrap().to_string();
    let ln = provider(&f.h, &f.t, "lightning", json!({}), None).await;
    let (s, a) = attempt(&f.h, &f.t, &first, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::OK, "{a}");
    let hash = a["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    use payment_gateway::providers::{EventSink, ProviderEvent};
    payment_gateway::service::ProviderSink {
        service: f.h.service.clone(),
        provider_id: ln,
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
    let s = sub(&f, id).await;
    assert_eq!(s["status"], "active", "{s}");
    assert!(s["saved_method_id"].is_null());
    f.clock.advance_days(32);
    f.h.service.bill_due().await.unwrap();
    let s = sub(&f, id).await;
    assert!(
        s["pay_url"]
            .as_str()
            .unwrap()
            .starts_with("https://pay.test/pay/")
    );
    let ev = events(&f).await;
    assert!(
        ev.contains(&"invoice.payment_required".to_string()),
        "{ev:?}"
    );
    f.clock.advance_days(1);
    f.h.service.bill_due().await.unwrap();
    assert_eq!(sub(&f, id).await["status"], "active");
    f.clock.advance_days(3);
    f.h.service.bill_due().await.unwrap();
    let ev = events(&f).await;
    assert!(ev.contains(&"invoice.failed".to_string()), "{ev:?}");
    assert_eq!(sub(&f, id).await["status"], "past_due");
}

#[tokio::test]
async fn tenants_cannot_see_or_change_each_others_plans_and_subscriptions() {
    let f = fixture((2030, 1, 1)).await;
    let p = plan(&f, monthly(1500)).await;
    let id = subscribe(&f, p, Some(f.saved)).await["id"]
        .as_u64()
        .unwrap();
    let other = tenant(&f.h).await;
    for (m, path) in [
        ("GET", format!("/v1/plans/{p}")),
        ("GET", format!("/v1/subscriptions/{id}")),
        ("POST", format!("/v1/subscriptions/{id}/cancel")),
        ("POST", format!("/v1/subscriptions/{id}/pause")),
        ("POST", format!("/v1/plans/{p}/archive")),
    ] {
        let (s, _) = call(&f.h.app, m, &path, Some(&other.key), Value::Null).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{m} {path}");
    }
    let (_, list) = call(
        &f.h.app,
        "GET",
        "/v1/subscriptions",
        Some(&other.key),
        Value::Null,
    )
    .await;
    assert!(list.as_array().unwrap().is_empty());
    let (_, list) = call(&f.h.app, "GET", "/v1/plans", Some(&other.key), Value::Null).await;
    assert!(list.as_array().unwrap().is_empty());
    let (s, _) = call(
        &f.h.app,
        "POST",
        "/v1/subscriptions",
        Some(&other.key),
        json!({"customer_ref": "cust-1", "plan_id": p}),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &f.h.app,
        "POST",
        "/v1/subscriptions",
        Some(&other.key),
        json!({"customer_ref": "cust-1", "plan_id": 1, "saved_method_id": f.saved}),
    )
    .await;
    assert_ne!(s, StatusCode::CREATED);
}

#[tokio::test]
async fn plan_and_subscription_validation() {
    let f = fixture((2030, 1, 1)).await;
    for body in [
        json!({"name": "", "amount": 100, "currency": "EUR"}),
        json!({"name": "x", "amount": 0, "currency": "EUR"}),
        json!({"name": "x", "amount": 100, "currency": "XXX"}),
        json!({"name": "x", "amount": 100, "currency": "EUR", "interval": "decade"}),
        json!({"name": "x", "amount": 100, "currency": "EUR", "interval_count": 0}),
    ] {
        let (s, _) = call(&f.h.app, "POST", "/v1/plans", Some(&f.t.key), body).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    let p = plan(&f, json!({"name": "min", "amount": 100, "currency": "EUR"})).await;
    let (_, r) = call(
        &f.h.app,
        "GET",
        &format!("/v1/plans/{p}"),
        Some(&f.t.key),
        Value::Null,
    )
    .await;
    assert_eq!(r["interval"], "month");
    assert_eq!(r["grace_days"], 3);
    call(
        &f.h.app,
        "POST",
        &format!("/v1/plans/{p}/archive"),
        Some(&f.t.key),
        Value::Null,
    )
    .await;
    let (s, _) = call(
        &f.h.app,
        "POST",
        "/v1/subscriptions",
        Some(&f.t.key),
        json!({"customer_ref": "cust-1", "plan_id": p, "saved_method_id": f.saved}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
