mod common;
use axum::http::StatusCode;
use common::*;
use hmac::{Hmac, Mac};
use payment_gateway::db;
use serde_json::{Value, json};
use sha2::Sha256;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap()
}

async fn real_provider(h: &Harness, t: &TenantFixture, kind: &str, config: Value) -> u64 {
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers", t.id),
        Some(ADMIN),
        json!({"method": kind, "provider_type": kind, "name": unique(kind), "config": config}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    r["id"].as_u64().unwrap()
}

async fn pending_attempt(h: &Harness, t: &TenantFixture, kind: &str) -> (String, String) {
    let p = new_payment(h, t, eur("o", 1000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, r) = attempt(h, t, &id, json!({"method": kind})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    let lookup = db::get_attempt(&h.pool, aid).await.unwrap().unwrap().lookup;
    (id, lookup)
}

async fn mark_paid(h: &Harness, provider: u64, lookup: &str) {
    use payment_gateway::providers::{EventSink, ProviderEvent};
    payment_gateway::service::ProviderSink {
        service: h.service.clone(),
        provider_id: provider,
    }
    .event(
        ProviderEvent::Paid {
            lookup: lookup.into(),
            reference: lookup.into(),
            amount: None,
            confirmed: true,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();
}

async fn post_hook(
    h: &Harness,
    t: &TenantFixture,
    provider: u64,
    headers: &[(&str, &str)],
    body: &str,
) -> Reply {
    let mut all = headers.to_vec();
    all.push(("content-type", "application/json"));
    let mut req = axum::http::Request::builder()
        .method("POST")
        .uri(format!("/hook/{}/{provider}", t.id));
    for (k, v) in &all {
        req = req.header(*k, *v);
    }
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    let res = h
        .app
        .clone()
        .oneshot(req.body(axum::body::Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        headers,
        text: String::from_utf8_lossy(&bytes).to_string(),
    }
}

async fn assert_disputed_once(h: &Harness, t: &TenantFixture, id: &str) {
    let view = get_payment(h, t, id).await;
    assert_eq!(view["dispute_status"], "open");
    assert_eq!(view["disputed"], true);
    assert_eq!(view["status"], "paid");
    h.service.deliver_due().await.unwrap();
    let n = t
        .receiver
        .types()
        .iter()
        .filter(|e| *e == "payment.disputed")
        .count();
    assert_eq!(n, 1, "{:?}", t.receiver.types());
}

fn stripe_signature(secret: &str, body: &str) -> String {
    let ts = chrono::Utc::now().timestamp();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("{ts}.{body}").as_bytes());
    format!("t={ts},v1={}", hex::encode(mac.finalize().into_bytes()))
}

#[tokio::test]
async fn stripe_dispute_events_reach_the_payment_through_the_real_adapter() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"id": "cs_d1", "url": "https://checkout.stripe.com/c/pay/cs_d1"}),
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v1/checkout/sessions"))
        .and(query_param("payment_intent", "pi_d1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [{"id": "cs_d1"}]})))
        .mount(&server)
        .await;
    let h = harness().await;
    let t = tenant(&h).await;
    let st = real_provider(
        &h,
        &t,
        "stripe",
        json!({"secret_key": "sk_test", "url": server.uri(), "webhook_secret": "whsec_d"}),
    )
    .await;
    let (id, lookup) = pending_attempt(&h, &t, "stripe").await;
    assert_eq!(lookup, "cs_d1");
    mark_paid(&h, st, &lookup).await;

    let created = fixture("stripe/dispute_created_event.json").replace("__INTENT__", "pi_d1");
    let sig = stripe_signature("whsec_d", &created);
    let bad = post_hook(&h, &t, st, &[("stripe-signature", "t=1,v1=00")], &created).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert!(get_payment(&h, &t, &id).await["dispute_status"].is_null());
    for _ in 0..2 {
        let r = post_hook(&h, &t, st, &[("stripe-signature", &sig)], &created).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    }
    assert_disputed_once(&h, &t, &id).await;

    let closed = fixture("stripe/dispute_closed_event.json").replace("__INTENT__", "pi_d1");
    let sig = stripe_signature("whsec_d", &closed);
    post_hook(&h, &t, st, &[("stripe-signature", &sig)], &closed).await;
    assert_eq!(get_payment(&h, &t, &id).await["dispute_status"], "lost");
    h.service.deliver_due().await.unwrap();
    assert!(
        t.receiver
            .types()
            .contains(&"payment.dispute_closed".to_string())
    );
}

fn revolut_headers(secret: &str, body: &str) -> (String, String) {
    let ts = chrono::Utc::now().timestamp_millis().to_string();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("v1.{ts}.{body}").as_bytes());
    (
        format!("v1={}", hex::encode(mac.finalize().into_bytes())),
        ts,
    )
}

#[tokio::test]
async fn revolut_dispute_event_reaches_the_payment_through_the_real_adapter() {
    let server = MockServer::start().await;
    let created: Value = serde_json::from_str(&fixture("revolut/order_created.json")).unwrap();
    let order_id = created["id"].as_str().unwrap().to_string();
    Mock::given(method("POST"))
        .and(path("/api/orders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(created))
        .mount(&server)
        .await;
    let h = harness().await;
    let t = tenant(&h).await;
    let rv = real_provider(
        &h,
        &t,
        "revolut",
        json!({"url": server.uri(), "token": "tok", "api_version": "2024-09-01", "webhook_secret": "wsk_d"}),
    )
    .await;
    let (id, lookup) = pending_attempt(&h, &t, "revolut").await;
    assert_eq!(lookup, order_id);
    mark_paid(&h, rv, &lookup).await;

    let body =
        fixture("revolut/webhook_dispute_action_required.json").replace("__ORDER__", &order_id);
    let (sig, ts) = revolut_headers("wsk_d", &body);
    let bad = post_hook(
        &h,
        &t,
        rv,
        &[
            ("revolut-signature", "v1=00"),
            ("revolut-request-timestamp", &ts),
        ],
        &body,
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    for _ in 0..2 {
        let r = post_hook(
            &h,
            &t,
            rv,
            &[
                ("revolut-signature", &sig),
                ("revolut-request-timestamp", &ts),
            ],
            &body,
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    }
    assert_disputed_once(&h, &t, &id).await;
}

#[tokio::test]
async fn paypal_dispute_event_reaches_the_payment_through_the_real_adapter() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "at"})))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "ORDD1",
            "links": [{"rel": "payer-action", "href": "https://paypal.com/checkoutnow?token=ORDD1"}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/captures/CAPD1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "CAPD1", "supplementary_data": {"related_ids": {"order_id": "ORDD1"}}
        })))
        .mount(&server)
        .await;
    let verify = Mock::given(method("POST"))
        .and(path("/v1/notifications/verify-webhook-signature"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"verification_status": "SUCCESS"})),
        );
    verify.mount(&server).await;
    let h = harness().await;
    let t = tenant(&h).await;
    let pp = real_provider(
        &h,
        &t,
        "paypal",
        json!({"client_id": "cid", "client_secret": "csecret", "url": server.uri(), "webhook_id": "wh_d"}),
    )
    .await;
    let (id, lookup) = pending_attempt(&h, &t, "paypal").await;
    assert_eq!(lookup, "ORDD1");
    mark_paid(&h, pp, &lookup).await;

    let body = fixture("paypal/webhook_dispute_created.json").replace("__CAPTURE__", "CAPD1");
    let time = chrono::Utc::now().to_rfc3339();
    let headers = [
        ("paypal-transmission-time", time.as_str()),
        ("paypal-auth-algo", "SHA256withRSA"),
        ("paypal-cert-url", "https://api.paypal.com/cert"),
        ("paypal-transmission-id", "tid"),
        ("paypal-transmission-sig", "sig"),
    ];
    let unsigned = post_hook(&h, &t, pp, &[], &body).await;
    assert_eq!(unsigned.status, StatusCode::BAD_REQUEST);
    for _ in 0..2 {
        let r = post_hook(&h, &t, pp, &headers, &body).await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    }
    assert_disputed_once(&h, &t, &id).await;
}
