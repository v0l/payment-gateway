use hmac::{Hmac, Mac};
use payment_gateway::db::{AttemptRow, SavedMethodRow};
use payment_gateway::providers::revolut::RevolutProvider;
use payment_gateway::providers::stripe::StripeProvider;
use payment_gateway::providers::{AttemptRequest, Provider, ProviderEvent, RefundRequest};
use serde_json::{Value, json};
use sha2::Sha256;
use std::collections::HashMap;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap()
}

fn fixture_json(name: &str) -> Value {
    serde_json::from_str(&fixture(name)).unwrap()
}

fn request(amount: u64) -> AttemptRequest {
    AttemptRequest {
        payment_id: [7u8; 32],
        amount,
        currency: "EUR".into(),
        description: "Pro plan".into(),
        expires: chrono::Utc::now() + chrono::Duration::hours(1),
        success_url: Some("https://shop.example/ok".into()),
        cancel_url: Some("https://shop.example/no".into()),
        customer_ref: Some("cust".into()),
        customer_email: Some("a@b.c".into()),
        option: None,
        save_method: false,
        receive: None,
    }
}

fn attempt_row(lookup: &str) -> AttemptRow {
    AttemptRow {
        id: 1,
        payment_id: vec![7; 32],
        provider_config_id: 1,
        watch_provider_id: 1,
        method: "x".into(),
        option_key: String::new(),
        base_amount: 2500,
        processing_fee: 0,
        currency: "EUR".into(),
        charge_amount: 2500,
        charge_currency: "EUR".into(),
        rate: 1.0,
        lookup: lookup.into(),
        external_id: None,
        details: "{}".into(),
        status: "paid".into(),
        created: chrono::Utc::now(),
        expires: chrono::Utc::now(),
        paid_at: None,
        save_method: false,
    }
}

fn saved(customer: &str, method: &str) -> SavedMethodRow {
    SavedMethodRow {
        id: 1,
        tenant_id: 1,
        customer_ref: "c".into(),
        provider_config_id: 1,
        external_customer_id: customer.into(),
        external_id: method.into(),
        brand: None,
        last4: None,
        exp_month: None,
        exp_year: None,
        enabled: true,
        created: chrono::Utc::now(),
    }
}

fn refund_request(amount: u64) -> RefundRequest {
    RefundRequest {
        amount,
        currency: "EUR".into(),
        destination: None,
    }
}

fn stripe_sig(secret: &str, body: &str) -> HashMap<String, String> {
    let ts = chrono::Utc::now().timestamp();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("{ts}.{body}").as_bytes());
    HashMap::from([(
        "stripe-signature".to_string(),
        format!("t={ts},v1={}", hex::encode(mac.finalize().into_bytes())),
    )])
}

fn revolut_sig(secret: &str, body: &str) -> HashMap<String, String> {
    let ts = chrono::Utc::now().timestamp_millis().to_string();
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

fn stripe(url: &str, key: &str) -> StripeProvider {
    StripeProvider::new(&json!({"secret_key": key, "url": url, "webhook_secret": "whsec_fixture"}))
        .unwrap()
}

fn revolut(url: &str, token: &str) -> RevolutProvider {
    RevolutProvider::new(&json!({
        "url": url, "token": token, "api_version": "2024-09-01",
        "public_key": "pk", "webhook_secret": "wsk_fixture"
    }))
    .unwrap()
}

#[tokio::test]
async fn stripe_recorded_responses_drive_checkout_webhook_refund_and_off_session_charge() {
    let server = MockServer::start().await;
    let session = fixture_json("stripe/checkout_session.json");
    let session_id = session["id"].as_str().unwrap().to_string();
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(session))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/v1/checkout/sessions/{session_id}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fixture_json("stripe/session_retrieve.json")),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/refunds"))
        .and(body_string_contains(
            "payment_intent=pi_3Qtest1234567890abcd",
        ))
        .and(body_string_contains("amount=1000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture_json("stripe/refund.json")))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .and(body_string_contains("off_session=true"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(fixture_json("stripe/off_session_intent.json")),
        )
        .mount(&server)
        .await;
    let p = stripe(&server.uri(), "sk_test_fixture");

    let created = p.create(request(2500)).await.unwrap();
    assert_eq!(created.lookup, session_id);
    assert!(
        created.details["checkout_url"]
            .as_str()
            .unwrap()
            .starts_with("https://checkout.stripe.com/")
    );

    let body = fixture("stripe/session_completed_event.json");
    let events = p
        .handle_webhook(&stripe_sig("whsec_fixture", &body), body.as_bytes())
        .await
        .unwrap();
    assert_eq!(
        events,
        vec![ProviderEvent::Paid {
            lookup: session_id.clone(),
            reference: session_id.clone(),
            amount: Some(2500),
            confirmed: true,
            saved: None
        }]
    );
    assert!(
        p.handle_webhook(&stripe_sig("whsec_other", &body), body.as_bytes())
            .await
            .is_err()
    );

    let refund = p
        .refund(&attempt_row(&session_id), refund_request(1000))
        .await
        .unwrap();
    assert_eq!(
        refund.external_id.as_deref(),
        Some("re_3Qtest1234567890abcd")
    );

    let charge = p
        .charge_saved(
            &saved("cus_Rtest123456", "pm_1Qtest1234567890"),
            request(700),
        )
        .await
        .unwrap();
    assert!(charge.completed);
    assert_eq!(charge.lookup, "pi_3Qoff1234567890abcd");
}

#[tokio::test]
async fn stripe_declined_off_session_charge_is_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .respond_with(
            ResponseTemplate::new(402).set_body_json(fixture_json("stripe/card_declined.json")),
        )
        .mount(&server)
        .await;
    let p = stripe(&server.uri(), "sk_test_fixture");
    let err = p
        .charge_saved(&saved("cus_x", "pm_x"), request(700))
        .await
        .unwrap_err();
    assert!(format!("{err:#}").contains("declined"));
}

#[tokio::test]
async fn revolut_recorded_responses_drive_order_webhook_and_refund() {
    let server = MockServer::start().await;
    let created = fixture_json("revolut/order_created.json");
    let order_id = created["id"].as_str().unwrap().to_string();
    Mock::given(method("POST"))
        .and(path("/api/orders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(created))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/orders/{order_id}")))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fixture_json("revolut/order_completed.json")),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/api/orders/{order_id}/refund")))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture_json("revolut/refund.json")))
        .mount(&server)
        .await;
    let p = revolut(&server.uri(), "tok_fixture");

    let attempt = p.create(request(2500)).await.unwrap();
    assert_eq!(attempt.lookup, order_id);
    assert!(
        attempt.details["checkout_url"]
            .as_str()
            .unwrap()
            .contains("revolut.com")
    );

    let body = fixture("revolut/webhook_order_completed.json");
    let events = p
        .handle_webhook(&revolut_sig("wsk_fixture", &body), body.as_bytes())
        .await
        .unwrap();
    assert_eq!(
        events,
        vec![ProviderEvent::Paid {
            lookup: order_id.clone(),
            reference: order_id.clone(),
            amount: Some(2500),
            confirmed: true,
            saved: None
        }]
    );
    assert!(
        p.handle_webhook(&revolut_sig("wrong", &body), body.as_bytes())
            .await
            .is_err()
    );

    let refund = p
        .refund(&attempt_row(&order_id), refund_request(1000))
        .await
        .unwrap();
    assert_eq!(
        refund.external_id.as_deref(),
        Some("6516e7aa-0b21-a0c1-b7a2-0a1b2c3d4e5f")
    );
}

async fn stripe_form(key: &str, path: &str, form: &[(&str, &str)]) -> Value {
    reqwest::Client::new()
        .post(format!("https://api.stripe.com{path}"))
        .bearer_auth(key)
        .form(form)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

#[tokio::test]
async fn stripe_test_mode_live_suite() {
    let Ok(key) = std::env::var("STRIPE_TEST_KEY") else {
        eprintln!("skipped: STRIPE_TEST_KEY not set");
        return;
    };
    assert!(key.starts_with("sk_test_"), "refusing a non test key");
    let p = StripeProvider::new(&json!({"secret_key": key})).unwrap();

    let session = p.create(request(2500)).await.unwrap();
    assert!(session.lookup.starts_with("cs_test_"));
    assert!(
        session.details["checkout_url"]
            .as_str()
            .unwrap()
            .starts_with("https://checkout.stripe.com/")
    );

    let customer = stripe_form(&key, "/v1/customers", &[("email", "e2e@example.com")]).await;
    let customer_id = customer["id"].as_str().unwrap();
    let pm = stripe_form(
        &key,
        "/v1/payment_methods/pm_card_visa/attach",
        &[("customer", customer_id)],
    )
    .await;
    let pm_id = pm["id"].as_str().unwrap();

    let charge = p
        .charge_saved(&saved(customer_id, pm_id), request(700))
        .await
        .unwrap();
    assert!(charge.completed);
    assert!(charge.lookup.starts_with("pi_"));

    let refund = p
        .refund(&attempt_row(&charge.lookup), refund_request(300))
        .await
        .unwrap();
    assert!(refund.external_id.unwrap().starts_with("re_"));
}

#[tokio::test]
async fn revolut_sandbox_live_suite() {
    let Ok(token) = std::env::var("REVOLUT_SANDBOX_TOKEN") else {
        eprintln!("skipped: REVOLUT_SANDBOX_TOKEN not set");
        return;
    };
    let p = revolut("https://sandbox-merchant.revolut.com", &token);
    let attempt = p.create(request(2500)).await.unwrap();
    assert!(
        attempt.details["checkout_url"]
            .as_str()
            .unwrap()
            .starts_with("https://")
    );
    p.cancel(&attempt_row(&attempt.lookup)).await.unwrap();
}
