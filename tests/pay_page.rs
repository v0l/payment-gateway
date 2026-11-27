mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::db;
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::{ProviderSink, sign_return};
use serde_json::{Value, json};

async fn settings(h: &Harness, t: &TenantFixture, body: Value) {
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/settings", t.id),
        Some(ADMIN),
        body,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
}

#[tokio::test]
async fn pay_page_is_served_with_a_locked_down_csp_and_no_framing() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = new_payment(&h, &t, eur("o", 100)).await;
    let id = p["id"].as_str().unwrap();
    let r = send(&h.app, "GET", &format!("/pay/{id}"), None, &[], Value::Null).await;
    assert_eq!(r.status, StatusCode::OK);
    let csp = r.headers["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("frame-ancestors 'none'"), "{csp}");
    assert!(
        csp.contains("script-src 'self'") && !csp.contains("unsafe-inline"),
        "{csp}"
    );
    assert_eq!(r.headers["referrer-policy"], "no-referrer");
    assert!(
        csp.contains(
            "script-src 'self' https://merchant.revolut.com https://sandbox-merchant.revolut.com"
        ),
        "{csp}"
    );
    assert!(
        csp.contains("frame-src https://merchant.revolut.com"),
        "{csp}"
    );
    assert!(
        csp.contains(
            "connect-src 'self' https://merchant.revolut.com https://sandbox-merchant.revolut.com"
        ),
        "{csp}"
    );
    assert!(!csp.contains("*") && !csp.contains("unsafe-eval"), "{csp}");
    assert!(r.text.contains("/static/assets/pay-"));
    assert!(!r.text.contains("<script>"));
}

#[tokio::test]
async fn revolut_origins_are_allowed_on_the_pay_page_and_embed_only() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = new_payment(&h, &t, eur("o", 100)).await;
    let id = p["id"].as_str().unwrap();
    for path in [format!("/pay/{id}"), format!("/embed/{id}")] {
        let r = send(&h.app, "GET", &path, None, &[], Value::Null).await;
        let csp = r.headers["content-security-policy"].to_str().unwrap();
        assert!(
            csp.contains("connect-src 'self' https://merchant.revolut.com"),
            "{path} {csp}"
        );
    }
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let (s, r) = attempt(&h, &t, id, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: h.service.clone(),
        provider_id: ln,
    }
    .event(
        ProviderEvent::Paid {
            lookup: hash.clone(),
            reference: hash,
            amount: None,
            confirmed: true,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();
    let r = send(
        &h.app,
        "GET",
        &format!("/receipt/{id}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let csp = r
        .headers
        .get("content-security-policy")
        .expect("receipt sends a CSP")
        .to_str()
        .unwrap();
    for directive in ["script-src", "frame-src", "connect-src"] {
        let value = csp
            .split(';')
            .map(str::trim)
            .find(|d| d.starts_with(directive))
            .unwrap_or_default();
        assert!(!value.contains("revolut"), "{directive} in {csp}");
    }
    assert!(csp.contains("connect-src 'self'"), "{csp}");
}

#[tokio::test]
async fn embed_allows_only_the_tenants_origins_to_frame_it() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = new_payment(&h, &t, eur("o", 100)).await;
    let id = p["id"].as_str().unwrap();

    let none = send(
        &h.app,
        "GET",
        &format!("/embed/{id}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert!(
        none.headers["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'")
    );

    settings(
        &h,
        &t,
        json!({"allowed_origins": ["https://shop.example", "http://localhost:3000"]}),
    )
    .await;
    let r = send(
        &h.app,
        "GET",
        &format!("/embed/{id}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    let csp = r.headers["content-security-policy"].to_str().unwrap();
    assert!(
        csp.contains("frame-ancestors https://shop.example http://localhost:3000"),
        "{csp}"
    );
}

#[tokio::test]
async fn unknown_and_malformed_ids_are_404_and_disabled_tenants_hide_their_pages() {
    let h = harness().await;
    let t = tenant(&h).await;
    for uri in [
        "/pay/zzz",
        "/pay/00",
        "/embed/nothex",
        &format!("/pay/{}/state", "ab".repeat(32)),
    ] {
        let r = send(&h.app, "GET", uri, None, &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{uri}");
    }
    let p = new_payment(&h, &t, eur("o", 100)).await;
    let id = p["id"].as_str().unwrap();
    call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/enabled", t.id),
        Some(ADMIN),
        json!({"enabled": false}),
    )
    .await;
    let r = send(
        &h.app,
        "GET",
        &format!("/pay/{id}/state"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn public_state_shows_branding_and_methods_but_no_secrets_or_metadata() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(
        &h,
        &t,
        "stripe",
        json!({"secret_key": "sk_live_topsecret"}),
        Some((1.0, 0, "EUR")),
    )
    .await;
    settings(&h, &t, json!({"display_name": "Acme", "accent_color": "#ff8800", "logo_url": "https://acme.example/l.png"})).await;
    let p = new_payment(&h, &t, json!({
        "reference": "internal-ref-77", "amount": 2500, "currency": "EUR",
        "description": "Pro plan", "metadata": {"user": "secret-user-id"}, "customer_email": "x@y.z"
    })).await;
    let id = p["id"].as_str().unwrap();
    let r = send(
        &h.app,
        "GET",
        &format!("/pay/{id}/state"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["tenant"]["name"], "Acme");
    assert_eq!(r.json["tenant"]["accent_color"], "#ff8800");
    assert_eq!(r.json["description"], "Pro plan");
    assert_eq!(r.json["methods"][0]["fee"], 25);
    assert_eq!(r.headers["cache-control"], "no-store");
    for secret in ["topsecret", "internal-ref-77", "secret-user-id", "x@y.z"] {
        assert!(!r.text.contains(secret), "leaked {secret}");
    }
}

#[tokio::test]
async fn customer_can_start_an_attempt_from_the_page_but_only_with_this_tenants_providers() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let ln = provider(&h, &a, "lightning", json!({}), None).await;
    let foreign = provider(&h, &b, "lightning", json!({}), None).await;
    let p = new_payment(&h, &a, eur("o", 1000)).await;
    let id = p["id"].as_str().unwrap();

    let bad = send(
        &h.app,
        "POST",
        &format!("/pay/{id}/attempts"),
        None,
        &[],
        json!({"provider_id": foreign}),
    )
    .await;
    assert_eq!(bad.status, StatusCode::NOT_FOUND);

    let ok = send(
        &h.app,
        "POST",
        &format!("/pay/{id}/attempts"),
        None,
        &[],
        json!({"provider_id": ln}),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert!(ok.json["attempts"][0]["details"]["bolt11"].is_string());
}

#[tokio::test]
async fn public_state_hides_the_fixedfloat_order_token() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "onchain", json!({}), None).await;
    let ff = provider(&h, &t, "fixedfloat", json!({"coins": ["XMR"]}), None).await;
    let p = new_payment(&h, &t, eur("o", 1000)).await;
    let id = p["id"].as_str().unwrap();
    let r = send(
        &h.app,
        "POST",
        &format!("/pay/{id}/attempts"),
        None,
        &[],
        json!({"provider_id": ff, "option": "XMR"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(!r.text.contains("order_token"));
}

#[tokio::test]
async fn qr_renders_svg_for_lightning_and_refuses_other_payments_attempts() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    let p1 = new_payment(&h, &t, eur("a", 1000)).await;
    let p2 = new_payment(&h, &t, eur("b", 1000)).await;
    let id1 = p1["id"].as_str().unwrap();
    let id2 = p2["id"].as_str().unwrap();
    let (_, r) = attempt(&h, &t, id1, json!({"method": "lightning"})).await;
    let aid = r["attempts"][0]["id"].as_u64().unwrap();

    let ok = send(
        &h.app,
        "GET",
        &format!("/pay/{id1}/qr/{aid}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK);
    assert_eq!(ok.headers["content-type"], "image/svg+xml");
    assert!(ok.text.contains("<svg"));
    let cross = send(
        &h.app,
        "GET",
        &format!("/pay/{id2}/qr/{aid}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(cross.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn return_url_is_signed_and_only_appears_when_the_payment_is_final() {
    let h = harness().await;
    let t = tenant(&h).await;
    allow_shop_origin(&h, &t).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(&h, &t, json!({
        "reference": "order 9", "amount": 1000, "currency": "EUR",
        "success_url": "https://shop.example/thanks?x=1", "cancel_url": "https://shop.example/no"
    })).await;
    let id = p["id"].as_str().unwrap();
    let (_, r) = attempt(&h, &t, id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();

    let before = send(
        &h.app,
        "GET",
        &format!("/pay/{id}/state"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert!(before.json["return_url"].is_null());

    ProviderSink {
        service: h.service.clone(),
        provider_id: ln,
    }
    .event(
        ProviderEvent::Paid {
            lookup: hash.clone(),
            reference: hash,
            amount: None,
            confirmed: true,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();
    let after = send(
        &h.app,
        "GET",
        &format!("/pay/{id}/state"),
        None,
        &[],
        Value::Null,
    )
    .await;
    let url = url::Url::parse(after.json["return_url"].as_str().unwrap()).unwrap();
    assert_eq!(url.host_str(), Some("shop.example"));
    assert_eq!(url.path(), "/thanks");
    let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(q["x"], "1");
    assert_eq!(q["status"], "paid");
    assert_eq!(q["reference"], "order 9");
    assert_eq!(
        q["sig"],
        sign_return(&t.webhook_secret, id, "order 9", "paid")
    );
    assert_ne!(
        q["sig"],
        sign_return(&t.webhook_secret, id, "order 9", "cancelled")
    );
}

#[tokio::test]
async fn cancelled_payment_returns_to_the_cancel_url() {
    let h = harness().await;
    let t = tenant(&h).await;
    allow_shop_origin(&h, &t).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(
        &h,
        &t,
        json!({
            "reference": "r", "amount": 1000, "currency": "EUR",
            "success_url": "https://shop.example/ok", "cancel_url": "https://shop.example/no"
        }),
    )
    .await;
    let id = p["id"].as_str().unwrap();
    call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/cancel"),
        Some(&t.key),
        Value::Null,
    )
    .await;
    let r = send(
        &h.app,
        "GET",
        &format!("/pay/{id}/state"),
        None,
        &[],
        Value::Null,
    )
    .await;
    let url = url::Url::parse(r.json["return_url"].as_str().unwrap()).unwrap();
    assert_eq!(url.path(), "/no");
    assert!(url.query().unwrap().contains("status=cancelled"));
}

#[tokio::test]
async fn state_changes_are_pushed_over_the_event_stream() {
    use futures::StreamExt;
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(&h, &t, eur("o", 1000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(&h, &t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();

    let res = h
        .app
        .clone()
        .oneshot_get(&format!("/pay/{id}/events"))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()["content-type"], "text/event-stream");
    let mut body = res.into_body().into_data_stream();
    let first = tokio::time::timeout(std::time::Duration::from_secs(3), body.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&first).contains("event: change"));

    ProviderSink {
        service: h.service.clone(),
        provider_id: ln,
    }
    .event(
        ProviderEvent::Paid {
            lookup: hash.clone(),
            reference: hash,
            amount: None,
            confirmed: true,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();
    let next = tokio::time::timeout(std::time::Duration::from_secs(3), body.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(String::from_utf8_lossy(&next).contains("data: update"));

    let other = new_payment(&h, &t, eur("other", 1000)).await;
    h.service
        .notify(&db::parse_payment_id(other["id"].as_str().unwrap()).unwrap());
    let quiet = tokio::time::timeout(std::time::Duration::from_millis(300), body.next()).await;
    assert!(quiet.is_err(), "events for other payments must not leak");
}

#[tokio::test]
async fn built_assets_are_served_with_immutable_caching_and_pages_are_not_cached() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = new_payment(&h, &t, eur("o", 100)).await;
    let id = p["id"].as_str().unwrap();
    let page = send(&h.app, "GET", &format!("/pay/{id}"), None, &[], Value::Null).await;
    assert_eq!(page.headers["cache-control"], "no-store");
    let script = page
        .text
        .split('"')
        .find(|s| s.starts_with("/static/assets/pay-") && s.ends_with(".js"))
        .unwrap()
        .to_string();
    let style = page
        .text
        .split('"')
        .find(|s| s.starts_with("/static/assets/pay-") && s.ends_with(".css"))
        .unwrap()
        .to_string();
    for (uri, ct) in [(&script, "text/javascript"), (&style, "text/css")] {
        let r = send(&h.app, "GET", uri, None, &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::OK, "{uri}");
        assert_eq!(r.headers["content-type"], ct);
        assert_eq!(
            r.headers["cache-control"],
            "public, max-age=31536000, immutable"
        );
    }
    let r = send(
        &h.app,
        "GET",
        "/static/manifest.json",
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = send(
        &h.app,
        "GET",
        "/static/i18n/fr.json",
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn gateway_script_is_served_cross_origin_and_revalidated() {
    let h = harness().await;
    let g = send(&h.app, "GET", "/gateway.js", None, &[], Value::Null).await;
    assert_eq!(g.status, StatusCode::OK);
    assert_eq!(g.headers["content-type"], "text/javascript");
    assert!(g.text.contains("PaymentGateway"));
    assert_eq!(g.headers["access-control-allow-origin"], "*");
    assert_eq!(g.headers["cache-control"], "no-cache");
}

trait OneshotGet {
    async fn oneshot_get(self, uri: &str) -> axum::response::Response;
}

impl OneshotGet for axum::Router {
    async fn oneshot_get(self, uri: &str) -> axum::response::Response {
        use tower::ServiceExt;
        self.oneshot(
            axum::http::Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
    }
}

async fn allow_shop_origin(h: &Harness, t: &TenantFixture) {
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/settings", t.id),
        Some(ADMIN),
        json!({"allowed_origins": ["https://shop.example"]}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
}

#[tokio::test]
async fn return_urls_outside_allowed_origins_are_refused() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    let body =
        |u: &str| json!({"reference": "r", "amount": 100, "currency": "EUR", "success_url": u});
    let (s, _) = call(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        body("https://evil.example/x"),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    allow_shop_origin(&h, &t).await;
    let (s, _) = call(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        body("https://evil.example/x"),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        body("https://shop.example@evil.example/"),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        body("https://shop.example/ok"),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/settings", t.id),
        Some(ADMIN),
        json!({"allowed_origins": ["https://shop.example"], "default_success_url": "https://evil.example/"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
