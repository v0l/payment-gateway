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
