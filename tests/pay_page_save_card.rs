mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::db;
use serde_json::{Value, json};

async fn state(h: &Harness, id: &str) -> Value {
    let r = send(
        &h.app,
        "GET",
        &format!("/pay/{id}/state"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    r.json
}

async fn page_attempt(h: &Harness, id: &str, body: Value) -> (StatusCode, Value) {
    let r = send(
        &h.app,
        "POST",
        &format!("/pay/{id}/attempts"),
        None,
        &[],
        body,
    )
    .await;
    (r.status, r.json)
}

fn known_customer(reference: &str) -> Value {
    json!({
        "reference": reference, "amount": 1500, "currency": "EUR",
        "customer_ref": format!("cust-{reference}"), "customer_email": "buyer@example.com"
    })
}

#[tokio::test]
async fn pay_page_offers_to_save_a_card_only_for_a_known_customer_with_an_email() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "stripe", json!({}), None).await;
    let anon = new_payment(&h, &t, eur("anon", 1500)).await;
    let no_email = new_payment(
        &h,
        &t,
        json!({"reference": "ne", "amount": 1500, "currency": "EUR", "customer_ref": "c1"}),
    )
    .await;
    let known = new_payment(&h, &t, known_customer("k")).await;
    assert_eq!(
        state(&h, anon["id"].as_str().unwrap()).await["can_save_card"],
        false
    );
    assert_eq!(
        state(&h, no_email["id"].as_str().unwrap()).await["can_save_card"],
        false
    );
    let s = state(&h, known["id"].as_str().unwrap()).await;
    assert_eq!(s["can_save_card"], true);
    assert!(!s.to_string().contains("cust-k"));
}

#[tokio::test]
async fn pay_page_attempt_with_save_method_stores_the_flag_and_lists_the_card_after_settlement() {
    let h = harness().await;
    let t = tenant(&h).await;
    let stripe = provider(&h, &t, "stripe", json!({}), None).await;
    let p = new_payment(&h, &t, known_customer("save")).await;
    let id = p["id"].as_str().unwrap();

    let (s, r) = page_attempt(&h, id, json!({"provider_id": stripe, "save_method": true})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    let row = db::get_attempt(&h.pool, aid).await.unwrap().unwrap();
    assert!(row.save_method);

    let hook = send(
        &h.app,
        "POST",
        &format!("/hook/{}/{stripe}", t.id),
        None,
        &[("x-mock-secret", "mock-secret")],
        json!({"lookup": row.lookup, "saved": "pm_card_1"}),
    )
    .await;
    assert_eq!(hook.status, StatusCode::OK, "{}", hook.text);

    let (s, saved) = call(
        &h.app,
        "GET",
        "/v1/saved-methods?customer_ref=cust-save",
        Some(&t.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(saved.as_array().unwrap().len(), 1, "{saved}");
}

#[tokio::test]
async fn pay_page_attempt_without_save_method_saves_nothing() {
    let h = harness().await;
    let t = tenant(&h).await;
    let stripe = provider(&h, &t, "stripe", json!({}), None).await;
    let p = new_payment(&h, &t, known_customer("nosave")).await;
    let id = p["id"].as_str().unwrap();
    let (s, r) = page_attempt(&h, id, json!({"provider_id": stripe})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    let row = db::get_attempt(&h.pool, aid).await.unwrap().unwrap();
    assert!(!row.save_method);
}

#[tokio::test]
async fn pay_page_refuses_save_method_without_a_customer_ref() {
    let h = harness().await;
    let t = tenant(&h).await;
    let stripe = provider(&h, &t, "stripe", json!({}), None).await;
    let p = new_payment(&h, &t, eur("anon", 1500)).await;
    let (s, _) = page_attempt(
        &h,
        p["id"].as_str().unwrap(),
        json!({"provider_id": stripe, "save_method": true}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn saved_card_from_the_pay_page_is_not_visible_to_another_tenant() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let stripe = provider(&h, &a, "stripe", json!({}), None).await;
    let p = new_payment(&h, &a, known_customer("iso")).await;
    let (s, r) = page_attempt(
        &h,
        p["id"].as_str().unwrap(),
        json!({"provider_id": stripe, "save_method": true}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    let lookup = db::get_attempt(&h.pool, aid).await.unwrap().unwrap().lookup;
    let hook = send(
        &h.app,
        "POST",
        &format!("/hook/{}/{stripe}", a.id),
        None,
        &[("x-mock-secret", "mock-secret")],
        json!({"lookup": lookup, "saved": "pm_card_1"}),
    )
    .await;
    assert_eq!(hook.status, StatusCode::OK, "{}", hook.text);
    let (s, theirs) = call(
        &h.app,
        "GET",
        "/v1/saved-methods?customer_ref=cust-iso",
        Some(&b.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(theirs.as_array().unwrap().is_empty(), "{theirs}");
    let (_, mine) = call(
        &h.app,
        "GET",
        "/v1/saved-methods?customer_ref=cust-iso",
        Some(&a.key),
        Value::Null,
    )
    .await;
    assert_eq!(mine.as_array().unwrap().len(), 1);
}
