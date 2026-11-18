mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::db;
use serde_json::{Value, json};

struct Paid {
    id: String,
    lookup: String,
    uri: String,
}

async fn paid(h: &Harness, t: &TenantFixture, secret: &str) -> Paid {
    let st = provider(h, t, "stripe", json!({"secret": secret}), None).await;
    let p = new_payment(h, t, eur("o", 1000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, t, &id, json!({"method": "stripe"})).await;
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    let lookup = db::get_attempt(&h.pool, aid).await.unwrap().unwrap().lookup;
    let uri = format!("/hook/{}/{st}", t.id);
    let r = hook(h, &uri, secret, json!({"lookup": lookup, "amount": 1000})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    Paid { id, lookup, uri }
}

async fn hook(h: &Harness, uri: &str, secret: &str, body: Value) -> Reply {
    send(
        &h.app,
        "POST",
        uri,
        None,
        &[("x-mock-secret", secret)],
        body,
    )
    .await
}

#[tokio::test]
async fn dispute_flags_the_payment_and_emits_webhooks_once() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = paid(&h, &t, "s").await;
    let open = json!({"kind": "dispute_open", "lookup": p.lookup, "event_id": "evt_1"});
    assert_eq!(
        hook(&h, &p.uri, "s", open.clone()).await.status,
        StatusCode::OK
    );
    let again = hook(&h, &p.uri, "s", open).await;
    assert_eq!(again.json["duplicate"], true);
    let view = get_payment(&h, &t, &p.id).await;
    assert_eq!(view["status"], "paid");
    assert_eq!(view["disputed"], true);
    assert_eq!(view["dispute_status"], "open");
    let lost = json!({"kind": "dispute_lost", "lookup": p.lookup, "event_id": "evt_2"});
    hook(&h, &p.uri, "s", lost).await;
    let view = get_payment(&h, &t, &p.id).await;
    assert_eq!(view["disputed"], false);
    assert_eq!(view["dispute_status"], "lost");
    h.service.deliver_due().await.unwrap();
    let types = t.receiver.types();
    assert_eq!(types.iter().filter(|t| *t == "payment.disputed").count(), 1);
    assert_eq!(
        types
            .iter()
            .filter(|t| *t == "payment.dispute_closed")
            .count(),
        1
    );
}

#[tokio::test]
async fn provider_refund_is_recorded_once_per_refund_id() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = paid(&h, &t, "s").await;
    let ev = |event_id: &str| json!({"kind": "refunded", "lookup": p.lookup, "refund_id": "re_1", "amount": 400, "event_id": event_id});
    hook(&h, &p.uri, "s", ev("e1")).await;
    hook(&h, &p.uri, "s", ev("e2")).await;
    let view = get_payment(&h, &t, &p.id).await;
    assert_eq!(view["status"], "partially_refunded");
    assert_eq!(view["refunds"].as_array().map(Vec::len), Some(1));
    let full = json!({"kind": "refunded", "lookup": p.lookup, "refund_id": "re_2", "amount": 600});
    hook(&h, &p.uri, "s", full).await;
    assert_eq!(get_payment(&h, &t, &p.id).await["status"], "refunded");
}

#[tokio::test]
async fn oversized_hook_body_is_rejected() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = paid(&h, &t, "s").await;
    let big = json!({"lookup": p.lookup, "pad": "x".repeat(300_000)});
    let r = hook(&h, &p.uri, "s", big).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn consecutive_failures_auto_disable_a_customer_provider() {
    let h = harness_with_mail().await;
    let t = tenant(&h).await;
    db::set_tenant_trusted(&h.pool, t.id, false).await.unwrap();
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    let rec: &dyn payment_gateway::providers::HealthRecorder = h.service.as_ref();
    for _ in 0..9 {
        rec.provider_call(st, Err("boom".into())).await;
    }
    let row = db::get_provider_config(&h.pool, st).await.unwrap().unwrap();
    assert!(row.enabled);
    rec.provider_call(st, Err("boom".into())).await;
    let row = db::get_provider_config(&h.pool, st).await.unwrap().unwrap();
    assert!(!row.enabled);
    assert!(
        db::provider_auto_disabled_at(&h.pool, st)
            .await
            .unwrap()
            .is_some()
    );
    let health = db::get_provider_health(&h.pool, st).await.unwrap().unwrap();
    assert_eq!(health.errors, 10);
    assert_eq!(health.last_error.as_deref(), Some("boom"));
}

#[tokio::test]
async fn success_resets_the_failure_streak() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    let rec: &dyn payment_gateway::providers::HealthRecorder = h.service.as_ref();
    for _ in 0..9 {
        rec.provider_call(st, Err("boom".into())).await;
    }
    rec.provider_call(st, Ok(())).await;
    rec.provider_call(st, Err("boom".into())).await;
    let row = db::get_provider_config(&h.pool, st).await.unwrap().unwrap();
    assert!(row.enabled);
    let health = db::get_provider_health(&h.pool, st).await.unwrap().unwrap();
    assert_eq!(health.consecutive_failures, 1);
    assert_eq!(health.successes, 1);
}

#[tokio::test]
async fn trusted_project_provider_is_never_auto_disabled() {
    let h = harness_with_mail().await;
    let t = tenant(&h).await;
    db::set_tenant_trusted(&h.pool, t.id, true).await.unwrap();
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    let rec: &dyn payment_gateway::providers::HealthRecorder = h.service.as_ref();
    for _ in 0..15 {
        rec.provider_call(st, Err("boom".into())).await;
    }
    let row = db::get_provider_config(&h.pool, st).await.unwrap().unwrap();
    assert!(row.enabled);
}

#[test]
fn test_connection_errors_do_not_echo_secrets() {
    let cfg = json!({"client_secret": "supersecretvalue", "client_id": "abc"});
    let out = payment_gateway::validate::sanitize_test_error(
        "auth failed for supersecretvalue",
        Some(&cfg),
    );
    assert!(!out.contains("supersecretvalue"));
}

#[tokio::test]
async fn test_connection_endpoint_reports_without_leaking_and_is_tenant_scoped() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(
        &h,
        &t,
        "stripe",
        json!({"secret": "topsecret1", "fail_test": true}),
        None,
    )
    .await;
    let p = h.service.registry.get(st).await.unwrap();
    let err = p.test_connection().await.unwrap_err();
    let cfg = json!({"secret": "topsecret1"});
    let msg = payment_gateway::validate::sanitize_test_error(&format!("{err:#}"), Some(&cfg));
    assert!(!msg.contains("topsecret1"), "{msg}");
    let ok = provider(&h, &t, "lightning", json!({}), None).await;
    let good = h
        .service
        .registry
        .get(ok)
        .await
        .unwrap()
        .test_connection()
        .await
        .unwrap();
    assert_eq!(good, "Connected to mock");
}
