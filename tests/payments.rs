mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};

#[tokio::test]
async fn create_get_and_find_by_reference() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(&h, &t, eur("order-1", 1500)).await;
    assert_eq!(p["status"], "pending");
    assert_eq!(p["amount"], 1500);
    assert!(
        p["pay_url"]
            .as_str()
            .unwrap()
            .starts_with("https://pay.test/pay/")
    );
    assert_eq!(p["methods"].as_array().unwrap().len(), 1);

    let id = p["id"].as_str().unwrap();
    let got = get_payment(&h, &t, id).await;
    assert_eq!(got["reference"], "order-1");

    let (_, found) = call(
        &h.app,
        "GET",
        "/v1/payments?reference=order-1",
        Some(&t.key),
        Value::Null,
    )
    .await;
    assert_eq!(found.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn payments_are_invisible_to_other_tenants() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let p = new_payment(&h, &a, eur("x", 100)).await;
    let id = p["id"].as_str().unwrap();
    let (s, _) = call(
        &h.app,
        "GET",
        &format!("/v1/payments/{id}"),
        Some(&b.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/cancel"),
        Some(&b.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = attempt(&h, &b, id, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn validation_rejects_bad_input() {
    let h = harness().await;
    let t = tenant(&h).await;
    let cases = [
        json!({"reference": "", "amount": 1, "currency": "EUR"}),
        json!({"reference": "r", "amount": 0, "currency": "EUR"}),
        json!({"reference": "r", "amount": 1, "currency": "XMR"}),
        json!({"reference": "r", "amount": 1, "currency": "ZZZ"}),
        json!({"reference": "r", "amount": 10, "currency": "EUR", "tax": 11}),
        json!({"reference": "r", "amount": 10, "currency": "EUR", "expires_in": 5}),
        json!({"reference": "r", "amount": 10, "currency": "EUR", "success_url": "javascript:x"}),
        json!({"reference": "r", "amount": 10, "currency": "EUR", "methods": ["bitcoin-cash"]}),
        json!({"reference": "r", "amount": 10, "currency": "EUR", "methods": []}),
    ];
    for c in cases {
        let (s, r) = call(&h.app, "POST", "/v1/payments", Some(&t.key), c.clone()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{c} -> {r}");
    }
}

#[tokio::test]
async fn idempotency_key_returns_same_payment_and_rejects_changed_params() {
    let h = harness().await;
    let t = tenant(&h).await;
    let first = send(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        &[("idempotency-key", "k1")],
        eur("o", 500),
    )
    .await;
    assert_eq!(first.status, StatusCode::CREATED);
    let again = send(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        &[("idempotency-key", "k1")],
        eur("o", 500),
    )
    .await;
    assert_eq!(again.status, StatusCode::OK);
    assert_eq!(first.json["id"], again.json["id"]);
    let changed = send(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        &[("idempotency-key", "k1")],
        eur("o", 501),
    )
    .await;
    assert_eq!(changed.status, StatusCode::CONFLICT);

    let other = tenant(&h).await;
    let other_first = send(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&other.key),
        &[("idempotency-key", "k1")],
        eur("o", 500),
    )
    .await;
    assert_eq!(other_first.status, StatusCode::CREATED);
    assert_ne!(other_first.json["id"], first.json["id"]);
}

#[tokio::test]
async fn concurrent_idempotent_creates_make_one_payment() {
    let h = harness().await;
    let t = tenant(&h).await;
    let mut tasks = vec![];
    for _ in 0..8 {
        let app = h.app.clone();
        let key = t.key.clone();
        tasks.push(tokio::spawn(async move {
            send(
                &app,
                "POST",
                "/v1/payments",
                Some(&key),
                &[("idempotency-key", "race")],
                eur("o", 700),
            )
            .await
        }));
    }
    let mut ids = std::collections::HashSet::new();
    for task in tasks {
        let r = task.await.unwrap();
        assert!(
            matches!(r.status, StatusCode::CREATED | StatusCode::OK),
            "{}",
            r.text
        );
        ids.insert(r.json["id"].as_str().unwrap().to_string());
    }
    assert_eq!(ids.len(), 1);
}

#[tokio::test]
async fn methods_depend_on_currency_and_allowed_list() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    provider(&h, &t, "stripe", json!({}), Some((2.0, 25, "EUR"))).await;

    let p = new_payment(&h, &t, eur("a", 1000)).await;
    let methods = p["methods"].as_array().unwrap();
    assert_eq!(methods.len(), 2);
    let stripe = methods.iter().find(|m| m["method"] == "stripe").unwrap();
    assert_eq!(stripe["fee"], 45);
    assert_eq!(stripe["total"], 1045);

    let btc = new_payment(
        &h,
        &t,
        json!({"reference": "b", "amount": 5_000_000, "currency": "BTC"}),
    )
    .await;
    let names: Vec<&str> = btc["methods"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["method"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["lightning"]);

    let only = new_payment(
        &h,
        &t,
        json!({"reference": "c", "amount": 1000, "currency": "EUR", "methods": ["lightning"]}),
    )
    .await;
    assert_eq!(only["methods"].as_array().unwrap().len(), 1);
    let (s, _) = attempt(
        &h,
        &t,
        only["id"].as_str().unwrap(),
        json!({"method": "stripe"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn lightning_attempt_converts_rounds_up_and_is_reused() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(&h, &t, eur("a", 5000)).await;
    let id = p["id"].as_str().unwrap();

    let (s, r) = attempt(&h, &t, id, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let a = &r["attempts"][0];
    assert_eq!(a["currency"], "BTC");
    assert_eq!(a["amount"], 100_000_000u64);
    assert!(
        a["details"]["bolt11"]
            .as_str()
            .unwrap()
            .starts_with("lnmock")
    );

    let (_, again) = attempt(&h, &t, id, json!({"method": "lightning"})).await;
    assert_eq!(again["attempts"].as_array().unwrap().len(), 1);
    assert_eq!(mock(&h, ln).created.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn attempt_fees_are_added_to_the_charge() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), Some((1.0, 0, "EUR"))).await;
    let p = new_payment(&h, &t, eur("a", 10_000)).await;
    let (_, r) = attempt(
        &h,
        &t,
        p["id"].as_str().unwrap(),
        json!({"method": "lightning"}),
    )
    .await;
    let a = &r["attempts"][0];
    assert_eq!(a["base_amount"], 10_000);
    assert_eq!(a["processing_fee"], 100);
    assert_eq!(a["amount"], 202_000_000u64);
}

#[tokio::test]
async fn attempts_on_closed_payments_are_refused() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(&h, &t, eur("a", 100)).await;
    let id = p["id"].as_str().unwrap();
    call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/cancel"),
        Some(&t.key),
        Value::Null,
    )
    .await;
    let (s, _) = attempt(&h, &t, id, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn provider_failure_is_a_bad_gateway_and_leaves_no_attempt() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(&h, &t, eur("a", 100)).await;
    let id = p["id"].as_str().unwrap();
    let (s, _) = attempt(&h, &t, id, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::OK);

    let p2 = new_payment(&h, &t, eur("b", 100)).await;
    let id2 = p2["id"].as_str().unwrap();
    *mock(&h, ln).fail_create.lock().unwrap() = true;
    let (s, _) = attempt(&h, &t, id2, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    let got = get_payment(&h, &t, id2).await;
    assert!(got["attempts"].as_array().unwrap().is_empty());
    assert_eq!(got["status"], "pending");
}

#[tokio::test]
async fn fixedfloat_requires_coin_option_and_an_onchain_provider() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "fixedfloat", json!({"coins": ["XMR", "LTC"]}), None).await;
    let p = new_payment(&h, &t, eur("a", 1000)).await;
    assert!(
        p["methods"].as_array().unwrap().is_empty(),
        "hidden without onchain"
    );

    provider(&h, &t, "onchain", json!({}), None).await;
    let p = new_payment(&h, &t, eur("b", 1000)).await;
    let id = p["id"].as_str().unwrap();
    let ff = p["methods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["method"] == "fixedfloat")
        .unwrap();
    assert_eq!(ff["options"], json!(["XMR", "LTC"]));

    let (s, _) = attempt(&h, &t, id, json!({"method": "fixedfloat"})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = attempt(
        &h,
        &t,
        id,
        json!({"method": "fixedfloat", "option": "DOGE"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, r) = attempt(&h, &t, id, json!({"method": "fixedfloat", "option": "xmr"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["attempts"][0]["option"], "XMR");
    assert!(
        r["attempts"][0]["details"]["deposit_address"]
            .as_str()
            .is_some()
    );
}
