mod common;
use axum::Router;
use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::*;
use payment_gateway::api_keys::{key_prefix, new_scoped_key};
use payment_gateway::crypto::hash_api_key;
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::{ProviderSink, webhook_signature};
use payment_gateway::{accounts, db};
use serde_json::{Value, json};

async fn key_with(
    h: &Harness,
    tenant_id: u64,
    name: &str,
    scopes: &str,
    expires: Option<chrono::DateTime<Utc>>,
) -> String {
    let key = new_scoped_key("test");
    db::insert_api_key(
        &h.pool,
        &db::NewApiKey {
            tenant_id,
            name,
            key_hash: &hash_api_key(&key),
            key_prefix: &key_prefix(&key),
            scopes,
            mode: "test",
            expires,
        },
    )
    .await
    .unwrap();
    key
}

async fn get(h: &Harness, uri: &str, key: &str) -> Reply {
    send(&h.app, "GET", uri, Some(key), &[], Value::Null).await
}

async fn post(h: &Harness, uri: &str, key: &str, body: Value) -> Reply {
    send(&h.app, "POST", uri, Some(key), &[], body).await
}

async fn paid(h: &Harness, t: &TenantFixture, ln: u64, reference: &str) -> String {
    let p = new_payment(h, t, eur(reference, 1000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
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
    id
}

fn v1_routes() -> Vec<String> {
    let src = include_str!("../src/api/tenant.rs");
    let mut out = vec![];
    for line in src.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(".route(\"") {
            let path = rest.split('"').next().unwrap();
            let methods: Vec<&str> = ["get", "post", "delete", "put", "patch"]
                .into_iter()
                .filter(|m| rest.contains(&format!("{m}(")))
                .collect();
            for m in methods {
                out.push(format!("{m} {path}"));
            }
        }
    }
    out
}

#[tokio::test]
async fn every_v1_route_is_in_the_openapi_spec() {
    let h = harness().await;
    let r = send(&h.app, "GET", "/v1/openapi.json", None, &[], Value::Null).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.json["openapi"], "3.1.0");
    let routes = v1_routes();
    assert!(routes.len() >= 13, "{routes:?}");
    for route in routes {
        let (m, p) = route.split_once(' ').unwrap();
        assert!(r.json["paths"][p][m].is_object(), "missing {route}");
    }
}

#[tokio::test]
async fn docs_page_is_self_contained_and_csp_safe() {
    let h = harness().await;
    let page = send(&h.app, "GET", "/docs", None, &[], Value::Null).await;
    assert_eq!(page.status, StatusCode::OK);
    let csp = page.headers["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("script-src 'self'") && !csp.contains("unsafe"));
    assert!(!page.text.contains("http://") && !page.text.contains("https://"));
    assert!(!page.text.contains("<script>") && !page.text.contains("style="));
    for asset in ["/docs/docs.js", "/docs/docs.css"] {
        let a = send(&h.app, "GET", asset, None, &[], Value::Null).await;
        assert_eq!(a.status, StatusCode::OK, "{asset}");
    }
}

#[tokio::test]
async fn errors_keep_the_string_and_add_a_detail_object() {
    let h = harness().await;
    let t = tenant(&h).await;
    let r = get(&h, "/v1/payments?limit=0", &t.key).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.json["error"].is_string());
    assert_eq!(r.json["error_detail"]["code"], "invalid_request");
    assert_eq!(r.json["error_detail"]["field"], "limit");
    assert_eq!(r.json["error_detail"]["message"], r.json["error"]);
    let r = get(&h, "/v1/payments", "nope").await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.json["error_detail"]["code"], "unauthorized");
}

#[tokio::test]
async fn existing_tenant_key_is_a_full_scope_default_key() {
    let h = harness().await;
    let t = tenant(&h).await;
    let keys = db::list_api_keys(&h.pool, t.id).await.unwrap();
    assert_eq!(keys.len(), 1);
    assert_eq!(keys[0].name, "default");
    assert_eq!(keys[0].scopes, "admin");
    let new_key = new_scoped_key("test");
    db::rotate_tenant_key(&h.pool, t.id, &hash_api_key(&new_key))
        .await
        .unwrap();
    assert_eq!(
        get(&h, "/v1/methods", &t.key).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        get(&h, "/v1/methods", &new_key).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn scopes_are_enforced_per_endpoint() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let id = paid(&h, &t, ln, "a").await;
    let read = key_with(&h, t.id, "reader", "payments:read", None).await;
    let write = key_with(&h, t.id, "writer", "payments:write", None).await;
    let refunder = key_with(&h, t.id, "refunder", "refunds:write", None).await;
    let saved = key_with(&h, t.id, "saved", "saved:write", None).await;

    for uri in [
        "/v1/methods",
        "/v1/rates",
        "/v1/payments",
        "/v1/refunds",
        "/v1/events",
    ] {
        assert_eq!(get(&h, uri, &read).await.status, StatusCode::OK, "{uri}");
        let denied = get(&h, uri, &write).await;
        assert_eq!(denied.status, StatusCode::FORBIDDEN, "{uri}");
        assert_eq!(denied.json["error_detail"]["code"], "insufficient_scope");
    }
    assert_eq!(
        get(&h, &format!("/v1/payments/{id}"), &read).await.status,
        StatusCode::OK
    );

    let refund = |key: String| {
        let app = h.app.clone();
        let id = id.clone();
        async move {
            send(
                &app,
                "POST",
                &format!("/v1/payments/{id}/refund"),
                Some(&key),
                &[],
                json!({"amount": 100}),
            )
            .await
        }
    };
    assert_eq!(refund(read.clone()).await.status, StatusCode::FORBIDDEN);
    assert_eq!(refund(write.clone()).await.status, StatusCode::FORBIDDEN);
    assert_ne!(refund(refunder.clone()).await.status, StatusCode::FORBIDDEN);
    assert_ne!(refund(t.key.clone()).await.status, StatusCode::FORBIDDEN);

    assert_eq!(
        post(&h, "/v1/payments", &read, eur("x", 500)).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(&h, "/v1/payments", &write, eur("x", 500)).await.status,
        StatusCode::CREATED
    );
    assert_eq!(
        post(&h, &format!("/v1/payments/{id}/cancel"), &read, json!({}))
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(
            &h,
            &format!("/v1/payments/{id}/attempts"),
            &read,
            json!({"method": "lightning"})
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    assert_eq!(
        get(&h, "/v1/saved-methods?customer_ref=c", &saved)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        get(&h, "/v1/saved-methods?customer_ref=c", &read)
            .await
            .status,
        StatusCode::OK
    );
    assert_eq!(
        get(&h, "/v1/saved-methods?customer_ref=c", &write)
            .await
            .status,
        StatusCode::FORBIDDEN
    );
    for key in [&read, &write, &refunder] {
        let del = send(
            &h.app,
            "DELETE",
            "/v1/saved-methods/1",
            Some(key),
            &[],
            Value::Null,
        )
        .await;
        assert_eq!(del.status, StatusCode::FORBIDDEN);
        let charge = send(
            &h.app,
            "POST",
            "/v1/saved-methods/1/charge",
            Some(key),
            &[],
            eur("c", 100),
        )
        .await;
        assert_eq!(charge.status, StatusCode::FORBIDDEN);
    }
    let del = send(
        &h.app,
        "DELETE",
        "/v1/saved-methods/999999",
        Some(&saved),
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(del.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn expired_and_revoked_keys_are_rejected_and_last_used_is_recorded() {
    let h = harness().await;
    let t = tenant(&h).await;
    let expired = key_with(
        &h,
        t.id,
        "old",
        "admin",
        Some(Utc::now() - Duration::seconds(5)),
    )
    .await;
    let future = key_with(
        &h,
        t.id,
        "future",
        "admin",
        Some(Utc::now() + Duration::hours(1)),
    )
    .await;
    let revoked = key_with(&h, t.id, "gone", "admin", None).await;
    assert_eq!(
        get(&h, "/v1/methods", &expired).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(get(&h, "/v1/methods", &future).await.status, StatusCode::OK);
    assert_eq!(
        get(&h, "/v1/methods", &revoked).await.status,
        StatusCode::OK
    );
    let keys = db::list_api_keys(&h.pool, t.id).await.unwrap();
    let row = keys.iter().find(|k| k.name == "gone").unwrap();
    assert!(row.last_used.is_some());
    assert!(
        keys.iter()
            .find(|k| k.name == "old")
            .unwrap()
            .last_used
            .is_none()
    );
    assert!(
        db::revoke_api_key(&h.pool, t.id, row.id, Utc::now())
            .await
            .unwrap()
    );
    assert_eq!(
        get(&h, "/v1/methods", &revoked).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn key_secrets_are_stored_hashed_only() {
    let h = harness().await;
    let t = tenant(&h).await;
    let key = key_with(&h, t.id, "k", "admin", None).await;
    let rows: Vec<(String, String)> = sqlx::query_as("select key_hash, key_prefix from api_key")
        .fetch_all(&h.pool)
        .await
        .unwrap();
    assert!(
        rows.iter()
            .all(|(hash, prefix)| hash != &key && !key.eq(prefix) && hash.len() == 64)
    );
    assert!(rows.iter().any(|(hash, _)| hash == &hash_api_key(&key)));
}

#[tokio::test]
async fn paged_lists_have_no_gaps_or_duplicates() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    for i in 0..7 {
        new_payment(&h, &t, eur(&format!("r{i}"), 100)).await;
    }
    sqlx::query("update payment set created = '2030-01-01 00:00:00' where tenant_id = ?")
        .bind(t.id)
        .execute(&h.pool)
        .await
        .unwrap();
    let mut seen = vec![];
    let mut cursor: Option<String> = None;
    loop {
        let uri = match &cursor {
            Some(c) => format!("/v1/payments?limit=3&cursor={c}"),
            None => "/v1/payments?limit=3".to_string(),
        };
        let r = get(&h, &uri, &t.key).await;
        assert_eq!(r.status, StatusCode::OK);
        seen.extend(
            r.json
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["id"].as_str().unwrap().to_string()),
        );
        match r.headers.get("x-next-cursor") {
            Some(c) => cursor = Some(c.to_str().unwrap().to_string()),
            None => break,
        }
    }
    assert_eq!(seen.len(), 7);
    let mut dedup = seen.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(dedup.len(), 7);

    for i in 0..5 {
        paid(&h, &t, ln, &format!("p{i}")).await;
    }
    let mut events = vec![];
    let mut cursor: Option<String> = None;
    loop {
        let uri = match &cursor {
            Some(c) => format!("/v1/events?limit=2&cursor={c}"),
            None => "/v1/events?limit=2".to_string(),
        };
        let r = get(&h, &uri, &t.key).await;
        events.extend(
            r.json
                .as_array()
                .unwrap()
                .iter()
                .map(|e| e["id"].as_str().unwrap().to_string()),
        );
        match r.headers.get("x-next-cursor") {
            Some(c) => cursor = Some(c.to_str().unwrap().to_string()),
            None => break,
        }
    }
    assert_eq!(events.len(), 5);
    let typed = get(&h, "/v1/events?type=payment.expired", &t.key).await;
    assert!(typed.json.as_array().unwrap().is_empty());
    assert!(typed.headers.get("x-next-cursor").is_none());
}

#[tokio::test]
async fn payment_filters_and_optional_reference() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let paid_id = paid(&h, &t, ln, "ref-a").await;
    let auto = send(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        &[],
        json!({"amount": 500, "currency": "EUR", "customer_ref": "cust"}),
    )
    .await;
    assert_eq!(auto.status, StatusCode::CREATED, "{}", auto.text);
    assert!(!auto.json["reference"].as_str().unwrap().is_empty());

    let by_ref = get(&h, "/v1/payments?reference=ref-a", &t.key).await;
    assert_eq!(by_ref.json.as_array().unwrap().len(), 1);
    assert_eq!(by_ref.json[0]["id"], paid_id);
    let by_status = get(&h, "/v1/payments?status=paid", &t.key).await;
    assert_eq!(by_status.json.as_array().unwrap().len(), 1);
    let by_customer = get(&h, "/v1/payments?customer_ref=cust", &t.key).await;
    assert_eq!(by_customer.json.as_array().unwrap().len(), 1);
    let future = (Utc::now() + Duration::days(1))
        .to_rfc3339()
        .replace('+', "%2B");
    let none = get(&h, &format!("/v1/payments?created_from={future}"), &t.key).await;
    assert!(none.json.as_array().unwrap().is_empty(), "{}", none.text);
    let all = get(&h, "/v1/payments", &t.key).await;
    assert_eq!(all.json.as_array().unwrap().len(), 2);
    let bad = get(&h, "/v1/payments?cursor=zz", &t.key).await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn refunds_feed_pages_and_is_tenant_isolated() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let ln = provider(&h, &a, "lightning", json!({}), None).await;
    let id = paid(&h, &a, ln, "a1").await;
    for _ in 0..3 {
        let r = send(
            &h.app,
            "POST",
            &format!("/v1/payments/{id}/refund"),
            Some(&a.key),
            &[],
            json!({"amount": 100, "destination": "lnbc1"}),
        )
        .await;
        let _ = r;
    }
    let first = get(&h, "/v1/refunds?limit=2", &a.key).await;
    assert_eq!(first.status, StatusCode::OK);
    let n1 = first.json.as_array().unwrap().len();
    assert!(n1 >= 1);
    if let Some(c) = first.headers.get("x-next-cursor") {
        let second = get(
            &h,
            &format!("/v1/refunds?limit=2&cursor={}", c.to_str().unwrap()),
            &a.key,
        )
        .await;
        let ids: Vec<_> = first
            .json
            .as_array()
            .unwrap()
            .iter()
            .chain(second.json.as_array().unwrap())
            .map(|r| r["id"].as_u64().unwrap())
            .collect();
        let mut d = ids.clone();
        d.dedup();
        assert_eq!(ids, d);
    }
    let other = get(&h, "/v1/refunds", &b.key).await;
    assert!(other.json.as_array().unwrap().is_empty());
    let other_events = get(&h, "/v1/events", &b.key).await;
    assert!(other_events.json.as_array().unwrap().is_empty());
    let other_payments = get(&h, "/v1/payments", &b.key).await;
    assert!(other_payments.json.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn deliveries_fan_out_to_matching_endpoints_only() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let all = Receiver::start().await;
    let only_expired = Receiver::start().await;
    let paid_prefix = Receiver::start().await;
    let off = Receiver::start().await;
    let seal = |s: &str| h.service.sealer.seal(s.as_bytes()).unwrap();
    db::insert_endpoint(&h.pool, t.id, &all.url, &seal("whsec_all"), None, None)
        .await
        .unwrap();
    db::insert_endpoint(
        &h.pool,
        t.id,
        &only_expired.url,
        &seal("whsec_x"),
        None,
        Some("payment.expired"),
    )
    .await
    .unwrap();
    db::insert_endpoint(
        &h.pool,
        t.id,
        &paid_prefix.url,
        &seal("whsec_p"),
        None,
        Some("payment.*"),
    )
    .await
    .unwrap();
    let off_id = db::insert_endpoint(&h.pool, t.id, &off.url, &seal("whsec_o"), None, None)
        .await
        .unwrap();
    let mut e = db::get_endpoint(&h.pool, t.id, off_id)
        .await
        .unwrap()
        .unwrap();
    e.enabled = false;
    db::update_endpoint(&h.pool, t.id, &e).await.unwrap();

    paid(&h, &t, ln, "f1").await;
    h.service.deliver_due().await.unwrap();
    assert_eq!(t.receiver.types(), vec!["payment.paid"]);
    assert_eq!(all.types(), vec!["payment.paid"]);
    assert_eq!(paid_prefix.types(), vec!["payment.paid"]);
    assert!(only_expired.types().is_empty());
    assert!(off.types().is_empty());

    let (headers, body) = all.got.lock().unwrap()[0].clone();
    let sig = headers["x-gateway-signature"].to_str().unwrap().to_string();
    let (ts, v1) = sig.strip_prefix("t=").unwrap().split_once(",v1=").unwrap();
    assert_eq!(
        v1,
        webhook_signature("whsec_all", ts.parse().unwrap(), &body)
    );

    let stats = db::endpoint_stats(&h.pool, t.id).await.unwrap();
    assert_eq!(stats.iter().map(|s| s.delivered).sum::<i64>(), 3);
}

struct Console {
    app: Router,
    cookie: String,
    csrf: String,
}

impl Console {
    async fn req(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let cookie = format!("pgw_session={}", self.cookie);
        let r = send(
            &self.app,
            method,
            &format!("/console/api{path}"),
            None,
            &[("cookie", &cookie), ("x-csrf-token", &self.csrf)],
            body,
        )
        .await;
        (r.status, r.json)
    }
}

async fn console(h: &Harness, email: &str) -> Console {
    let hash = accounts::hash_password("correct horse battery").unwrap();
    accounts::insert_account(&h.pool, email, &hash, false)
        .await
        .unwrap();
    let r = send(
        &h.app,
        "POST",
        "/console/api/login",
        None,
        &[],
        json!({"email": email, "password": "correct horse battery"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let cookie = r.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("pgw_session=")
        .to_string();
    Console {
        app: h.app.clone(),
        cookie,
        csrf: r.json["csrf"].as_str().unwrap().to_string(),
    }
}

#[tokio::test]
async fn console_creates_lists_and_revokes_keys_and_shows_the_secret_once() {
    let h = harness().await;
    let a = console(&h, "a@example.com").await;
    let b = console(&h, "b@example.com").await;
    let (_, p) = a.req("POST", "/projects", json!({"name": "A"})).await;
    let pid = p["id"].as_u64().unwrap();
    let default_key = p["api_key"].as_str().unwrap().to_string();
    assert_eq!(
        get(&h, "/v1/methods", &default_key).await.status,
        StatusCode::OK
    );

    let (s, k) = a
        .req(
            "POST",
            &format!("/projects/{pid}/keys"),
            json!({"name": "reader", "scopes": ["payments:read"]}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{k}");
    let secret = k["api_key"].as_str().unwrap().to_string();
    assert!(
        secret.starts_with("pgw_test_"),
        "untrusted projects get test keys"
    );
    assert_eq!(get(&h, "/v1/methods", &secret).await.status, StatusCode::OK);
    assert_eq!(
        send(
            &h.app,
            "POST",
            "/v1/payments",
            Some(&secret),
            &[],
            eur("x", 100)
        )
        .await
        .status,
        StatusCode::FORBIDDEN
    );

    let (_, list) = a
        .req("GET", &format!("/projects/{pid}/keys"), Value::Null)
        .await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert!(!list.to_string().contains(&secret));
    assert!(!list.to_string().contains(&hash_api_key(&secret)));

    let (s, _) = a
        .req(
            "POST",
            &format!("/projects/{pid}/keys"),
            json!({"name": "reader", "scopes": ["payments:read"]}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = a
        .req(
            "POST",
            &format!("/projects/{pid}/keys"),
            json!({"name": "bad", "scopes": ["root"]}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = a
        .req(
            "POST",
            &format!("/projects/{pid}/keys"),
            json!({"name": "none", "scopes": []}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, _) = b
        .req("GET", &format!("/projects/{pid}/keys"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let kid = k["id"].as_u64().unwrap();
    let (s, _) = b
        .req(
            "POST",
            &format!("/projects/{pid}/keys/{kid}/revoke"),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, _) = a
        .req(
            "POST",
            &format!("/projects/{pid}/keys/{kid}/revoke"),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        get(&h, "/v1/methods", &secret).await.status,
        StatusCode::UNAUTHORIZED
    );

    db::set_tenant_trusted(&h.pool, pid, true).await.unwrap();
    let (_, live) = a
        .req(
            "POST",
            &format!("/projects/{pid}/keys"),
            json!({"name": "live", "scopes": ["admin"]}),
        )
        .await;
    assert!(live["api_key"].as_str().unwrap().starts_with("pgw_live_"));
}

#[tokio::test]
async fn console_endpoints_send_test_events_and_never_echo_secrets() {
    let h = harness().await;
    let a = console(&h, "a@example.com").await;
    let b = console(&h, "b@example.com").await;
    let (_, p) = a.req("POST", "/projects", json!({"name": "A"})).await;
    let pid = p["id"].as_u64().unwrap();
    db::set_tenant_trusted(&h.pool, pid, true).await.unwrap();
    let recv = Receiver::start().await;

    let (s, e) = a
        .req(
            "POST",
            &format!("/projects/{pid}/webhook-endpoints"),
            json!({"url": recv.url, "events": ["payment.paid"]}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{e}");
    let secret = e["secret"].as_str().unwrap().to_string();
    let eid = e["id"].as_u64().unwrap();
    let stored: Vec<u8> = sqlx::query_scalar("select secret from webhook_endpoint where id = ?")
        .bind(eid)
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert!(!String::from_utf8_lossy(&stored).contains(&secret));

    let (_, list) = a
        .req(
            "GET",
            &format!("/projects/{pid}/webhook-endpoints"),
            Value::Null,
        )
        .await;
    assert!(!list.to_string().contains(&secret));
    assert_eq!(list[0]["events"], json!(["payment.paid"]));

    let (s, r) = a
        .req(
            "POST",
            &format!("/projects/{pid}/webhook-endpoints/{eid}/test"),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["ok"], true, "{r}");
    let got = recv.got.lock().unwrap().clone();
    assert_eq!(got.len(), 1);
    let sig = got[0].0["x-gateway-signature"].to_str().unwrap();
    let (ts, v1) = sig.strip_prefix("t=").unwrap().split_once(",v1=").unwrap();
    assert_eq!(
        v1,
        webhook_signature(&secret, ts.parse().unwrap(), &got[0].1)
    );
    let body: Value = serde_json::from_str(&got[0].1).unwrap();
    assert_eq!(body["type"], "webhook.test");

    recv.fail_with(500);
    let (_, r) = a
        .req(
            "POST",
            &format!("/projects/{pid}/webhook-endpoints/{eid}/test"),
            json!({}),
        )
        .await;
    assert_eq!(r["ok"], false);

    let (s, _) = b
        .req(
            "GET",
            &format!("/projects/{pid}/webhook-endpoints"),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = a
        .req(
            "POST",
            &format!("/projects/{pid}/webhook-endpoints/{eid}"),
            json!({"enabled": false}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, list) = a
        .req(
            "GET",
            &format!("/projects/{pid}/webhook-endpoints"),
            Value::Null,
        )
        .await;
    assert_eq!(list[0]["enabled"], false);
    let (s, r) = a
        .req(
            "POST",
            &format!("/projects/{pid}/webhook-endpoints/{eid}/rotate-secret"),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_ne!(r["secret"], json!(secret));
    let (s, _) = a
        .req(
            "DELETE",
            &format!("/projects/{pid}/webhook-endpoints/{eid}"),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn untrusted_projects_cannot_add_endpoints_beyond_the_limit_or_to_internal_hosts() {
    let h = harness().await;
    let a = console(&h, "a@example.com").await;
    let (_, p) = a.req("POST", "/projects", json!({"name": "A"})).await;
    let pid = p["id"].as_u64().unwrap();
    let (s, _) = a
        .req(
            "POST",
            &format!("/projects/{pid}/webhook-endpoints"),
            json!({"url": "http://127.0.0.1:9/h"}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    sqlx::query("update tenant set limit_webhook_endpoints = 1 where id = ?")
        .bind(pid)
        .execute(&h.pool)
        .await
        .unwrap();
    db::insert_endpoint(
        &h.pool,
        pid,
        "https://example.com/h",
        &[1, 2, 3],
        None,
        None,
    )
    .await
    .unwrap();
    let (s, r) = a
        .req(
            "POST",
            &format!("/projects/{pid}/webhook-endpoints"),
            json!({"url": "https://example.com/other"}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{r}");
    assert_eq!(r["code"], "limit_exceeded");
}
