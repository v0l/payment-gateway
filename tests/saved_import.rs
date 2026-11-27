mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::api_keys::{key_prefix, new_scoped_key};
use payment_gateway::crypto::hash_api_key;
use payment_gateway::db;
use serde_json::{Value, json};

async fn scoped_key(h: &Harness, tenant_id: u64, scopes: &str) -> String {
    let key = new_scoped_key("test");
    db::insert_api_key(
        &h.pool,
        &db::NewApiKey {
            tenant_id,
            name: scopes,
            key_hash: &hash_api_key(&key),
            key_prefix: &key_prefix(&key),
            scopes,
            mode: "test",
            expires: None,
        },
    )
    .await
    .unwrap();
    key
}

fn import(provider_id: u64) -> Value {
    json!({
        "customer_ref": "cust-1",
        "provider_id": provider_id,
        "external_customer_id": "cus_EXTERNAL_9",
        "external_id": "pm_EXTERNAL_9",
        "brand": "visa",
        "last4": "4242",
        "exp_month": 12,
        "exp_year": 2030
    })
}

async fn post_import(h: &Harness, key: &str, body: Value) -> (StatusCode, Value) {
    call(&h.app, "POST", "/v1/saved-methods", Some(key), body).await
}

async fn listed(h: &Harness, key: &str) -> Value {
    call(
        &h.app,
        "GET",
        "/v1/saved-methods?customer_ref=cust-1",
        Some(key),
        Value::Null,
    )
    .await
    .1
}

#[tokio::test]
async fn import_returns_the_saved_method_without_external_ids() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    let (s, r) = post_import(&h, &t.key, import(st)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["customer_ref"], "cust-1");
    assert_eq!(r["provider_id"], st);
    assert_eq!(r["brand"], "visa");
    assert_eq!(r["last4"], "4242");
    assert_eq!(r["exp_month"], 12);
    assert_eq!(r["exp_year"], 2030);
    let list = listed(&h, &t.key).await;
    for text in [r.to_string(), list.to_string()] {
        assert!(!text.contains("EXTERNAL_9"), "{text}");
        assert!(!text.contains("external"), "{text}");
    }
    assert_eq!(list.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn importing_twice_gives_one_row_and_the_same_id() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    let (_, first) = post_import(&h, &t.key, import(st)).await;
    let (s, second) = post_import(&h, &t.key, import(st)).await;
    assert_eq!(s, StatusCode::OK, "{second}");
    assert_eq!(first["id"], second["id"]);
    assert_eq!(listed(&h, &t.key).await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn import_for_another_customer_cannot_take_over_an_existing_method() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    post_import(&h, &t.key, import(st)).await;
    let mut other = import(st);
    other["customer_ref"] = json!("cust-2");
    let (s, r) = post_import(&h, &t.key, other).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    assert_eq!(r["error_detail"]["field"], "external_id");
}

#[tokio::test]
async fn imported_methods_are_private_to_their_tenant() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let sa = provider(&h, &a, "stripe", json!({}), None).await;
    let sb = provider(&h, &b, "stripe", json!({}), None).await;
    let (_, saved) = post_import(&h, &a.key, import(sa)).await;
    assert!(listed(&h, &b.key).await.as_array().unwrap().is_empty());
    let id = saved["id"].as_u64().unwrap();
    let (s, _) = call(
        &h.app,
        "DELETE",
        &format!("/v1/saved-methods/{id}"),
        Some(&b.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = post_import(&h, &b.key, import(sa)).await;
    assert_eq!(s, StatusCode::NOT_FOUND, "another tenant's provider");
    let (s, own) = post_import(&h, &b.key, import(sb)).await;
    assert_eq!(s, StatusCode::OK, "{own}");
    assert_ne!(own["id"], saved["id"]);
    assert_eq!(listed(&h, &a.key).await.as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn import_needs_the_saved_write_scope() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    for scopes in ["payments:read", "payments:write"] {
        let key = scoped_key(&h, t.id, scopes).await;
        let (s, r) = post_import(&h, &key, import(st)).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{scopes}");
        assert_eq!(r["error_detail"]["code"], "insufficient_scope");
    }
    assert!(listed(&h, &t.key).await.as_array().unwrap().is_empty());
    let key = scoped_key(&h, t.id, "saved:write").await;
    let (s, r) = post_import(&h, &key, import(st)).await;
    assert_eq!(s, StatusCode::OK, "{r}");
}

#[tokio::test]
async fn import_rejects_bad_input_and_non_card_providers() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let cases = [
        ("customer_ref", json!("")),
        ("external_id", json!("")),
        ("external_customer_id", json!("")),
        ("exp_month", json!(13)),
        ("exp_year", json!(30)),
        ("last4", json!("123456789")),
    ];
    for (field, bad) in cases {
        let mut body = import(st);
        body[field] = bad;
        let (s, r) = post_import(&h, &t.key, body).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{field}: {r}");
        assert_eq!(r["error_detail"]["field"], field);
    }
    let (s, _) = post_import(&h, &t.key, import(ln)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let mut missing = import(st);
    missing.as_object_mut().unwrap().remove("external_id");
    let (s, _) = post_import(&h, &t.key, missing).await;
    assert!(s.is_client_error());
    assert!(listed(&h, &t.key).await.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn imported_method_can_be_charged_off_session() {
    let h = harness().await;
    let t = tenant(&h).await;
    let st = provider(&h, &t, "stripe", json!({}), None).await;
    let (_, saved) = post_import(&h, &t.key, import(st)).await;
    let id = saved["id"].as_u64().unwrap();
    let r = send(
        &h.app,
        "POST",
        &format!("/v1/saved-methods/{id}/charge"),
        Some(&t.key),
        &[("idempotency-key", "imp-1")],
        json!({"reference": "renewal-1", "amount": 500, "currency": "EUR"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text);
}
