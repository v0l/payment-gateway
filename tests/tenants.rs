mod common;
use axum::http::StatusCode;
use common::*;
use serde_json::{Value, json};

#[tokio::test]
async fn admin_endpoints_reject_missing_or_wrong_token() {
    let h = harness().await;
    let (s, _) = call(&h.app, "GET", "/admin/tenants", None, Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(&h.app, "GET", "/admin/tenants", Some("nope"), Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn tenant_key_authenticates_and_rotation_revokes_old_key() {
    let h = harness().await;
    let t = tenant(&h).await;
    let (s, _) = call(&h.app, "GET", "/v1/methods", Some(&t.key), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(&h.app, "GET", "/v1/methods", None, Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/rotate-key", t.id),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let new_key = r["api_key"].as_str().unwrap();
    let (s, _) = call(&h.app, "GET", "/v1/methods", Some(&t.key), Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(&h.app, "GET", "/v1/methods", Some(new_key), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn disabled_tenant_is_rejected() {
    let h = harness().await;
    let t = tenant(&h).await;
    call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/enabled", t.id),
        Some(ADMIN),
        json!({"enabled": false}),
    )
    .await;
    let (s, _) = call(&h.app, "GET", "/v1/methods", Some(&t.key), Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn duplicate_tenant_name_conflicts_and_bad_webhook_url_rejected() {
    let h = harness().await;
    let name = unique("dup");
    let (s, _) = call(
        &h.app,
        "POST",
        "/admin/tenants",
        Some(ADMIN),
        json!({"name": name}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(
        &h.app,
        "POST",
        "/admin/tenants",
        Some(ADMIN),
        json!({"name": name}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = call(
        &h.app,
        "POST",
        "/admin/tenants",
        Some(ADMIN),
        json!({"name": unique("bad"), "webhook_url": "ftp://x"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn provider_configs_are_scoped_per_tenant_and_secrets_never_returned() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let pid = provider(
        &h,
        &a,
        "stripe",
        json!({"secret_key": "sk_live_topsecret"}),
        Some((1.5, 0, "EUR")),
    )
    .await;

    let (_, ms) = call(&h.app, "GET", "/v1/methods", Some(&a.key), Value::Null).await;
    assert_eq!(ms.as_array().unwrap().len(), 1);
    assert!(!ms.to_string().contains("topsecret"));
    let (_, ms) = call(&h.app, "GET", "/v1/methods", Some(&b.key), Value::Null).await;
    assert!(ms.as_array().unwrap().is_empty());
    let (_, list) = call(
        &h.app,
        "GET",
        &format!("/admin/tenants/{}/providers", a.id),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert!(!list.to_string().contains("topsecret"));

    call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers/{pid}/enabled", a.id),
        Some(ADMIN),
        json!({"enabled": false}),
    )
    .await;
    let (_, ms) = call(&h.app, "GET", "/v1/methods", Some(&a.key), Value::Null).await;
    assert!(ms.as_array().unwrap().is_empty());

    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers", a.id),
        Some(ADMIN),
        json!({"method": "bogus", "provider_type": "x", "name": "n", "config": {}}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn mock_providers_are_refused_when_mocks_are_disabled() {
    let h = harness().await;
    let t = tenant(&h).await;
    let app = payment_gateway::api::router(payment_gateway::api::AppState::new(
        h.service.clone(),
        ADMIN.into(),
        false,
        None,
    ));
    let (s, _) = call(
        &app,
        "POST",
        &format!("/admin/tenants/{}/providers", t.id),
        Some(ADMIN),
        json!({"method": "stripe", "provider_type": "mock", "name": "m", "config": {}}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn settings_validate_origins_colour_and_urls() {
    let h = harness().await;
    let t = tenant(&h).await;
    let uri = format!("/admin/tenants/{}/settings", t.id);
    let (s, r) = call(
        &h.app,
        "POST",
        &uri,
        Some(ADMIN),
        json!({
            "display_name": "Shop", "accent_color": "#ff8800",
            "allowed_origins": ["https://Shop.example/path", "http://localhost:3000"],
            "default_success_url": "https://shop.example/thanks"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(
        r["allowed_origins"],
        json!(["https://shop.example", "http://localhost:3000"])
    );
    for bad in [
        json!({"accent_color": "red"}),
        json!({"allowed_origins": ["javascript:alert(1)"]}),
        json!({"logo_url": "not a url"}),
    ] {
        let (s, _) = call(&h.app, "POST", &uri, Some(ADMIN), bad).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
}
