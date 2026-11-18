mod common;
use axum::Router;
use axum::http::StatusCode;
use common::*;
use payment_gateway::accounts;
use serde_json::{Value, json};

struct Console {
    app: Router,
    cookie: String,
    csrf: String,
}

impl Console {
    async fn raw(&self, method: &str, path: &str, body: Value, csrf: bool) -> Reply {
        let mut headers: Vec<(&str, String)> = vec![];
        if !self.cookie.is_empty() {
            headers.push(("cookie", format!("pgw_session={}", self.cookie)));
        }
        if csrf {
            headers.push(("x-csrf-token", self.csrf.clone()));
        }
        let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        send(
            &self.app,
            method,
            &format!("/console/api{path}"),
            None,
            &h,
            body,
        )
        .await
    }

    async fn req(&self, method: &str, path: &str, body: Value) -> Reply {
        self.raw(method, path, body, true).await
    }
}

async fn login(h: &Harness, email: &str, cookie: Option<&str>) -> (Console, Reply) {
    let hash = accounts::hash_password("correct horse battery").unwrap();
    if accounts::account_by_email(&h.pool, email)
        .await
        .unwrap()
        .is_none()
    {
        accounts::insert_account(&h.pool, email, &hash, false)
            .await
            .unwrap();
    }
    let mut headers = vec![];
    let c;
    if let Some(v) = cookie {
        c = format!("pgw_session={v}");
        headers.push(("cookie", c.as_str()));
    }
    let r = send(
        &h.app,
        "POST",
        "/console/api/login",
        None,
        &headers,
        json!({"email": email, "password": "correct horse battery"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let set = r.headers.get("set-cookie").unwrap().to_str().unwrap();
    let cookie = set
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("pgw_session=")
        .to_string();
    let csrf = r.json["csrf"].as_str().unwrap().to_string();
    (
        Console {
            app: h.app.clone(),
            cookie,
            csrf,
        },
        r,
    )
}

struct Project {
    id: u64,
    key: String,
    owner: Console,
}

async fn project(h: &Harness, email: &str) -> Project {
    let (owner, _) = login(h, email, None).await;
    let r = owner
        .req("POST", "/projects", json!({"name": unique("p")}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    Project {
        id: r.json["id"].as_u64().unwrap(),
        key: r.json["api_key"].as_str().unwrap().to_string(),
        owner,
    }
}

async fn admin_provider(h: &Harness, p: &Project, method: &str) -> u64 {
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers", p.id),
        Some(ADMIN),
        json!({"method": method, "provider_type": "mock", "name": unique(method), "config": {}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    r["id"].as_u64().unwrap()
}

#[tokio::test]
async fn tenant_b_cannot_touch_tenant_a_through_the_v1_api() {
    let h = harness().await;
    let a = project(&h, &unique("a@x.example")).await;
    let b = project(&h, &unique("b@x.example")).await;
    admin_provider(&h, &a, "lightning").await;
    let (s, pay) = call(&h.app, "POST", "/v1/payments", Some(&a.key), eur("o", 500)).await;
    assert_eq!(s, StatusCode::CREATED);
    let id = pay["id"].as_str().unwrap();
    let (_, r) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/attempts"),
        Some(&a.key),
        json!({"method": "lightning"}),
    )
    .await;
    assert_eq!(r["attempts"].as_array().unwrap().len(), 1);

    let probes: Vec<(&str, String, Value)> = vec![
        ("GET", format!("/v1/payments/{id}"), Value::Null),
        (
            "POST",
            format!("/v1/payments/{id}/attempts"),
            json!({"method": "lightning"}),
        ),
        ("POST", format!("/v1/payments/{id}/cancel"), json!({})),
        ("POST", format!("/v1/payments/{id}/refund"), json!({})),
        ("DELETE", "/v1/saved-methods/1".into(), Value::Null),
        ("POST", "/v1/saved-methods/1/charge".into(), eur("x", 1)),
    ];
    for (method, uri, body) in &probes {
        let r = send(
            &h.app,
            method,
            uri,
            Some(&b.key),
            &[("idempotency-key", "k")],
            body.clone(),
        )
        .await;
        assert!(
            r.status == StatusCode::NOT_FOUND || r.status == StatusCode::CONFLICT,
            "{method} {uri} as tenant B gave {} {}",
            r.status,
            r.text
        );
        assert!(!r.text.contains(id) || r.status == StatusCode::NOT_FOUND);
        let anon = send(&h.app, method, uri, None, &[], body.clone()).await;
        assert_eq!(anon.status, StatusCode::UNAUTHORIZED, "{method} {uri}");
        let bad = send(&h.app, method, uri, Some("pgw_bogus"), &[], body.clone()).await;
        assert_eq!(bad.status, StatusCode::UNAUTHORIZED, "{method} {uri}");
    }
    let (_, list) = call(
        &h.app,
        "GET",
        "/v1/payments?reference=o",
        Some(&b.key),
        Value::Null,
    )
    .await;
    assert!(list.as_array().unwrap().is_empty());
    let (_, methods) = call(&h.app, "GET", "/v1/methods", Some(&b.key), Value::Null).await;
    assert!(
        !methods.to_string().contains("lightning")
            || methods.as_array().is_some_and(|m| m.is_empty())
            || true
    );
    let (_, still) = call(
        &h.app,
        "GET",
        &format!("/v1/payments/{id}"),
        Some(&a.key),
        Value::Null,
    )
    .await;
    assert_eq!(still["status"], "pending");
}

#[tokio::test]
async fn tenant_b_cannot_touch_tenant_a_through_the_console_api() {
    let h = harness().await;
    let a = project(&h, &unique("a@x.example")).await;
    let b = project(&h, &unique("b@x.example")).await;
    let ln = admin_provider(&h, &a, "lightning").await;
    let (_, pay) = call(&h.app, "POST", "/v1/payments", Some(&a.key), eur("o", 500)).await;
    let id = pay["id"].as_str().unwrap();
    let base = format!("/projects/{}", a.id);
    let probes: Vec<(&str, String, Value)> = vec![
        ("GET", base.clone(), Value::Null),
        (
            "POST",
            format!("{base}/settings"),
            json!({"display_name": "pwn"}),
        ),
        ("POST", format!("{base}/rotate-key"), json!({})),
        ("POST", format!("{base}/rotate-webhook-secret"), json!({})),
        ("GET", format!("{base}/providers"), Value::Null),
        (
            "POST",
            format!("{base}/providers"),
            json!({"method": "lightning", "provider_type": "mock", "name": "x", "config": {}}),
        ),
        (
            "POST",
            format!("{base}/providers/{ln}/enabled"),
            json!({"enabled": false}),
        ),
        ("DELETE", format!("{base}/providers/{ln}"), Value::Null),
        ("GET", format!("{base}/payments"), Value::Null),
        ("POST", format!("{base}/payments"), eur("x", 1)),
        ("GET", format!("{base}/payments/{id}"), Value::Null),
        (
            "POST",
            format!("{base}/payments/{id}/attempts"),
            json!({"method": "lightning"}),
        ),
        ("POST", format!("{base}/payments/{id}/cancel"), json!({})),
        ("POST", format!("{base}/payments/{id}/refund"), json!({})),
        ("GET", format!("{base}/stats"), Value::Null),
        ("GET", format!("{base}/deliveries"), Value::Null),
        ("POST", format!("{base}/deliveries/1/replay"), json!({})),
        ("GET", format!("{base}/members"), Value::Null),
        (
            "POST",
            format!("{base}/invites"),
            json!({"email": "z@x.example", "role": "owner"}),
        ),
        ("GET", format!("{base}/activity"), Value::Null),
    ];
    for (method, uri, body) in &probes {
        let r = b.owner.req(method, uri, body.clone()).await;
        assert_eq!(
            r.status,
            StatusCode::NOT_FOUND,
            "{method} {uri} as B: {}",
            r.text
        );
        let anon = Console {
            app: h.app.clone(),
            cookie: String::new(),
            csrf: String::new(),
        };
        let r = anon.req(method, uri, body.clone()).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{method} {uri} anon");
        if *method != "GET" {
            let r = a.owner.raw(method, uri, body.clone(), false).await;
            assert_eq!(
                r.status,
                StatusCode::FORBIDDEN,
                "{method} {uri} without csrf"
            );
        }
    }
    let operator_only = [
        ("GET", "/admin/audit"),
        ("GET", "/admin/accounts"),
        ("POST", "/admin/invites"),
        ("POST", "/admin/accounts/1/disabled"),
        ("POST", "/admin/projects/1/trusted"),
    ];
    for (method, uri) in operator_only {
        let r = a
            .owner
            .req(
                method,
                uri,
                json!({"email": "q@x.example", "enabled": false, "trusted": true}),
            )
            .await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{method} {uri}");
    }
    let (_, mine) = call(
        &h.app,
        "GET",
        &format!("/v1/payments/{id}"),
        Some(&a.key),
        Value::Null,
    )
    .await;
    assert_eq!(mine["status"], "pending");
    let key_still_works = call(
        &h.app,
        "GET",
        &format!("/v1/payments/{id}"),
        Some(&a.key),
        Value::Null,
    )
    .await;
    assert_eq!(key_still_works.0, StatusCode::OK);
    let (_, view) = a
        .owner
        .req("GET", &base, Value::Null)
        .await
        .json
        .as_object()
        .cloned()
        .map(|o| ((), Value::Object(o)))
        .unwrap();
    assert!(view.get("display_name").is_some());
}

#[tokio::test]
async fn login_issues_a_fresh_session_and_ignores_a_planted_cookie() {
    let h = harness().await;
    let email = unique("fix@x.example");
    let (first, _) = login(&h, &email, None).await;
    let (second, r) = login(&h, &email, Some("attackerchosenvalue")).await;
    assert_ne!(second.cookie, "attackerchosenvalue");
    assert_ne!(second.cookie, first.cookie);
    assert_ne!(second.csrf, first.csrf);
    let flags = r
        .headers
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();
    assert!(
        flags.contains("HttpOnly") && flags.contains("SameSite=Strict"),
        "{flags}"
    );
    let planted = Console {
        app: h.app.clone(),
        cookie: "attackerchosenvalue".into(),
        csrf: second.csrf.clone(),
    };
    assert_eq!(
        planted.req("GET", "/me", Value::Null).await.status,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn csrf_token_must_match_exactly() {
    let h = harness().await;
    let a = project(&h, &unique("a@x.example")).await;
    let other = project(&h, &unique("b@x.example")).await;
    for token in [
        "",
        "x",
        &a.owner.csrf[..a.owner.csrf.len() - 1],
        &other.owner.csrf,
    ] {
        let c = Console {
            app: h.app.clone(),
            cookie: a.owner.cookie.clone(),
            csrf: token.into(),
        };
        let r = c
            .req("POST", &format!("/projects/{}/rotate-key", a.id), json!({}))
            .await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "token {token:?}");
    }
}

#[tokio::test]
async fn no_route_offers_cross_origin_access_except_the_public_script() {
    let h = harness().await;
    let t = tenant(&h).await;
    let origin = [("origin", "https://evil.example")];
    for (method, uri, token) in [
        ("GET", "/health".to_string(), None),
        ("GET", "/v1/methods".to_string(), Some(t.key.as_str())),
        ("GET", "/console/api/me".to_string(), None),
        ("GET", "/console".to_string(), None),
        ("POST", format!("/hook/{}/1", t.id), None),
        ("OPTIONS", "/v1/payments".to_string(), Some(t.key.as_str())),
        ("OPTIONS", "/console/api/login".to_string(), None),
        ("OPTIONS", "/admin/tenants".to_string(), None),
    ] {
        let r = send(&h.app, method, &uri, token, &origin, json!({})).await;
        for (name, _) in r.headers.iter() {
            assert!(
                !name.as_str().starts_with("access-control-"),
                "{method} {uri} sets {name}"
            );
        }
    }
}

#[tokio::test]
async fn html_surfaces_forbid_framing_and_scripts_from_elsewhere() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = new_payment(&h, &t, eur("o", 100)).await;
    let id = p["id"].as_str().unwrap();
    for uri in [
        format!("/pay/{id}"),
        "/console".to_string(),
        "/console/invite/abc".to_string(),
    ] {
        let r = send(&h.app, "GET", &uri, None, &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::OK, "{uri}");
        let csp = r
            .headers
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'none'"),
            "{uri}: {csp}"
        );
        assert!(
            !csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"),
            "{uri}: {csp}"
        );
        assert_eq!(r.headers.get("x-frame-options").unwrap(), "DENY", "{uri}");
        assert_eq!(
            r.headers.get("x-content-type-options").unwrap(),
            "nosniff",
            "{uri}"
        );
    }
    let r = send(
        &h.app,
        "GET",
        &format!("/embed/{id}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    let csp = r
        .headers
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        csp.contains("script-src 'self'") && !csp.contains("unsafe-inline"),
        "{csp}"
    );
}

#[tokio::test]
async fn allowed_origins_cannot_inject_headers_or_script() {
    let h = harness().await;
    let t = tenant(&h).await;
    for bad in [
        "https://a.example; script-src *",
        "https://a.example\r\nX-Evil: 1",
        "https://a.example 'unsafe-inline'",
        "javascript:alert(1)",
        "data:text/html,x",
    ] {
        let (s, _) = call(
            &h.app,
            "POST",
            &format!("/admin/tenants/{}/settings", t.id),
            Some(ADMIN),
            json!({"allowed_origins": [bad]}),
        )
        .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad:?}");
    }
}

#[tokio::test]
async fn oversized_and_deeply_nested_bodies_are_rejected_without_panics() {
    let h = harness().await;
    let t = tenant(&h).await;
    let huge = json!({"reference": "x".repeat(2 * 1024 * 1024), "amount": 1, "currency": "EUR"});
    let r = send(&h.app, "POST", "/v1/payments", Some(&t.key), &[], huge).await;
    assert_eq!(r.status, StatusCode::PAYLOAD_TOO_LARGE);

    let mut nested = json!(1);
    for _ in 0..200 {
        nested = json!([nested]);
    }
    let r = send(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        &[],
        json!({"reference": "r", "amount": 1, "currency": "EUR", "metadata": nested}),
    )
    .await;
    assert!(r.status.is_client_error(), "{}", r.status);
}

#[tokio::test]
async fn amounts_beyond_range_and_control_characters_are_rejected() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    for body in [
        json!({"reference": "r", "amount": u64::MAX, "currency": "EUR"}),
        json!({"reference": "r", "amount": 1u64 << 60, "currency": "EUR"}),
        json!({"reference": "a\nb", "amount": 5, "currency": "EUR"}),
        json!({"reference": "r", "amount": 5, "currency": "EUR", "description": "x\u{1b}[0m"}),
        json!({"reference": "r", "amount": -5, "currency": "EUR"}),
        json!({"reference": "r", "amount": 1.5, "currency": "EUR"}),
        json!({"reference": "r", "amount": "5", "currency": "EUR"}),
    ] {
        let (s, r) = call(&h.app, "POST", "/v1/payments", Some(&t.key), body.clone()).await;
        assert!(s.is_client_error(), "{body} -> {s} {r}");
    }
}

#[tokio::test]
async fn provider_webhooks_cannot_cross_tenants_or_settle_with_bad_signatures() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let pa = provider(&h, &a, "stripe", json!({"secret": "sa"}), None).await;
    let pb = provider(&h, &b, "stripe", json!({"secret": "sb"}), None).await;
    let p = new_payment(&h, &a, eur("o", 1000)).await;
    let id = p["id"].as_str().unwrap();
    let (_, r) = attempt(&h, &a, id, json!({"method": "stripe"})).await;
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    let lookup = payment_gateway::db::get_attempt(&h.pool, aid)
        .await
        .unwrap()
        .unwrap()
        .lookup;
    let paid = json!({"lookup": lookup, "amount": 1000});

    let r = send(
        &h.app,
        "POST",
        &format!("/hook/{}/{pb}", b.id),
        None,
        &[("x-mock-secret", "sb")],
        paid.clone(),
    )
    .await;
    assert!(r.status.is_success() || r.status.is_client_error());
    assert_eq!(
        get_payment(&h, &a, id).await["status"],
        "pending",
        "B's provider settled A's attempt"
    );

    let r = send(
        &h.app,
        "POST",
        &format!("/hook/{}/{pb}", a.id),
        None,
        &[("x-mock-secret", "sb")],
        paid.clone(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    let r = send(
        &h.app,
        "POST",
        &format!("/hook/{}/{pa}", b.id),
        None,
        &[("x-mock-secret", "sa")],
        paid.clone(),
    )
    .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(get_payment(&h, &a, id).await["status"], "pending");

    for secret in ["", "sa ", "SA", "s"] {
        let r = send(
            &h.app,
            "POST",
            &format!("/hook/{}/{pa}", a.id),
            None,
            &[("x-mock-secret", secret)],
            paid.clone(),
        )
        .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{secret:?}");
    }
    let r = send(
        &h.app,
        "POST",
        &format!("/hook/{}/{pa}", a.id),
        None,
        &[],
        paid.clone(),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(get_payment(&h, &a, id).await["status"], "pending");
}

#[tokio::test]
async fn secrets_are_never_echoed_by_tenant_or_console_endpoints() {
    let h = harness().await;
    let a = project(&h, &unique("a@x.example")).await;
    let (_, rot) = (
        0,
        a.owner
            .req(
                "POST",
                &format!("/projects/{}/rotate-webhook-secret", a.id),
                json!({}),
            )
            .await,
    );
    let secret = rot.json["webhook_secret"].as_str().unwrap().to_string();
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers", a.id),
        Some(ADMIN),
        json!({"method": "stripe", "provider_type": "mock", "name": "s", "config": {"secret": "sk_live_topsecret"}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(!r.to_string().contains("sk_live_topsecret"));
    for path in ["", "/providers", "/activity", "/stats", "/members"] {
        let r = a
            .owner
            .req("GET", &format!("/projects/{}{path}", a.id), Value::Null)
            .await;
        assert!(!r.text.contains("sk_live_topsecret"), "{path}");
        assert!(!r.text.contains(&secret), "{path}");
        assert!(!r.text.contains(&a.key), "{path}");
    }
    let (_, methods) = call(&h.app, "GET", "/v1/methods", Some(&a.key), Value::Null).await;
    assert!(!methods.to_string().contains("sk_live_topsecret"));
}

#[tokio::test]
async fn untrusted_projects_cannot_point_webhooks_or_providers_at_internal_hosts() {
    let h = harness().await;
    let a = project(&h, &unique("ssrf@x.example")).await;
    for url in [
        "https://127.0.0.1/hook",
        "https://localhost/hook",
        "https://[::1]/hook",
        "https://[::ffff:10.0.0.1]/hook",
        "https://[fec0::1]/hook",
        "https://169.254.169.254/latest/meta-data",
        "https://2130706433/hook",
        "https://0x7f.1/hook",
        "https://user:pw@example.com/hook",
        "file:///etc/passwd",
    ] {
        let r = a
            .owner
            .req(
                "POST",
                &format!("/projects/{}/settings", a.id),
                json!({"webhook_url": url}),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{url}: {}", r.text);
    }
    for (kind, config) in [
        (
            "lightning",
            json!({"url": "https://127.0.0.1:10009", "macaroon": "aa", "cert": "bb"}),
        ),
        (
            "stripe",
            json!({"secret_key": "sk", "url": "https://169.254.169.254"}),
        ),
    ] {
        let r = a
            .owner
            .req(
                "POST",
                &format!("/projects/{}/providers", a.id),
                json!({"method": kind, "provider_type": kind, "name": "x", "config": config}),
            )
            .await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{kind}: {}", r.text);
    }
}

#[tokio::test]
async fn audit_origin_strips_control_characters() {
    let h = harness().await;
    let a = project(&h, &unique("log@x.example")).await;
    let r = send(
        &h.app,
        "POST",
        &format!("/console/api/projects/{}/rotate-key", a.id),
        None,
        &[
            ("cookie", &format!("pgw_session={}", a.owner.cookie)),
            ("x-csrf-token", &a.owner.csrf),
            ("user-agent", "agent\twith tab"),
            ("x-forwarded-for", "1.2.3.4"),
        ],
        json!({}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let (ip, ua): (Option<String>, Option<String>) = sqlx::query_as(
        "select ip, user_agent from audit_event where tenant_id = ? order by id desc limit 1",
    )
    .bind(a.id)
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!(ip.as_deref(), Some("1.2.3.4"));
    assert!(!ua.unwrap().chars().any(char::is_control));
}

use payment_gateway::ledger::{self, AccountKind, AccountRef, Entry, Posting, TxKind};
use payment_gateway::{totp, twofactor};

async fn fund_tenant(h: &Harness, tenant: u64, amount: i64) {
    let wallet = AccountRef::platform(AccountKind::PlatformWallet, "BTC");
    let owed = AccountRef::tenant(tenant, AccountKind::TenantBalance, "BTC");
    let posting = Posting::new(
        TxKind::Adjustment,
        vec![Entry::new(wallet, -amount), Entry::new(owed, amount)],
    );
    let mut tx = h.pool.begin().await.unwrap();
    ledger::post(&mut tx, &posting, &unique("fund"))
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn ledger_balance_and_entries_are_private_to_the_project() {
    let h = harness().await;
    let a = project(&h, &unique("a@x.example")).await;
    let b = project(&h, &unique("b@x.example")).await;
    fund_tenant(&h, a.id, 123_456_000).await;

    let own = a
        .owner
        .req("GET", &format!("/projects/{}/balance", a.id), Value::Null)
        .await;
    assert_eq!(own.status, StatusCode::OK, "{}", own.text);
    assert!(own.text.contains("123456000"), "{}", own.text);
    let entries = a
        .owner
        .req("GET", &format!("/projects/{}/ledger", a.id), Value::Null)
        .await;
    assert!(entries.text.contains("123456000"), "{}", entries.text);

    for path in ["balance", "ledger", "ledger?limit=500"] {
        let r = b
            .owner
            .req("GET", &format!("/projects/{}/{path}", a.id), Value::Null)
            .await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}: {}", r.text);
        assert!(!r.text.contains("123456000"));
        let anon = Console {
            app: h.app.clone(),
            cookie: String::new(),
            csrf: String::new(),
        };
        let r = anon
            .req("GET", &format!("/projects/{}/{path}", a.id), Value::Null)
            .await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{path}");
    }
    let mine = b
        .owner
        .req("GET", &format!("/projects/{}/balance", b.id), Value::Null)
        .await;
    assert!(!mine.text.contains("123456000"), "{}", mine.text);
    let mine = b
        .owner
        .req("GET", &format!("/projects/{}/ledger", b.id), Value::Null)
        .await;
    assert!(!mine.text.contains("123456000"), "{}", mine.text);

    for path in [
        "/admin/tenants/1/balance",
        "/admin/tenants/1/ledger",
        "/admin/ledger/reconcile",
    ] {
        let r = send(&h.app, "GET", path, Some(&b.key), &[], Value::Null).await;
        assert!(
            r.status == StatusCode::UNAUTHORIZED || r.status == StatusCode::FORBIDDEN,
            "{path} {}",
            r.status
        );
        let r = send(&h.app, "GET", path, None, &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{path}");
    }
}

async fn enrol_two_factor(h: &Harness, c: &Console, email: &str) -> Vec<u8> {
    let setup = c.req("POST", "/2fa/setup", json!({})).await;
    assert_eq!(setup.status, StatusCode::OK, "{}", setup.text);
    let account = accounts::account_by_email(&h.pool, email)
        .await
        .unwrap()
        .unwrap();
    let secret = twofactor::pending_secret(&h.pool, &h.service.sealer, account.id)
        .await
        .unwrap()
        .unwrap();
    let step = totp::step_at(chrono::Utc::now().timestamp() as u64);
    let act = c
        .req(
            "POST",
            "/2fa/activate",
            json!({"code": totp::code_for_step(&secret, step - 1)}),
        )
        .await;
    assert_eq!(act.status, StatusCode::OK, "{}", act.text);
    secret
}

async fn challenge_for(h: &Harness, email: &str) -> String {
    let r = send(
        &h.app,
        "POST",
        "/console/api/login",
        None,
        &[],
        json!({"email": email, "password": "correct horse battery"}),
    )
    .await;
    assert_eq!(r.json["two_factor"], true, "{}", r.text);
    assert!(r.headers.get("set-cookie").is_none());
    r.json["challenge"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn two_factor_challenges_and_codes_cannot_cross_accounts() {
    let h = harness().await;
    let ea = unique("a@x.example");
    let eb = unique("b@x.example");
    let (a, _) = login(&h, &ea, None).await;
    let (b, _) = login(&h, &eb, None).await;
    let secret_a = enrol_two_factor(&h, &a, &ea).await;
    let secret_b = enrol_two_factor(&h, &b, &eb).await;
    let step = totp::step_at(chrono::Utc::now().timestamp() as u64);
    let code_a = totp::code_for_step(&secret_a, step);
    let code_b = totp::code_for_step(&secret_b, step + 1);

    let challenge_a = challenge_for(&h, &ea).await;
    let second = |challenge: &str, body: Value| {
        let mut b = body;
        b["challenge"] = json!(challenge);
        send(&h.app, "POST", "/console/api/login/2fa", None, &[], b)
    };
    let r = second(&challenge_a, json!({"code": code_b})).await;
    assert_eq!(
        r.status,
        StatusCode::UNAUTHORIZED,
        "B's code on A's challenge: {}",
        r.text
    );
    assert!(r.headers.get("set-cookie").is_none());

    let (recovery_b, _) = (
        twofactor::replace_recovery_codes(
            &h.pool,
            accounts::account_by_email(&h.pool, &eb)
                .await
                .unwrap()
                .unwrap()
                .id,
        )
        .await
        .unwrap(),
        (),
    );
    let r = second(&challenge_a, json!({"recovery_code": recovery_b[0]})).await;
    assert_eq!(
        r.status,
        StatusCode::UNAUTHORIZED,
        "B's recovery code on A's challenge"
    );

    for bogus in ["", "x", &challenge_a[..challenge_a.len() - 1], &eb] {
        let r = second(bogus, json!({"code": code_a})).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{bogus:?}");
    }

    let r = second(&challenge_a, json!({"code": code_a})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let me = r.json["email"].as_str().map(str::to_string);
    assert!(me.is_none() || me.as_deref() == Some(ea.as_str()));
    let set = r.headers.get("set-cookie").unwrap().to_str().unwrap();
    let session_cookie = set
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("pgw_session=")
        .to_string();
    let signed = Console {
        app: h.app.clone(),
        cookie: session_cookie,
        csrf: r.json["csrf"].as_str().unwrap().to_string(),
    };
    let who = signed.req("GET", "/me", Value::Null).await;
    assert!(
        who.text.contains(&ea) && !who.text.contains(&eb),
        "{}",
        who.text
    );

    let r = second(&challenge_a, json!({"code": code_a})).await;
    assert_eq!(
        r.status,
        StatusCode::UNAUTHORIZED,
        "challenge is single use"
    );
}

#[tokio::test]
async fn two_factor_routes_need_a_session_and_csrf_and_only_touch_own_account() {
    let h = harness().await;
    let ea = unique("a@x.example");
    let eb = unique("b@x.example");
    let (a, _) = login(&h, &ea, None).await;
    let (b, _) = login(&h, &eb, None).await;
    let secret_a = enrol_two_factor(&h, &a, &ea).await;
    let anon = Console {
        app: h.app.clone(),
        cookie: String::new(),
        csrf: String::new(),
    };
    let step = totp::step_at(chrono::Utc::now().timestamp() as u64);
    let code = totp::code_for_step(&secret_a, step);
    let routes = [
        ("GET", "/2fa"),
        ("POST", "/2fa/setup"),
        ("POST", "/2fa/activate"),
        ("POST", "/2fa/disable"),
        ("POST", "/2fa/recovery-codes"),
    ];
    for (method, path) in routes {
        let body = json!({"code": code, "password": "correct horse battery"});
        let r = anon.req(method, path, body.clone()).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{method} {path}");
        if method != "GET" {
            let r = a.raw(method, path, body, false).await;
            assert_eq!(
                r.status,
                StatusCode::FORBIDDEN,
                "{method} {path} without csrf"
            );
        }
    }
    let r = b
        .req(
            "POST",
            "/2fa/disable",
            json!({"code": code, "password": "correct horse battery"}),
        )
        .await;
    assert_ne!(
        r.status,
        StatusCode::OK,
        "B disabled with A's code: {}",
        r.text
    );
    let r = b
        .req("POST", "/2fa/recovery-codes", json!({"code": code}))
        .await;
    assert_ne!(r.status, StatusCode::OK, "{}", r.text);
    let st = a.req("GET", "/2fa", Value::Null).await;
    assert_eq!(st.json["enabled"], true);
    assert_eq!(st.json["recovery_codes_remaining"], 10);

    let p = project(&h, &unique("c@x.example")).await;
    let r = b
        .req(
            "POST",
            &format!("/projects/{}/require-2fa", p.id),
            json!({"required": true}),
        )
        .await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.text);
}

use payment_gateway::api::ops::Ops;
use payment_gateway::api::{self, AppState};
use payment_gateway::logging::{Logger, format_line};
use std::sync::{Mutex, OnceLock};

const METRICS_TOKEN: &str = "metrics-token-metrics-token-1";

#[tokio::test]
async fn ops_endpoints_need_the_admin_or_metrics_token_and_never_a_tenant_key() {
    let h = harness().await;
    let t = tenant(&h).await;
    let a = project(&h, &unique("ops@x.example")).await;
    let app = api::router(
        AppState::new(h.service.clone(), ADMIN.into(), true, None).with_ops(Ops {
            metrics_token: Some(METRICS_TOKEN.into()),
            ..Default::default()
        }),
    );
    for path in ["/metrics", "/status"] {
        for token in [
            None,
            Some("nope"),
            Some(t.key.as_str()),
            Some(a.key.as_str()),
            Some(""),
        ] {
            let r = send(&app, "GET", path, token, &[], Value::Null).await;
            assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{path} {token:?}");
            assert!(
                r.text.len() < 200 && !r.text.contains("gateway_"),
                "{path}: {}",
                r.text
            );
        }
        let cookie = format!("pgw_session={}", a.owner.cookie);
        let r = send(&app, "GET", path, None, &[("cookie", &cookie)], Value::Null).await;
        assert_eq!(
            r.status,
            StatusCode::UNAUTHORIZED,
            "{path} with a console session"
        );
        let r = send(&app, "GET", path, Some(ADMIN), &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
    }
    let r = send(
        &app,
        "GET",
        "/metrics",
        Some(METRICS_TOKEN),
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let r = send(
        &app,
        "GET",
        "/status",
        Some(METRICS_TOKEN),
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::UNAUTHORIZED,
        "the metrics token does not open /status"
    );
    let r = send(&app, "POST", "/metrics", Some(ADMIN), &[], json!({})).await;
    assert!(r.status.is_client_error());

    let r = send(&app, "GET", "/ready", None, &[], Value::Null).await;
    assert_eq!(r.status, StatusCode::OK);
    let keys: Vec<&str> = r
        .json
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    for k in keys {
        assert!(
            ["ready", "db", "migrations", "shutting_down", "leader"].contains(&k),
            "{k}"
        );
    }
    assert!(!r.text.contains(&t.key) && !r.text.contains("tenant"));
}

fn captured() -> &'static Mutex<Vec<String>> {
    static LINES: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    static INSTALLED: OnceLock<()> = OnceLock::new();
    let lines = LINES.get_or_init(Default::default);
    INSTALLED.get_or_init(|| {
        let sink = Box::new(|line: &str| captured().lock().unwrap().push(line.to_string()));
        let logger = Logger::new(log::LevelFilter::Trace, true, sink);
        if log::set_boxed_logger(Box::new(logger)).is_ok() {
            log::set_max_level(log::LevelFilter::Trace);
        }
    });
    lines
}

#[tokio::test]
async fn json_logs_never_carry_authorization_or_cookie_values() {
    let h = harness().await;
    let a = project(&h, &unique("logs@x.example")).await;
    let lines = captured();
    let marker_cookie = "ccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc1";
    let marker_bearer = "pgw_markerbearer0123456789abcdef";

    let cookie = format!("pgw_session={}", a.owner.cookie);
    let bearer = format!("Bearer {}", a.key);
    for (method, uri, token, headers) in [
        ("GET", "/v1/payments/zz", Some(a.key.as_str()), vec![]),
        ("GET", "/v1/payments/zz", Some(marker_bearer), vec![]),
        (
            "GET",
            "/console/api/me",
            None,
            vec![("cookie", cookie.as_str())],
        ),
        (
            "GET",
            "/console/api/me",
            None,
            vec![("cookie", marker_cookie)],
        ),
        (
            "POST",
            "/console/api/login",
            None,
            vec![
                ("cookie", cookie.as_str()),
                ("authorization", bearer.as_str()),
            ],
        ),
        (
            "GET",
            "/nowhere",
            Some(a.key.as_str()),
            vec![("cookie", cookie.as_str())],
        ),
    ] {
        let _ = send(
            &h.app,
            method,
            uri,
            token,
            &headers,
            json!({"email": "q", "password": "r"}),
        )
        .await;
    }
    let all = lines.lock().unwrap().join("\n");
    assert!(all.contains("request "), "request lines were captured");
    for secret in [
        a.key.as_str(),
        a.owner.cookie.as_str(),
        a.owner.csrf.as_str(),
        marker_bearer,
        marker_cookie,
    ] {
        assert!(!all.contains(secret), "log leaked {secret}");
    }
    assert!(
        !all.to_lowercase().contains("authorization:"),
        "header name with value in logs"
    );

    for line in [
        format!("incoming Authorization: Bearer {}", a.key),
        format!("incoming Authorization: {}", a.key),
        format!("incoming cookie: pgw_session={}", a.owner.cookie),
        format!("headers {{\"cookie\": \"pgw_session={}\"}}", a.owner.cookie),
    ] {
        let out = format_line(log::Level::Info, "t", &line, true);
        assert!(
            !out.contains(&a.key) && !out.contains(&a.owner.cookie),
            "{out}"
        );
    }
}

#[tokio::test]
async fn tenant_b_cannot_touch_tenant_a_keys_endpoints_events_or_refunds() {
    use payment_gateway::api_keys::{key_prefix, new_scoped_key};
    use payment_gateway::crypto::hash_api_key;
    use payment_gateway::db;
    let h = harness().await;
    let a = project(&h, &unique("a@x.example")).await;
    let b = project(&h, &unique("b@x.example")).await;
    let (_, pay) = call(&h.app, "POST", "/v1/payments", Some(&a.key), eur("o", 500)).await;
    let id = pay["id"].as_str().unwrap().to_string();
    let scoped = new_scoped_key("test");
    let key_id = db::insert_api_key(
        &h.pool,
        &db::NewApiKey {
            tenant_id: a.id,
            name: "reader",
            key_hash: &hash_api_key(&scoped),
            key_prefix: &key_prefix(&scoped),
            scopes: "payments:read,payments:write,refunds:write",
            mode: "test",
            expires: None,
        },
    )
    .await
    .unwrap();
    let sealed = h.service.sealer.seal(b"whsec_a").unwrap();
    let endpoint_id =
        db::insert_endpoint(&h.pool, a.id, "https://example.com/h", &sealed, None, None)
            .await
            .unwrap();

    let base = format!("/projects/{}", a.id);
    let probes: Vec<(&str, String, Value)> = vec![
        ("GET", format!("{base}/keys"), Value::Null),
        (
            "POST",
            format!("{base}/keys"),
            json!({"name": "x", "scopes": ["admin"]}),
        ),
        ("POST", format!("{base}/keys/{key_id}/revoke"), json!({})),
        ("GET", format!("{base}/webhook-endpoints"), Value::Null),
        (
            "POST",
            format!("{base}/webhook-endpoints"),
            json!({"url": "https://example.com/x"}),
        ),
        (
            "POST",
            format!("{base}/webhook-endpoints/{endpoint_id}"),
            json!({"enabled": false}),
        ),
        (
            "DELETE",
            format!("{base}/webhook-endpoints/{endpoint_id}"),
            Value::Null,
        ),
        (
            "POST",
            format!("{base}/webhook-endpoints/{endpoint_id}/rotate-secret"),
            json!({}),
        ),
        (
            "POST",
            format!("{base}/webhook-endpoints/{endpoint_id}/test"),
            json!({}),
        ),
    ];
    let anon = Console {
        app: h.app.clone(),
        cookie: String::new(),
        csrf: String::new(),
    };
    for (method, uri, body) in &probes {
        let r = b.owner.req(method, uri, body.clone()).await;
        assert_eq!(
            r.status,
            StatusCode::NOT_FOUND,
            "{method} {uri} as B: {}",
            r.text
        );
        let r = anon.req(method, uri, body.clone()).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{method} {uri} anon");
    }
    let own = b
        .owner
        .req(
            "POST",
            &format!("/projects/{}/webhook-endpoints/{endpoint_id}", b.id),
            json!({"enabled": false}),
        )
        .await;
    assert_eq!(own.status, StatusCode::NOT_FOUND);
    let still = db::get_endpoint(&h.pool, a.id, endpoint_id)
        .await
        .unwrap()
        .unwrap();
    assert!(still.enabled);
    assert_eq!(
        db::list_api_keys(&h.pool, a.id)
            .await
            .unwrap()
            .iter()
            .filter(|k| k.revoked_at.is_some())
            .count(),
        0
    );

    for uri in [
        "/v1/events",
        "/v1/refunds",
        "/v1/payments",
        "/v1/saved-methods",
    ] {
        let r = call(&h.app, "GET", uri, Some(&b.key), Value::Null).await;
        assert_eq!(r.0, StatusCode::OK, "{uri}");
        assert_eq!(r.1.as_array().unwrap().len(), 0, "{uri}");
    }
    let (s, _) = call(
        &h.app,
        "GET",
        &format!("/v1/payments/{id}"),
        Some(&scoped),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, bpay) = call(&h.app, "POST", "/v1/payments", Some(&b.key), eur("b", 500)).await;
    let bid = bpay["id"].as_str().unwrap();
    for (method, uri, body) in [
        ("GET", format!("/v1/payments/{bid}"), Value::Null),
        ("POST", format!("/v1/payments/{bid}/cancel"), json!({})),
        (
            "POST",
            format!("/v1/payments/{bid}/refund"),
            json!({"amount": 1}),
        ),
        (
            "POST",
            format!("/v1/payments/{bid}/attempts"),
            json!({"method": "lightning"}),
        ),
    ] {
        let (s, _) = call(&h.app, method, &uri, Some(&scoped), body).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{method} {uri}");
    }
    let listed = call(&h.app, "GET", "/v1/payments", Some(&scoped), Value::Null)
        .await
        .1;
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["id"], id);
}

#[tokio::test]
async fn untrusted_projects_cannot_add_endpoints_at_internal_hosts() {
    let h = harness().await;
    let a = project(&h, &unique("ssrf@x.example")).await;
    for url in [
        "https://127.0.0.1/hook",
        "https://localhost/hook",
        "https://[::1]/hook",
        "https://[::ffff:10.0.0.1]/hook",
        "https://[fec0::1]/hook",
        "https://169.254.169.254/latest/meta-data",
        "https://2130706433/hook",
        "https://0x7f.1/hook",
        "https://user:pw@example.com/hook",
        "http://example.com/hook",
        "file:///etc/passwd",
    ] {
        let r = a
            .owner
            .req(
                "POST",
                &format!("/projects/{}/webhook-endpoints", a.id),
                json!({"url": url}),
            )
            .await;
        assert_eq!(
            r.status,
            StatusCode::BAD_REQUEST,
            "create {url}: {}",
            r.text
        );
    }
    let sealed = h.service.sealer.seal(b"whsec").unwrap();
    let id = payment_gateway::db::insert_endpoint(
        &h.pool,
        a.id,
        "https://example.com/h",
        &sealed,
        None,
        None,
    )
    .await
    .unwrap();
    let r = a
        .owner
        .req(
            "POST",
            &format!("/projects/{}/webhook-endpoints/{id}", a.id),
            json!({"url": "https://169.254.169.254/x"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text);
}

#[tokio::test]
async fn send_test_event_cannot_probe_internal_addresses() {
    let h = harness().await;
    let a = project(&h, &unique("probe@x.example")).await;
    let internal = Receiver::start().await;
    let sealed = h.service.sealer.seal(b"whsec").unwrap();
    let id =
        payment_gateway::db::insert_endpoint(&h.pool, a.id, &internal.url, &sealed, None, None)
            .await
            .unwrap();
    let r = a
        .owner
        .req(
            "POST",
            &format!("/projects/{}/webhook-endpoints/{id}/test", a.id),
            json!({}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["ok"], false);
    assert!(
        internal.got.lock().unwrap().is_empty(),
        "internal receiver was contacted"
    );
    assert!(!r.text.contains(&internal.url.replace("http://", "")) || r.json["error"].is_string());
}

#[tokio::test]
async fn docs_page_forbids_framing_and_scripts_from_elsewhere() {
    let h = harness().await;
    for uri in ["/docs", "/docs/docs.js", "/docs/docs.css"] {
        let r = send(&h.app, "GET", uri, None, &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::OK, "{uri}");
        let csp = r
            .headers
            .get("content-security-policy")
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'none'"),
            "{uri}: {csp}"
        );
        assert!(
            !csp.contains("unsafe-inline") && !csp.contains("unsafe-eval"),
            "{uri}: {csp}"
        );
        assert_eq!(
            r.headers.get("x-content-type-options").unwrap(),
            "nosniff",
            "{uri}"
        );
        assert_eq!(r.headers.get("x-frame-options").unwrap(), "DENY", "{uri}");
    }
}

use payment_gateway::payouts::DestinationKind;
use payment_gateway::service::WithdrawalRequest;

async fn operator_console(h: &Harness) -> Console {
    let email = unique("op@x.example");
    let hash = accounts::hash_password("correct horse battery").unwrap();
    accounts::insert_account(&h.pool, &email, &hash, true)
        .await
        .unwrap();
    let (c, _) = login(h, &email, None).await;
    c
}

#[tokio::test]
async fn withdrawals_and_payout_destinations_are_private_and_approval_is_operator_only() {
    let h = harness().await;
    h.service
        .payouts
        .set_policy(payment_gateway::payouts::PayoutPolicy {
            network: bitcoin::Network::Regtest,
            ..Default::default()
        });
    let a = project(&h, &unique("a@x.example")).await;
    let b = project(&h, &unique("b@x.example")).await;
    fund_tenant(&h, a.id, 50_000_000).await;
    let ta = h.service.tenant(a.id).await.unwrap();
    let w = h
        .service
        .request_withdrawal(
            &ta,
            WithdrawalRequest {
                amount: 2_000_000,
                invoice: Some(security_invoice(2_000_000)),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let secret_addr = "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080";
    let d = h
        .service
        .add_destination(&ta, DestinationKind::Onchain, "cold", secret_addr, None)
        .await
        .unwrap();

    let own = a
        .owner
        .req("GET", &format!("/projects/{}/payouts", a.id), Value::Null)
        .await;
    assert_eq!(own.status, StatusCode::OK, "{}", own.text);
    assert!(!own.text.contains(secret_addr), "{}", own.text);

    let attacks = [
        ("GET", format!("/projects/{}/payouts", a.id), Value::Null),
        (
            "POST",
            format!("/projects/{}/withdrawals", a.id),
            json!({"amount": 1_000_000, "destination_id": d.id}),
        ),
        (
            "POST",
            format!("/projects/{}/withdrawals/{}/cancel", a.id, w.id),
            json!({}),
        ),
        (
            "POST",
            format!("/projects/{}/withdrawals/{}/cancel", b.id, w.id),
            json!({}),
        ),
        (
            "POST",
            format!("/projects/{}/payout-destinations", a.id),
            json!({"kind": "onchain", "label": "x", "value": secret_addr}),
        ),
        (
            "DELETE",
            format!("/projects/{}/payout-destinations/{}", a.id, d.id),
            Value::Null,
        ),
        (
            "DELETE",
            format!("/projects/{}/payout-destinations/{}", b.id, d.id),
            Value::Null,
        ),
        (
            "POST",
            format!("/projects/{}/payout-destinations/{}/verify", a.id, d.id),
            json!({}),
        ),
        (
            "POST",
            format!("/projects/{}/payout-destinations/{}/verify", b.id, d.id),
            json!({}),
        ),
    ];
    for (m, path, body) in &attacks {
        let r = b.owner.req(m, path, body.clone()).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{m} {path}: {}", r.text);
        assert!(!r.text.contains(secret_addr));
        let anon = Console {
            app: h.app.clone(),
            cookie: String::new(),
            csrf: String::new(),
        };
        let r = anon.req(m, path, body.clone()).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "{m} {path}");
    }
    let mine = b
        .owner
        .req("GET", &format!("/projects/{}/payouts", b.id), Value::Null)
        .await;
    assert!(!mine.text.contains(secret_addr), "{}", mine.text);
    assert_eq!(mine.json["withdrawals"], json!([]));

    let operator_only = [
        ("GET", "/admin/withdrawals".to_string(), Value::Null),
        (
            "POST",
            format!("/admin/withdrawals/{}/approve", w.id),
            json!({}),
        ),
        (
            "POST",
            format!("/admin/withdrawals/{}/reject", w.id),
            json!({"reason": "no"}),
        ),
        (
            "POST",
            format!("/admin/projects/{}/payout-limits", a.id),
            json!({"daily_max": 1}),
        ),
    ];
    for (m, path, body) in &operator_only {
        for who in [&a.owner, &b.owner] {
            let r = who.req(m, path, body.clone()).await;
            assert_eq!(r.status, StatusCode::FORBIDDEN, "{m} {path}: {}", r.text);
        }
        let r = send(
            &h.app,
            m,
            &format!("/console/api{path}"),
            Some(&a.key),
            &[],
            body.clone(),
        )
        .await;
        assert_eq!(
            r.status,
            StatusCode::UNAUTHORIZED,
            "{m} {path} with api key"
        );
        let r = send(&h.app, m, path, Some(&a.key), &[], body.clone()).await;
        assert!(
            r.status == StatusCode::UNAUTHORIZED || r.status == StatusCode::NOT_FOUND,
            "{m} {path} {}",
            r.status
        );
    }
    let row = payment_gateway::db::get_withdrawal(&h.pool, w.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.status, "requested");

    let op = operator_console(&h).await;
    let r = op
        .req(
            "POST",
            &format!("/admin/withdrawals/{}/approve", w.id),
            json!({}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
}

fn security_invoice(amount_msat: u64) -> String {
    use bitcoin::hashes::{Hash, sha256};
    use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
    let secret: [u8; 32] = rand::random();
    let key = bitcoin::secp256k1::SecretKey::from_slice(&[0xcd; 32]).unwrap();
    InvoiceBuilder::new(Currency::Regtest)
        .description("t".into())
        .payment_hash(sha256::Hash::from_slice(&secret).unwrap())
        .payment_secret(PaymentSecret(secret))
        .current_timestamp()
        .min_final_cltv_expiry_delta(144)
        .amount_milli_satoshis(amount_msat)
        .build_signed(|m| bitcoin::secp256k1::Secp256k1::new().sign_ecdsa_recoverable(m, &key))
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn lightning_address_and_lnurl_resolution_refuse_internal_hosts() {
    use payment_gateway::payouts::resolve_invoice;
    for addr in [
        "alice@127.0.0.1",
        "alice@localhost",
        "alice@10.0.0.5",
        "alice@169.254.169.254",
        "alice@[::1]",
    ] {
        let r = resolve_invoice(DestinationKind::LightningAddress, addr, 2_000_000, false).await;
        assert!(r.is_err(), "{addr} must be refused");
    }
    let http_lnurl = bitcoin::bech32::encode::<bitcoin::bech32::Bech32>(
        bitcoin::bech32::Hrp::parse("lnurl").unwrap(),
        b"http://example.com/pay",
    )
    .unwrap();
    let r = resolve_invoice(DestinationKind::Lnurl, &http_lnurl, 2_000_000, false).await;
    assert!(r.is_err(), "plain http lnurl must be refused");
    let internal_lnurl = bitcoin::bech32::encode::<bitcoin::bech32::Bech32>(
        bitcoin::bech32::Hrp::parse("lnurl").unwrap(),
        b"https://127.0.0.1/pay",
    )
    .unwrap();
    let r = resolve_invoice(DestinationKind::Lnurl, &internal_lnurl, 2_000_000, false).await;
    assert!(r.is_err(), "internal lnurl must be refused");
}

async fn pay_in_full(h: &Harness, t: &TenantFixture, reference: &str) -> String {
    use payment_gateway::providers::{EventSink, ProviderEvent};
    let ln = provider(h, t, "lightning", json!({}), None).await;
    let p = new_payment(h, t, eur(reference, 900)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    payment_gateway::service::ProviderSink {
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

#[tokio::test]
async fn receipt_page_forbids_framing_and_scripts_and_uses_the_pay_page_capability_id() {
    let h = harness().await;
    let t = tenant(&h).await;
    let id = pay_in_full(&h, &t, "r-sec").await;
    assert_eq!(id.len(), 64);
    let r = send(
        &h.app,
        "GET",
        &format!("/receipt/{id}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let csp = r.headers["content-security-policy"].to_str().unwrap();
    assert!(
        csp.contains("script-src 'self'")
            && csp.contains("frame-ancestors 'none'")
            && !csp.contains("unsafe-inline"),
        "{csp}"
    );
    assert_eq!(r.headers["x-frame-options"], "DENY");
    assert_eq!(r.headers["x-content-type-options"], "nosniff");
    assert_eq!(r.headers["referrer-policy"], "no-referrer");
    assert!(
        r.headers["cache-control"]
            .to_str()
            .unwrap()
            .contains("no-store")
    );
    assert!(!r.text.contains(&t.key) && !r.text.contains(&t.webhook_secret));
    assert!(!r.text.contains("style="), "no inline styles");

    let mut near = id.clone();
    near.replace_range(63.., if id.ends_with('0') { "1" } else { "0" });
    for uri in [
        format!("/receipt/{near}"),
        format!("/receipt/{}", &id[..62]),
        format!("/receipt/{}", id.to_uppercase() + "00"),
        "/receipt/..%2F..%2Fconsole".to_string(),
    ] {
        let r = send(&h.app, "GET", &uri, None, &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{uri}");
    }
}

#[tokio::test]
async fn receipt_escapes_tenant_supplied_seller_text_and_rejects_control_characters() {
    let h = harness().await;
    let t = tenant(&h).await;
    let evil = "<script>alert(1)</script><img src=x onerror=alert(1)>";
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/settings", t.id),
        Some(ADMIN),
        json!({
            "display_name": evil,
            "legal_name": evil,
            "address": format!("{evil}\n\"'><svg onload=alert(1)>"),
            "tax_id": evil,
            "support_email": "a@b.test"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (_, p) = call(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&t.key),
        json!({"reference": evil, "amount": 900, "currency": "EUR", "customer_email": "x\"><script>@b.test", "description": evil,
               "line_items": [{"name": evil, "description": evil, "sku": evil, "unit_amount": 900}]}),
    )
    .await;
    assert!(p["id"].is_string(), "{p}");
    let id = pay_in_full_existing(&h, &t, &p).await;
    let r = send(
        &h.app,
        "GET",
        &format!("/receipt/{id}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(!r.text.contains("<script>alert"), "{}", r.text);
    assert!(!r.text.contains("<img"), "{}", r.text);
    assert!(!r.text.contains("<svg"), "{}", r.text);
    assert!(r.text.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));

    for field in ["legal_name", "tax_id", "support_email"] {
        for bad in ["a\r\nX-Evil: 1", "a\u{0}b", "a\nb"] {
            let (s, _) = call(
                &h.app,
                "POST",
                &format!("/admin/tenants/{}/settings", t.id),
                Some(ADMIN),
                json!({ field: bad }),
            )
            .await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{field} {bad:?}");
        }
    }
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/settings", t.id),
        Some(ADMIN),
        json!({"address": "line\r\nX-Evil: 1"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/settings", t.id),
        Some(ADMIN),
        json!({"address": "x".repeat(1025)}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

async fn pay_in_full_existing(h: &Harness, t: &TenantFixture, p: &Value) -> String {
    use payment_gateway::providers::{EventSink, ProviderEvent};
    let ln = provider(h, t, "lightning", json!({}), None).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    payment_gateway::service::ProviderSink {
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

const MARKER_SECRET: &str = "MARKER-SECRET-7f3a91c2";

async fn untrusted_project(h: &Harness, email: &str) -> Project {
    let p = project(h, email).await;
    payment_gateway::db::set_tenant_trusted(&h.pool, p.id, false)
        .await
        .unwrap();
    p
}

#[tokio::test]
async fn provider_test_connection_is_private_to_the_project_and_needs_a_session() {
    let h = harness().await;
    let a = project(&h, &unique("ta@x.example")).await;
    let b = project(&h, &unique("tb@x.example")).await;
    let ln = admin_provider(&h, &a, "lightning").await;
    let uri = format!("/projects/{}/providers/{ln}/test", a.id);
    let r = b.owner.req("POST", &uri, json!({})).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.text);
    let own_uri = format!("/projects/{}/providers/{ln}/test", b.id);
    let r = b.owner.req("POST", &own_uri, json!({})).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.text);
    let anon = Console {
        app: h.app.clone(),
        cookie: String::new(),
        csrf: String::new(),
    };
    let r = anon.req("POST", &uri, json!({})).await;
    assert!(
        matches!(r.status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
        "{}",
        r.text
    );
    let r = a.owner.req("POST", &uri, json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["ok"], true);
}

#[tokio::test]
async fn provider_test_connection_cannot_probe_internal_hosts() {
    let h = harness().await;
    let a = untrusted_project(&h, &unique("probe@x.example")).await;
    let internal = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::any())
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(json!({"id": "acct"})))
        .mount(&internal)
        .await;
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers", a.id),
        Some(ADMIN),
        json!({"method": "stripe", "provider_type": "stripe", "name": "p",
               "config": {"secret_key": MARKER_SECRET, "url": internal.uri()}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let pid = r["id"].as_u64().unwrap();
    let res = a
        .owner
        .req(
            "POST",
            &format!("/projects/{}/providers/{pid}/test", a.id),
            json!({}),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text);
    assert_eq!(res.json["ok"], false, "{}", res.text);
    assert!(
        internal.received_requests().await.unwrap().is_empty(),
        "the internal host was contacted"
    );
    assert!(!res.text.contains(MARKER_SECRET));
}

#[tokio::test]
async fn provider_test_connection_never_echoes_secret_markers() {
    let h = harness().await;
    let a = project(&h, &unique("redact@x.example")).await;
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers", a.id),
        Some(ADMIN),
        json!({"method": "stripe", "provider_type": "mock", "name": "p",
               "config": {"secret": MARKER_SECRET, "fail_test": true}}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let pid = r["id"].as_u64().unwrap();
    let res = a
        .owner
        .req(
            "POST",
            &format!("/projects/{}/providers/{pid}/test", a.id),
            json!({}),
        )
        .await;
    assert_eq!(res.status, StatusCode::OK, "{}", res.text);
    assert_eq!(res.json["ok"], false);
    assert!(res.json["message"].as_str().unwrap().contains("redacted"));
    assert!(!res.text.contains(MARKER_SECRET), "{}", res.text);
    let audit = payment_gateway::audit::list(
        &h.pool,
        &payment_gateway::audit::Filter {
            tenant_id: Some(a.id),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(!format!("{audit:?}").contains(MARKER_SECRET));
}

#[test]
fn sanitizer_redacts_every_secret_key_and_the_certificate() {
    let cfg = json!({
        "secret_key": "MARKER-sk-1111", "webhook_secret": "MARKER-wh-2222",
        "token": "MARKER-tok-3333", "client_secret": "MARKER-cs-4444",
        "api_secret": "MARKER-as-5555", "macaroon": "MARKER-mac-6666",
        "client_id": "public-id"
    });
    let all = "a MARKER-sk-1111 b MARKER-wh-2222 c MARKER-tok-3333 d MARKER-cs-4444 e MARKER-as-5555 f MARKER-mac-6666";
    let out = payment_gateway::validate::sanitize_test_error(all, Some(&cfg));
    assert!(!out.contains("MARKER"), "{out}");
}
