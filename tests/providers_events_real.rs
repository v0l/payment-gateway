use hmac::{Hmac, Mac};
use payment_gateway::db::{AttemptRow, SavedMethodRow};
use payment_gateway::providers::paypal::PayPalProvider;
use payment_gateway::providers::revolut::RevolutProvider;
use payment_gateway::providers::stripe::StripeProvider;
use payment_gateway::providers::{
    AttemptRequest, DisputeState, Provider, ProviderEvent, RefundRequest,
};
use serde_json::{Value, json};
use sha2::Sha256;
use std::collections::HashMap;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn request() -> AttemptRequest {
    AttemptRequest {
        payment_id: [7u8; 32],
        amount: 1045,
        currency: "EUR".into(),
        description: "Pro".into(),
        expires: chrono::Utc::now() + chrono::Duration::hours(1),
        success_url: Some("https://shop.example/ok".into()),
        cancel_url: Some("https://shop.example/no".into()),
        customer_ref: None,
        customer_email: None,
        option: None,
        save_method: false,
        receive: None,
        lines: vec![],
    }
}

fn attempt(lookup: &str) -> AttemptRow {
    AttemptRow {
        id: 1,
        payment_id: vec![7; 32],
        provider_config_id: 1,
        watch_provider_id: 1,
        method: "x".into(),
        option_key: String::new(),
        base_amount: 1000,
        processing_fee: 0,
        currency: "EUR".into(),
        charge_amount: 1000,
        charge_currency: "EUR".into(),
        rate: 1.0,
        lookup: lookup.into(),
        external_id: None,
        details: "{}".into(),
        status: "pending".into(),
        created: chrono::Utc::now(),
        expires: chrono::Utc::now(),
        paid_at: None,
        save_method: false,
        shared_provider_id: None,
        platform_fee_bps: None,
        platform_fee_base: None,
        platform_fee: None,
        platform_net: None,
    }
}

fn saved() -> SavedMethodRow {
    SavedMethodRow {
        id: 1,
        tenant_id: 1,
        customer_ref: "c".into(),
        provider_config_id: 1,
        external_customer_id: "cus_9".into(),
        external_id: "pm_9".into(),
        brand: None,
        last4: None,
        exp_month: None,
        exp_year: None,
        enabled: true,
        created: chrono::Utc::now(),
    }
}

fn stripe(url: &str) -> StripeProvider {
    StripeProvider::new(&json!({"secret_key": "sk_test", "url": url, "webhook_secret": "whsec_x"}))
        .unwrap()
}

fn stripe_headers(secret: &str, body: &str, ts: i64) -> HashMap<String, String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("{ts}.{body}").as_bytes());
    HashMap::from([(
        "stripe-signature".to_string(),
        format!("t={ts},v1={}", hex::encode(mac.finalize().into_bytes())),
    )])
}

fn stripe_event(kind: &str, object: Value) -> String {
    json!({"id": "evt_7", "type": kind, "data": {"object": object}}).to_string()
}

async fn stripe_lookup_mock(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/v1/checkout/sessions"))
        .and(query_param("payment_intent", "pi_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [{"id": "cs_1"}]})))
        .mount(server)
        .await;
}

#[tokio::test]
async fn stripe_charge_refunded_yields_refund_events_for_the_session() {
    let server = MockServer::start().await;
    stripe_lookup_mock(&server).await;
    Mock::given(method("GET"))
        .and(path("/v1/refunds"))
        .and(query_param("charge", "ch_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
            {"id": "re_1", "status": "succeeded", "amount": 400},
            {"id": "re_2", "status": "failed", "failure_reason": "expired_or_canceled_card"},
            {"id": "re_3", "status": "pending", "amount": 1}
        ]})))
        .mount(&server)
        .await;
    let body = stripe_event(
        "charge.refunded",
        json!({"id": "ch_1", "payment_intent": "pi_1"}),
    );
    let events = stripe(&server.uri())
        .handle_webhook(
            &stripe_headers("whsec_x", &body, chrono::Utc::now().timestamp()),
            body.as_bytes(),
        )
        .await
        .unwrap();
    assert_eq!(
        events,
        vec![
            ProviderEvent::Refunded {
                lookup: "cs_1".into(),
                external_id: "re_1".into(),
                amount: Some(400)
            },
            ProviderEvent::RefundFailed {
                lookup: "cs_1".into(),
                external_id: "re_2".into(),
                reason: "expired_or_canceled_card".into()
            },
        ]
    );
}

#[tokio::test]
async fn stripe_dispute_created_and_closed_map_to_dispute_events() {
    let server = MockServer::start().await;
    stripe_lookup_mock(&server).await;
    let p = stripe(&server.uri());
    let now = chrono::Utc::now().timestamp();
    let cases = [
        (
            "charge.dispute.created",
            "needs_response",
            DisputeState::Open,
        ),
        ("charge.dispute.closed", "won", DisputeState::Won),
        ("charge.dispute.closed", "lost", DisputeState::Lost),
    ];
    for (kind, status, state) in cases {
        let body = stripe_event(
            kind,
            json!({"id": "dp_1", "payment_intent": "pi_1", "status": status, "reason": "fraudulent"}),
        );
        let events = p
            .handle_webhook(&stripe_headers("whsec_x", &body, now), body.as_bytes())
            .await
            .unwrap();
        assert_eq!(
            events,
            vec![ProviderEvent::Dispute {
                lookup: "cs_1".into(),
                state,
                reason: Some("fraudulent".into())
            }]
        );
    }
}

#[tokio::test]
async fn stripe_webhook_outside_replay_window_is_rejected() {
    let p = stripe("http://127.0.0.1:1");
    let body = stripe_event("charge.dispute.created", json!({"payment_intent": "pi_1"}));
    let old = chrono::Utc::now().timestamp() - 3600;
    assert!(
        p.handle_webhook(&stripe_headers("whsec_x", &body, old), body.as_bytes())
            .await
            .is_err()
    );
    assert_eq!(p.event_id(body.as_bytes()).as_deref(), Some("evt_7"));
}

#[tokio::test]
async fn stripe_off_session_charge_needing_authentication_returns_a_pay_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "pi_8", "status": "requires_action",
            "next_action": {"redirect_to_url": {"url": "https://hooks.stripe.com/3d_secure/x"}}
        })))
        .mount(&server)
        .await;
    let c = stripe(&server.uri())
        .charge_saved(&saved(), request())
        .await
        .unwrap();
    assert!(!c.completed);
    assert_eq!(c.lookup, "pi_8");
    assert_eq!(
        c.details["checkout_url"],
        "https://hooks.stripe.com/3d_secure/x"
    );
}

#[tokio::test]
async fn stripe_test_connection_fetches_the_account_and_hides_the_key() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/account"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "acct_1"})))
        .mount(&server)
        .await;
    let msg = stripe(&server.uri()).test_connection().await.unwrap();
    assert!(msg.contains("acct_1"));
    assert!(!msg.contains("sk_test"));
}

fn revolut(url: &str) -> RevolutProvider {
    RevolutProvider::new(&json!({"url": url, "token": "tok", "api_version": "2024-09-01", "public_key": "pk", "webhook_secret": "whsec"})).unwrap()
}

fn revolut_headers(secret: &str, body: &str, ts_ms: i64) -> HashMap<String, String> {
    let ts = ts_ms.to_string();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("v1.{ts}.{body}").as_bytes());
    HashMap::from([
        (
            "revolut-signature".to_string(),
            format!("v1={}", hex::encode(mac.finalize().into_bytes())),
        ),
        ("revolut-request-timestamp".to_string(), ts),
    ])
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[tokio::test]
async fn revolut_declined_and_dispute_events_map_and_old_ones_are_rejected() {
    let p = revolut("http://127.0.0.1:1");
    let cases = [
        (
            "ORDER_PAYMENT_DECLINED",
            ProviderEvent::Failed {
                lookup: "ord_1".into(),
                reason: "payment declined".into(),
            },
        ),
        (
            "DISPUTE_ACTION_REQUIRED",
            ProviderEvent::Dispute {
                lookup: "ord_1".into(),
                state: DisputeState::Open,
                reason: None,
            },
        ),
        (
            "DISPUTE_WON",
            ProviderEvent::Dispute {
                lookup: "ord_1".into(),
                state: DisputeState::Won,
                reason: None,
            },
        ),
        (
            "DISPUTE_LOST",
            ProviderEvent::Dispute {
                lookup: "ord_1".into(),
                state: DisputeState::Lost,
                reason: None,
            },
        ),
    ];
    for (kind, expected) in cases {
        let body = json!({"event": kind, "order_id": "ord_1"}).to_string();
        let events = p
            .handle_webhook(&revolut_headers("whsec", &body, now_ms()), body.as_bytes())
            .await
            .unwrap();
        assert_eq!(events, vec![expected]);
    }
    let body = json!({"event": "DISPUTE_WON", "order_id": "ord_1"}).to_string();
    assert!(
        p.handle_webhook(
            &revolut_headers("whsec", &body, now_ms() - 3_600_000),
            body.as_bytes()
        )
        .await
        .is_err()
    );
    assert!(
        p.handle_webhook(&revolut_headers("bad", &body, now_ms()), body.as_bytes())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn revolut_refund_order_completion_and_failure_map_to_refund_events() {
    let server = MockServer::start().await;
    for (id, state) in [("rf_1", "completed"), ("rf_2", "failed")] {
        Mock::given(method("GET"))
            .and(path(format!("/api/orders/{id}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "id": id, "type": "refund", "state": state, "related_order_id": "ord_1", "amount": 300
            })))
            .mount(&server)
            .await;
    }
    let p = revolut(&server.uri());
    let done = json!({"event": "ORDER_COMPLETED", "order_id": "rf_1"}).to_string();
    assert_eq!(
        p.handle_webhook(&revolut_headers("whsec", &done, now_ms()), done.as_bytes())
            .await
            .unwrap(),
        vec![ProviderEvent::Refunded {
            lookup: "ord_1".into(),
            external_id: "rf_1".into(),
            amount: Some(300)
        }]
    );
    let failed = json!({"event": "ORDER_FAILED", "order_id": "rf_2"}).to_string();
    let events = p
        .handle_webhook(
            &revolut_headers("whsec", &failed, now_ms()),
            failed.as_bytes(),
        )
        .await
        .unwrap();
    assert!(matches!(events[0], ProviderEvent::RefundFailed { .. }));
}

#[tokio::test]
async fn revolut_lists_and_removes_saved_methods_and_tests_connection() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/1.0/customers/cus_1/payment-methods"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id": "pm_1", "method_details": {"brand": "visa", "last4": "4242", "expiry_month": 12, "expiry_year": 2030}}
        ])))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/api/1.0/customers/cus_9/payment-methods/pm_9"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/1.0/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    let p = revolut(&server.uri());
    let list = p.list_saved("cus_1").await.unwrap();
    assert_eq!(list[0].method_id, "pm_1");
    assert_eq!(list[0].last4.as_deref(), Some("4242"));
    p.remove_saved(&saved()).await.unwrap();
    let msg = p.test_connection().await.unwrap();
    assert!(!msg.contains("tok"));
}

fn paypal(url: &str) -> PayPalProvider {
    PayPalProvider::new(
        &json!({"client_id": "cid", "client_secret": "csecret", "url": url, "webhook_id": "wh_1"}),
    )
    .unwrap()
}

async fn paypal_token(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/v1/oauth2/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "at"})))
        .mount(server)
        .await;
}

fn paypal_headers(time: String) -> HashMap<String, String> {
    HashMap::from([
        ("paypal-transmission-time".to_string(), time),
        ("paypal-auth-algo".to_string(), "SHA256withRSA".to_string()),
        (
            "paypal-cert-url".to_string(),
            "https://api.paypal.com/cert".to_string(),
        ),
        ("paypal-transmission-id".to_string(), "tid".to_string()),
        ("paypal-transmission-sig".to_string(), "sig".to_string()),
    ])
}

#[tokio::test]
async fn paypal_order_returns_the_approval_link_and_decimal_amount() {
    let server = MockServer::start().await;
    paypal_token(&server).await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders"))
        .and(wiremock::matchers::body_string_contains(
            "\"value\":\"10.45\"",
        ))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "ORD1",
            "links": [{"rel": "payer-action", "href": "https://paypal.com/checkoutnow?token=ORD1"}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    let c = paypal(&server.uri()).create(request()).await.unwrap();
    assert_eq!(c.lookup, "ORD1");
    assert!(!c.completed);
    assert_eq!(
        c.details["checkout_url"],
        "https://paypal.com/checkoutnow?token=ORD1"
    );
}

#[tokio::test]
async fn paypal_return_capture_settles_with_the_captured_amount() {
    let server = MockServer::start().await;
    paypal_token(&server).await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders/ORD1/capture"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "ORD1", "status": "COMPLETED",
            "purchase_units": [{"payments": {"captures": [
                {"id": "CAP1", "status": "COMPLETED", "amount": {"value": "10.45", "currency_code": "EUR"}}
            ]}}]
        })))
        .mount(&server)
        .await;
    let ev = paypal(&server.uri())
        .capture_return(&attempt("ORD1"))
        .await
        .unwrap();
    assert_eq!(
        ev,
        Some(ProviderEvent::Paid {
            lookup: "ORD1".into(),
            reference: "CAP1".into(),
            amount: Some(1045),
            confirmed: true,
            saved: None
        })
    );
}

#[tokio::test]
async fn paypal_webhooks_verify_remotely_capture_on_approval_and_map_disputes() {
    let server = MockServer::start().await;
    paypal_token(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/notifications/verify-webhook-signature"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"verification_status": "SUCCESS"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/checkout/orders/ORD1/capture"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "ORD1", "status": "COMPLETED",
            "purchase_units": [{"payments": {"captures": [
                {"id": "CAP1", "status": "COMPLETED", "amount": {"value": "10.45", "currency_code": "EUR"}}
            ]}}]
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/v2/payments/captures/CAP1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "CAP1", "supplementary_data": {"related_ids": {"order_id": "ORD1"}}
        })))
        .mount(&server)
        .await;
    let p = paypal(&server.uri());
    let headers = paypal_headers(chrono::Utc::now().to_rfc3339());

    let approved =
        json!({"id": "WH-1", "event_type": "CHECKOUT.ORDER.APPROVED", "resource": {"id": "ORD1"}})
            .to_string();
    let events = p
        .handle_webhook(&headers, approved.as_bytes())
        .await
        .unwrap();
    assert!(matches!(
        events[0],
        ProviderEvent::Paid {
            amount: Some(1045),
            ..
        }
    ));
    assert_eq!(p.event_id(approved.as_bytes()).as_deref(), Some("WH-1"));

    let refunded = json!({"id": "WH-2", "event_type": "PAYMENT.CAPTURE.REFUNDED", "resource": {
        "id": "RF1", "amount": {"value": "4.00", "currency_code": "EUR"},
        "supplementary_data": {"related_ids": {"order_id": "ORD1"}}}})
    .to_string();
    assert_eq!(
        p.handle_webhook(&headers, refunded.as_bytes())
            .await
            .unwrap(),
        vec![ProviderEvent::Refunded {
            lookup: "ORD1".into(),
            external_id: "RF1".into(),
            amount: Some(400)
        }]
    );

    let dispute = |status: &str, outcome: &str| {
        json!({"id": "WH-3", "event_type": "CUSTOMER.DISPUTE.RESOLVED", "resource": {
            "status": status, "reason": "MERCHANDISE_OR_SERVICE_NOT_RECEIVED",
            "dispute_outcome": {"outcome_code": outcome},
            "disputed_transactions": [{"seller_transaction_id": "CAP1"}]}})
        .to_string()
    };
    let lost = dispute("RESOLVED", "RESOLVED_BUYER_FAVOUR");
    let events = p.handle_webhook(&headers, lost.as_bytes()).await.unwrap();
    assert!(matches!(
        &events[0],
        ProviderEvent::Dispute { state: DisputeState::Lost, lookup, .. } if lookup == "ORD1"
    ));
    let won = dispute("RESOLVED", "RESOLVED_SELLER_FAVOUR");
    let events = p.handle_webhook(&headers, won.as_bytes()).await.unwrap();
    assert!(matches!(
        &events[0],
        ProviderEvent::Dispute {
            state: DisputeState::Won,
            ..
        }
    ));
}

#[tokio::test]
async fn paypal_webhook_with_failed_verification_or_stale_time_is_rejected() {
    let server = MockServer::start().await;
    paypal_token(&server).await;
    Mock::given(method("POST"))
        .and(path("/v1/notifications/verify-webhook-signature"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"verification_status": "FAILURE"})),
        )
        .mount(&server)
        .await;
    let p = paypal(&server.uri());
    let body =
        json!({"id": "WH-1", "event_type": "CHECKOUT.ORDER.APPROVED", "resource": {"id": "ORD1"}})
            .to_string();
    assert!(
        p.handle_webhook(
            &paypal_headers(chrono::Utc::now().to_rfc3339()),
            body.as_bytes()
        )
        .await
        .is_err()
    );
    let stale = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
    assert!(
        p.handle_webhook(&paypal_headers(stale), body.as_bytes())
            .await
            .is_err()
    );
    assert!(
        p.handle_webhook(&HashMap::new(), body.as_bytes())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn paypal_refund_hits_the_capture_and_test_connection_hides_secrets() {
    let server = MockServer::start().await;
    paypal_token(&server).await;
    Mock::given(method("GET"))
        .and(path("/v2/checkout/orders/ORD1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "ORD1", "status": "COMPLETED",
            "purchase_units": [{"payments": {"captures": [{"id": "CAP1", "status": "COMPLETED", "amount": {"value": "10.45", "currency_code": "EUR"}}]}}]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v2/payments/captures/CAP1/refund"))
        .and(wiremock::matchers::body_string_contains(
            "\"value\":\"4.00\"",
        ))
        .respond_with(
            ResponseTemplate::new(201).set_body_json(json!({"id": "RF9", "status": "COMPLETED"})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let p = paypal(&server.uri());
    let out = p
        .refund(
            &attempt("ORD1"),
            RefundRequest {
                amount: 400,
                currency: "EUR".into(),
                destination: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(out.external_id.as_deref(), Some("RF9"));
    let msg = p.test_connection().await.unwrap();
    assert!(!msg.contains("csecret") && !msg.contains("cid"));

    let bad = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/oauth2/token"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({"error_description": "Client Authentication failed"})),
        )
        .mount(&bad)
        .await;
    let err = format!(
        "{:#}",
        paypal(&bad.uri()).test_connection().await.unwrap_err()
    );
    assert!(!err.contains("csecret"));
}
