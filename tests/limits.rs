mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::api::{self, AppState};
use payment_gateway::limits::{LimitOverrides, Limits};
use payment_gateway::ratelimit::{RateConfig, RateLimits, Rule};
use serde_json::{Value, json};
use std::sync::Arc;

async fn set_limits(h: &Harness, id: u64, f: impl FnOnce(&mut LimitOverrides)) {
    let mut o = LimitOverrides::default();
    f(&mut o);
    assert!(db::set_tenant_limits(&h.pool, id, &o).await.unwrap());
}

use payment_gateway::db;

async fn create(
    h: &Harness,
    t: &TenantFixture,
    reference: &str,
    cents: u64,
) -> (StatusCode, Value) {
    call(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        eur(reference, cents),
    )
    .await
}

fn assert_limit(r: &(StatusCode, Value), name: &str, current: u64, max: u64) {
    assert_eq!(r.0, StatusCode::FORBIDDEN, "{}", r.1);
    assert_eq!(r.1["code"], "limit_exceeded");
    assert_eq!(r.1["limit"], name);
    assert_eq!(r.1["current"], current);
    assert_eq!(r.1["max"], max);
    assert!(r.1["error"].as_str().unwrap().contains(name));
}

fn limited_app(h: &Harness, config: RateConfig) -> axum::Router {
    let mut state = AppState::new(h.service.clone(), ADMIN.into(), true, None);
    state.rate_limits = Arc::new(RateLimits::new(config));
    api::router(state)
}

fn generous() -> RateConfig {
    let big = Rule::new(10_000, 1000.0);
    RateConfig {
        api_key: big,
        api_ip: big,
        pay_ip: big,
        pay_attempt_ip: big,
        signup_ip: big,
        login_ip: big,
        hook_ip: big,
    }
}

#[tokio::test]
async fn payments_per_day_blocks_at_the_boundary_and_resets_after_a_day() {
    let h = harness().await;
    let t = tenant(&h).await;
    set_limits(&h, t.id, |o| o.limit_payments_per_day = Some(3)).await;
    for i in 0..3 {
        assert_eq!(
            create(&h, &t, &format!("r{i}"), 100).await.0,
            StatusCode::CREATED
        );
    }
    assert_limit(&create(&h, &t, "r3", 100).await, "payments_per_day", 3, 3);
    sqlx::query("update payment set created = created - interval 25 hour where tenant_id = ?")
        .bind(t.id)
        .execute(&h.pool)
        .await
        .unwrap();
    assert_eq!(create(&h, &t, "r4", 100).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn pending_limit_counts_only_live_pending_payments() {
    let h = harness().await;
    let t = tenant(&h).await;
    set_limits(&h, t.id, |o| o.limit_pending_payments = Some(2)).await;
    create(&h, &t, "a", 100).await;
    create(&h, &t, "b", 100).await;
    assert_limit(&create(&h, &t, "c", 100).await, "pending_payments", 2, 2);

    sqlx::query("update payment set expires = now() - interval 1 minute where tenant_id = ? and reference = 'a'")
        .bind(t.id)
        .execute(&h.pool)
        .await
        .unwrap();
    assert_eq!(create(&h, &t, "c", 100).await.0, StatusCode::CREATED);

    sqlx::query("update payment set status = 'cancelled' where tenant_id = ? and reference = 'b'")
        .bind(t.id)
        .execute(&h.pool)
        .await
        .unwrap();
    assert_eq!(create(&h, &t, "d", 100).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn amount_per_payment_blocks_just_above_and_allows_the_limit() {
    let h = harness().await;
    let t = tenant(&h).await;
    set_limits(&h, t.id, |o| o.limit_amount_per_payment = Some(5000)).await;
    assert_eq!(create(&h, &t, "ok", 5000).await.0, StatusCode::CREATED);
    assert_limit(
        &create(&h, &t, "big", 5001).await,
        "amount_per_payment",
        5001,
        5000,
    );
}

#[tokio::test]
async fn volume_per_day_blocks_when_the_total_would_pass_the_limit() {
    let h = harness().await;
    let t = tenant(&h).await;
    set_limits(&h, t.id, |o| o.limit_volume_per_day = Some(1000)).await;
    assert_eq!(create(&h, &t, "a", 600).await.0, StatusCode::CREATED);
    assert_eq!(create(&h, &t, "b", 400).await.0, StatusCode::CREATED);
    assert_limit(&create(&h, &t, "c", 1).await, "volume_per_day", 1000, 1000);
    sqlx::query("update payment set created = created - interval 25 hour where tenant_id = ?")
        .bind(t.id)
        .execute(&h.pool)
        .await
        .unwrap();
    assert_eq!(create(&h, &t, "d", 1000).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn attempts_per_payment_blocks_past_the_cap() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let oc = provider(&h, &t, "onchain", json!({}), None).await;
    set_limits(&h, t.id, |o| o.limit_attempts_per_payment = Some(2)).await;
    let p = new_payment(&h, &t, eur("x", 1000)).await;
    let id = p["id"].as_str().unwrap();
    let (s, _) = attempt(
        &h,
        &t,
        id,
        json!({"method": "lightning", "provider_id": ln}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = attempt(&h, &t, id, json!({"method": "onchain", "provider_id": oc})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, r) = attempt(
        &h,
        &t,
        id,
        json!({"method": "lightning", "provider_id": ln, "save_method": false}),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::OK,
        "reusing a pending attempt is not a new attempt: {r}"
    );
    sqlx::query("update payment_attempt set status = 'expired' where payment_id = ?")
        .bind(hex::decode(id).unwrap())
        .execute(&h.pool)
        .await
        .unwrap();
    let r = attempt(
        &h,
        &t,
        id,
        json!({"method": "lightning", "provider_id": ln}),
    )
    .await;
    assert_limit(&r, "attempts_per_payment", 2, 2);
}

#[tokio::test]
async fn provider_cap_blocks_the_next_provider() {
    let h = harness().await;
    let t = tenant(&h).await;
    set_limits(&h, t.id, |o| o.limit_providers = Some(1)).await;
    provider_ok(&h, &t).await;
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers", t.id),
        Some(ADMIN),
        json!({"method": "lightning", "provider_type": "mock", "name": "second", "config": {}}),
    )
    .await;
    assert_limit(&(s, r), "providers", 1, 1);
}

async fn provider_ok(h: &Harness, t: &TenantFixture) {
    provider(h, t, "lightning", json!({}), None).await;
}

#[tokio::test]
async fn limits_are_per_tenant() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    set_limits(&h, a.id, |o| o.limit_payments_per_day = Some(1)).await;
    assert_eq!(create(&h, &a, "1", 100).await.0, StatusCode::CREATED);
    assert_eq!(create(&h, &a, "2", 100).await.0, StatusCode::FORBIDDEN);
    assert_eq!(create(&h, &b, "1", 100).await.0, StatusCode::CREATED);
    assert_eq!(create(&h, &b, "2", 100).await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn reference_over_255_and_metadata_over_8k_are_rejected() {
    let h = harness().await;
    let t = tenant(&h).await;
    let long = "x".repeat(256);
    assert_eq!(create(&h, &t, &long, 100).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        create(&h, &t, &"x".repeat(255), 100).await.0,
        StatusCode::CREATED
    );
    let mut body = eur("m", 100);
    body["metadata"] = json!({"k": "v".repeat(9000)});
    let (s, _) = call(&h.app, "POST", "/v1/payments", Some(&t.key), body).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn bodies_over_1_mib_are_rejected() {
    let h = harness().await;
    let t = tenant(&h).await;
    let mut body = eur("big", 100);
    body["description"] = json!("d".repeat(1024 * 1024 + 10));
    let (s, _) = call(&h.app, "POST", "/v1/payments", Some(&t.key), body).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn api_key_rate_limit_returns_429_with_retry_after_and_isolates_tenants() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let app = limited_app(
        &h,
        RateConfig {
            api_key: Rule::new(3, 0.1),
            ..generous()
        },
    );
    for _ in 0..3 {
        let r = send(&app, "GET", "/v1/methods", Some(&a.key), &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::OK);
    }
    let r = send(&app, "GET", "/v1/methods", Some(&a.key), &[], Value::Null).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    let secs: u64 = r.headers["retry-after"].to_str().unwrap().parse().unwrap();
    assert!((1..=10).contains(&secs));
    assert_eq!(r.json["code"], "rate_limited");
    let r = send(&app, "GET", "/v1/methods", Some(&b.key), &[], Value::Null).await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn ip_limits_apply_per_route_family_and_do_not_trust_a_spoofed_xff() {
    let h = harness().await;
    let app = limited_app(
        &h,
        RateConfig {
            api_ip: Rule::new(2, 0.01),
            pay_ip: Rule::new(2, 0.01),
            signup_ip: Rule::new(2, 0.01),
            login_ip: Rule::new(2, 0.01),
            hook_ip: Rule::new(2, 0.01),
            ..generous()
        },
    );
    let routes = [
        ("GET", "/v1/methods"),
        ("GET", "/pay/zz/state"),
        ("POST", "/hook/1/1"),
        ("POST", "/console/api/login"),
        ("POST", "/console/api/signup"),
    ];
    for (method, path) in routes {
        let hit = |xff: &'static str| {
            let app = app.clone();
            async move {
                send(
                    &app,
                    method,
                    path,
                    None,
                    &[("x-forwarded-for", xff)],
                    Value::Null,
                )
                .await
                .status
            }
        };
        assert_ne!(
            hit("1.1.1.1, 10.0.0.1").await,
            StatusCode::TOO_MANY_REQUESTS,
            "{path}"
        );
        assert_ne!(
            hit("2.2.2.2, 10.0.0.1").await,
            StatusCode::TOO_MANY_REQUESTS,
            "{path}"
        );
        assert_eq!(
            hit("3.3.3.3, 10.0.0.1").await,
            StatusCode::TOO_MANY_REQUESTS,
            "{path} spoofed first hop"
        );
        assert_ne!(
            hit("3.3.3.3, 10.0.0.2").await,
            StatusCode::TOO_MANY_REQUESTS,
            "{path} other client"
        );
    }
}

#[tokio::test]
async fn pay_page_attempt_creation_has_a_stricter_limit() {
    let h = harness().await;
    let app = limited_app(
        &h,
        RateConfig {
            pay_attempt_ip: Rule::new(2, 0.01),
            ..generous()
        },
    );
    let path = format!("/pay/{}/attempts", "ab".repeat(32));
    for _ in 0..2 {
        let r = send(
            &app,
            "POST",
            &path,
            None,
            &[("x-forwarded-for", "8.8.8.8")],
            json!({}),
        )
        .await;
        assert_ne!(r.status, StatusCode::TOO_MANY_REQUESTS);
    }
    let r = send(
        &app,
        "POST",
        &path,
        None,
        &[("x-forwarded-for", "8.8.8.8")],
        json!({}),
    )
    .await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(r.headers.contains_key("retry-after"));
    let state = format!("/pay/{}/state", "ab".repeat(32));
    let r = send(
        &app,
        "GET",
        &state,
        None,
        &[("x-forwarded-for", "8.8.8.8")],
        Value::Null,
    )
    .await;
    assert_ne!(r.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn trusted_and_untrusted_defaults_differ() {
    let h = harness().await;
    let t = tenant(&h).await;
    let tenant = db::get_tenant(&h.pool, t.id).await.unwrap().unwrap();
    assert!(tenant.trusted);
    assert_eq!(Limits::for_tenant(&tenant), Limits::TRUSTED);
    db::set_tenant_trusted(&h.pool, t.id, false).await.unwrap();
    let tenant = db::get_tenant(&h.pool, t.id).await.unwrap().unwrap();
    assert_eq!(Limits::for_tenant(&tenant), Limits::UNTRUSTED);
    set_limits(&h, t.id, |o| o.limit_providers = Some(9)).await;
    let tenant = db::get_tenant(&h.pool, t.id).await.unwrap().unwrap();
    assert_eq!(Limits::for_tenant(&tenant).providers, 9);
    assert_eq!(
        Limits::for_tenant(&tenant).pending_payments,
        Limits::UNTRUSTED.pending_payments
    );
}
