use hmac::{Hmac, Mac};
use payment_gateway::db::{AttemptRow, SavedMethodRow};
use payment_gateway::providers::revolut::RevolutProvider;
use payment_gateway::providers::stripe::StripeProvider;
use payment_gateway::providers::{AttemptRequest, Provider, ProviderEvent, RefundRequest};
use serde_json::{Value, json};
use sha2::Sha256;
use std::collections::HashMap;
use wiremock::matchers::{body_partial_json, body_string_contains, method, path};
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
        lines: vec![],
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
        shared_provider_id: None,
        platform_fee_bps: None,
        platform_fee_base: None,
        platform_fee: None,
        platform_net: None,
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
async fn revolut_save_method_attempt_exposes_widget_details_and_plain_attempt_does_not() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/orders"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fixture_json("revolut/order_created.json")),
        )
        .mount(&server)
        .await;
    let p = revolut(&server.uri(), "tok_fixture");

    let plain = p.create(request(2500)).await.unwrap();
    assert!(plain.details.get("widget").is_none());

    let mut req = request(2500);
    req.save_method = true;
    let saved = p.create(req).await.unwrap();
    let widget = &saved.details["widget"];
    assert_eq!(widget["provider"], "revolut");
    assert_eq!(widget["token"], "eb78ecc2-d65f-4ace-acbd-e55253e7ea0f");
    assert_eq!(widget["email"], "a@b.c");
    assert!(saved.details["checkout_url"].as_str().is_some());
}

#[tokio::test]
async fn revolut_recorded_responses_drive_order_webhook_and_refund() {
    let server = MockServer::start().await;
    let created = fixture_json("revolut/order_created.json");
    let created_id = created["id"].as_str().unwrap().to_string();
    let completed = fixture_json("revolut/order_completed.json");
    let order_id = completed["id"].as_str().unwrap().to_string();
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
        .and(body_partial_json(
            json!({"amount": 1000, "currency": "EUR"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture_json("revolut/refund.json")))
        .mount(&server)
        .await;
    let p = revolut(&server.uri(), "tok_fixture");

    let attempt = p.create(request(2500)).await.unwrap();
    assert_eq!(attempt.lookup, created_id);
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
        Some("6afd9ba3-edb0-aa37-82b4-d1fe5f922128")
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

const SANDBOX_CARD_ENVIRONMENT: &str = r#"{"type":"browser","time_zone_utc_offset":0,"color_depth":24,"screen_width":1920,"screen_height":1080,"java_enabled":false,"challenge_window_width":400,"browser_url":"https://example.com"}"#;

async fn revolut_call(
    token: &str,
    verb: reqwest::Method,
    path: &str,
    body: Option<Value>,
) -> Value {
    let mut req = reqwest::Client::builder()
        .user_agent("payment-gateway-tests")
        .build()
        .unwrap()
        .request(verb, format!("https://sandbox-merchant.revolut.com{path}"))
        .bearer_auth(token)
        .header("Revolut-Api-Version", "2024-09-01");
    if let Some(body) = body {
        req = req.json(&body);
    }
    let rsp = req.send().await.unwrap();
    let status = rsp.status();
    let text = rsp.text().await.unwrap();
    assert!(
        status.is_success() || status.as_u16() == 204,
        "{path} {status}: {text}"
    );
    serde_json::from_str(&text).unwrap_or(Value::Null)
}

async fn revolut_pay_with_test_card(token: &str, order_id: &str, save: bool) -> bool {
    let environment: Value = serde_json::from_str(SANDBOX_CARD_ENVIRONMENT).unwrap();
    let mut body = json!({"payment_method": {
        "type": "card", "number": "4929420573595709", "expiry_month": 12,
        "expiry_year": 2030, "cvv": "123", "cardholder_name": "Test Person",
        "environment": environment
    }});
    if save {
        body["save_payment_method_for"] = json!("merchant");
    }
    revolut_call(
        token,
        reqwest::Method::POST,
        &format!("/api/orders/{order_id}/payments"),
        Some(body),
    )
    .await;
    for _ in 0..12 {
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let order = revolut_call(
            token,
            reqwest::Method::GET,
            &format!("/api/orders/{order_id}"),
            None,
        )
        .await;
        if order["state"] == "completed" && order["payments"][0]["state"] == "captured" {
            return true;
        }
    }
    false
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
    eprintln!("order created {}", attempt.lookup);
    p.cancel(&attempt_row(&attempt.lookup)).await.unwrap();

    let mut paid_order = None;
    for _ in 0..4 {
        let order = p.create(request(2500)).await.unwrap();
        if revolut_pay_with_test_card(&token, &order.lookup, false).await {
            paid_order = Some(order.lookup);
            break;
        }
    }
    let paid_order = paid_order.expect("sandbox never completed a card payment");
    eprintln!("order paid {paid_order}");

    let mut refund = None;
    for _ in 0..10 {
        match p
            .refund(&attempt_row(&paid_order), refund_request(400))
            .await
        {
            Ok(r) => {
                refund = Some(r);
                break;
            }
            Err(e) => {
                eprintln!("refund not yet possible: {e:#}");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        }
    }
    let refund = refund.expect("sandbox never accepted the refund");
    eprintln!("refund {:?}", refund.external_id);
    assert!(refund.external_id.is_some());
    let refunded = revolut_call(
        &token,
        reqwest::Method::GET,
        &format!("/api/orders/{paid_order}"),
        None,
    )
    .await;
    assert_eq!(refunded["state"], "completed");
}

#[tokio::test]
async fn revolut_recorded_decline_webhook_maps_to_a_failed_attempt() {
    let server = MockServer::start().await;
    let declined = fixture_json("revolut/order_payment_declined.json");
    let order_id = declined["id"].as_str().unwrap().to_string();
    assert_eq!(declined["payments"][0]["state"], "declined");
    assert_eq!(
        declined["payments"][0]["decline_reason"],
        "insufficient_funds"
    );
    let p = revolut(&server.uri(), "tok_fixture");
    let body = fixture("revolut/webhook_order_payment_declined.json");
    let events = p
        .handle_webhook(&revolut_sig("wsk_fixture", &body), body.as_bytes())
        .await
        .unwrap();
    assert_eq!(
        events,
        vec![ProviderEvent::Failed {
            lookup: order_id,
            reason: "payment declined".into()
        }]
    );
}

#[tokio::test]
async fn revolut_recorded_saved_methods_are_listed_with_card_details() {
    let server = MockServer::start().await;
    let list = fixture_json("revolut/payment_methods.json");
    Mock::given(method("GET"))
        .and(path("/api/1.0/customers/cus_fixture/payment-methods"))
        .respond_with(ResponseTemplate::new(200).set_body_json(list))
        .mount(&server)
        .await;
    let methods = revolut(&server.uri(), "tok_fixture")
        .list_saved("cus_fixture")
        .await
        .unwrap();
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0].last4.as_deref(), Some("5709"));
    assert_eq!(methods[0].brand.as_deref(), Some("VISA"));
    assert_eq!(methods[0].exp_year, Some(2030));
}

#[tokio::test]
async fn revolut_sandbox_live_test_connection_saved_methods_and_removal() {
    let Ok(token) = std::env::var("REVOLUT_SANDBOX_TOKEN") else {
        eprintln!("skipped: REVOLUT_SANDBOX_TOKEN not set");
        return;
    };
    let p = revolut("https://sandbox-merchant.revolut.com", &token);
    let msg = p.test_connection().await.unwrap();
    eprintln!("test connection: {msg}");
    assert!(!msg.contains(&token));

    let customers = revolut_call(&token, reqwest::Method::GET, "/api/1.0/customers", None).await;
    let mut listed = false;
    for c in customers.as_array().unwrap() {
        let id = c["id"].as_str().unwrap();
        let methods = p.list_saved(id).await.unwrap();
        if let Some(m) = methods.first() {
            eprintln!(
                "saved list ok: {} method(s), last4 {:?}",
                methods.len(),
                m.last4
            );
            assert!(!m.method_id.is_empty());
            listed = true;
            break;
        }
    }
    assert!(listed, "no sandbox customer holds a saved method");

    let ghost = SavedMethodRow {
        id: 1,
        tenant_id: 1,
        customer_ref: "c".into(),
        provider_config_id: 1,
        external_customer_id: "00000000-0000-0000-0000-000000000000".into(),
        external_id: "00000000-0000-0000-0000-000000000000".into(),
        brand: None,
        last4: None,
        exp_month: None,
        exp_year: None,
        enabled: true,
        created: chrono::Utc::now(),
    };
    p.remove_saved(&ghost).await.unwrap();

    let bad = revolut("https://sandbox-merchant.revolut.com", "not-a-token");
    let err = format!("{:#}", bad.test_connection().await.unwrap_err());
    assert!(!err.contains("not-a-token"), "{err}");
}

#[tokio::test]
async fn revolut_recorded_widget_order_settles_with_the_saved_card() {
    let server = MockServer::start().await;
    let order = fixture_json("revolut/order_completed_saved_card.json");
    let order_id = order["id"].as_str().unwrap().to_string();
    let customer_id = order["customer"]["id"].as_str().unwrap().to_string();
    assert!(order["customer"]["email"].as_str().is_some());
    Mock::given(method("GET"))
        .and(path(format!("/api/orders/{order_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(order))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!(
            "/api/1.0/customers/{customer_id}/payment-methods"
        )))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fixture_json("revolut/payment_methods.json")),
        )
        .mount(&server)
        .await;
    let body = format!(r#"{{"event":"ORDER_COMPLETED","order_id":"{order_id}"}}"#);
    let events = revolut(&server.uri(), "tok_fixture")
        .handle_webhook(&revolut_sig("wsk_fixture", &body), body.as_bytes())
        .await
        .unwrap();
    let ProviderEvent::Paid { saved, .. } = &events[0] else {
        panic!("{events:?}");
    };
    let saved = saved.as_ref().expect("saved card");
    assert_eq!(saved.customer_id, customer_id);
    assert_eq!(saved.last4.as_deref(), Some("5709"));
}

#[test]
fn revolut_recorded_customer_methods_list_the_card_saved_for_merchant() {
    let list = fixture_json("revolut/customer_payment_methods_saved_for_merchant.json");
    let card = &list["payment_methods"][0];
    assert_eq!(card["saved_for"], "merchant");
    assert_eq!(card["last_four"], "5709");
}

#[tokio::test]
async fn revolut_off_session_charge_waits_for_a_processing_order_to_complete() {
    let server = MockServer::start().await;
    let completed = fixture_json("revolut/order_off_session_completed.json");
    let order_id = completed["id"].as_str().unwrap().to_string();
    let mut processing = completed.clone();
    processing["state"] = json!("processing");
    Mock::given(method("POST"))
        .and(path("/api/orders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(processing.clone()))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("/api/orders/{order_id}/payments")))
        .respond_with(ResponseTemplate::new(200).set_body_json(completed["payments"][0].clone()))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/orders/{order_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(processing))
        .up_to_n_times(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/api/orders/{order_id}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(completed))
        .mount(&server)
        .await;
    let charge = revolut(&server.uri(), "tok_fixture")
        .charge_saved(&saved("cus_live", "pm_live"), request(700))
        .await
        .unwrap();
    assert!(charge.completed);
    assert_eq!(charge.lookup, order_id);
}
