mod common;
use axum::Router;
use axum::http::StatusCode;
use common::*;
use payment_gateway::accounts;
use serde_json::{Value, json};

struct Client {
    app: Router,
    cookie: String,
    csrf: String,
}

impl Client {
    fn anon(app: &Router) -> Self {
        Self {
            app: app.clone(),
            cookie: String::new(),
            csrf: String::new(),
        }
    }

    async fn req(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let r = self.raw(method, path, body, true).await;
        (r.status, r.json)
    }

    async fn raw(&self, method: &str, path: &str, body: Value, with_csrf: bool) -> Reply {
        let mut headers: Vec<(&str, String)> = vec![];
        if !self.cookie.is_empty() {
            headers.push(("cookie", format!("pgw_session={}", self.cookie)));
        }
        if with_csrf && !self.csrf.is_empty() {
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

    async fn login(app: &Router, email: &str, password: &str) -> (Self, Reply) {
        let r = send(
            app,
            "POST",
            "/console/api/login",
            None,
            &[],
            json!({"email": email, "password": password}),
        )
        .await;
        let mut c = Self::anon(app);
        if let Some(set) = r.headers.get("set-cookie") {
            let v = set.to_str().unwrap();
            c.cookie = v
                .split(';')
                .next()
                .unwrap()
                .trim_start_matches("pgw_session=")
                .to_string();
        }
        c.csrf = r.json["csrf"].as_str().unwrap_or("").to_string();
        (c, r)
    }
}

async fn make_account(h: &Harness, email: &str, password: &str, operator: bool) -> u64 {
    let hash = accounts::hash_password(password).unwrap();
    accounts::insert_account(&h.pool, email, &hash, operator)
        .await
        .unwrap()
}

async fn signed_in(h: &Harness, email: &str, operator: bool) -> Client {
    make_account(h, email, "correct horse battery", operator).await;
    let (c, r) = Client::login(&h.app, email, "correct horse battery").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    c
}

async fn project(c: &Client, name: &str) -> (u64, String, String) {
    let (s, p) = c.req("POST", "/projects", json!({"name": name})).await;
    assert_eq!(s, StatusCode::OK, "{p}");
    (
        p["id"].as_u64().unwrap(),
        p["api_key"].as_str().unwrap().to_string(),
        p["webhook_secret"].as_str().unwrap().to_string(),
    )
}

#[tokio::test]
async fn console_shell_is_served_without_a_session_and_with_a_strict_csp() {
    let h = harness().await;
    for path in ["/console", "/console/invite/abc", "/console/anything"] {
        let r = send(&h.app, "GET", path, None, &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
        let csp = r.headers["content-security-policy"].to_str().unwrap();
        assert!(
            csp.contains("frame-ancestors 'none'")
                && csp.contains("script-src 'self'")
                && !csp.contains("unsafe"),
            "{csp}"
        );
        assert!(r.text.contains("/console/app.js"));
    }
    let js = send(&h.app, "GET", "/console/app.js", None, &[], Value::Null).await;
    assert_eq!(js.headers["content-type"], "text/javascript");
    assert_eq!(js.headers["cache-control"], "no-cache");
}

#[tokio::test]
async fn styleguide_and_design_tokens_are_served_and_csp_safe() {
    let h = harness().await;
    let page = send(&h.app, "GET", "/console/styleguide", None, &[], Value::Null).await;
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.text.contains("/console/styleguide.js"));
    assert!(!page.text.contains("style=") && !page.text.contains("<style"));
    for (path, ct) in [
        ("/console/tokens.css", "text/css"),
        ("/console/components.css", "text/css"),
        ("/console/styleguide.js", "text/javascript"),
    ] {
        let r = send(&h.app, "GET", path, None, &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
        assert_eq!(r.headers["content-type"], ct);
    }
    let tokens = send(&h.app, "GET", "/console/tokens.css", None, &[], Value::Null).await;
    assert!(tokens.text.contains("--accent") && tokens.text.contains("prefers-reduced-motion"));
    let js = include_str!("../web/styleguide.js");
    assert!(!js.contains("style:") && !js.contains("cssText"));
}

#[test]
fn console_and_pay_css_take_colours_from_tokens() {
    for css in [
        include_str!("../web/console.css"),
        include_str!("../web/pay.css"),
        include_str!("../web/components.css"),
    ] {
        let loose = css.lines().filter(|l| {
            let l = l.trim_start();
            !l.starts_with("--") && l.contains('#') && !l.contains("#fff")
        });
        assert_eq!(loose.count(), 0, "colour literals belong in tokens.css");
    }
}

#[test]
fn console_never_hands_null_to_replace_children_which_renders_the_text_null() {
    for (name, js) in [
        ("payments", include_str!("../web/console/payments.js")),
        ("providers", include_str!("../web/console/providers.js")),
    ] {
        for line in js.lines().filter(|l| l.contains("replaceChildren(")) {
            assert!(!line.contains(": null)"), "{name}: {line}");
        }
    }
}

#[test]
fn expired_and_cancelled_pills_are_neutral_and_failures_stay_red() {
    let css = include_str!("../web/console.css");
    let red = css
        .lines()
        .find(|l| l.contains("var(--bad)") && l.starts_with(".pill"))
        .unwrap();
    assert!(red.contains(".pill.failed") && red.contains(".pill.disputed"));
    assert!(!red.contains("expired") && !red.contains("cancelled"));
}

#[tokio::test]
async fn api_needs_a_session() {
    let h = harness().await;
    let c = Client::anon(&h.app);
    for (m, p) in [
        ("GET", "/me"),
        ("GET", "/projects"),
        ("POST", "/projects"),
        ("GET", "/admin/accounts"),
    ] {
        let (s, _) = c.req(m, p, json!({"name": "x"})).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED, "{p}");
    }
}

#[tokio::test]
async fn login_sets_a_hardened_cookie_and_rejects_wrong_passwords_the_same_way() {
    let h = harness().await;
    make_account(&h, "me@example.com", "correct horse battery", false).await;
    let (c, ok) = Client::login(&h.app, "  ME@Example.com ", "correct horse battery").await;
    assert_eq!(ok.status, StatusCode::OK);
    let cookie = ok.headers["set-cookie"].to_str().unwrap();
    for flag in ["HttpOnly", "SameSite=Strict", "Path=/console"] {
        assert!(cookie.contains(flag), "{cookie}");
    }
    let (s, me) = c.req("GET", "/me", Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(me["email"], "me@example.com");

    let (_, wrong) = Client::login(&h.app, "me@example.com", "nope nope nope").await;
    let (_, unknown) = Client::login(&h.app, "who@example.com", "nope nope nope").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.json, unknown.json, "no account enumeration");
    assert!(wrong.headers.get("set-cookie").is_none());
}

#[tokio::test]
async fn repeated_failures_lock_the_login_and_success_does_not_bypass_it() {
    let h = harness().await;
    make_account(&h, "me@example.com", "correct horse battery", false).await;
    for _ in 0..8 {
        Client::login(&h.app, "me@example.com", "wrong wrong wrong").await;
    }
    let (_, r) = Client::login(&h.app, "me@example.com", "correct horse battery").await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn state_changing_calls_need_the_csrf_token() {
    let h = harness().await;
    let c = signed_in(&h, "me@example.com", false).await;
    let r = c
        .raw("POST", "/projects", json!({"name": "x"}), false)
        .await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let mut wrong = Client::anon(&h.app);
    wrong.cookie = c.cookie.clone();
    wrong.csrf = "0".repeat(32);
    let (s, _) = wrong.req("POST", "/projects", json!({"name": "x"})).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = c.req("POST", "/projects", json!({"name": "x"})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = Client {
        csrf: String::new(),
        ..Client {
            app: h.app.clone(),
            cookie: c.cookie.clone(),
            csrf: String::new(),
        }
    }
    .req("GET", "/me", Value::Null)
    .await;
    assert_eq!(s, StatusCode::OK, "reads need no csrf");
}

#[tokio::test]
async fn logout_and_disable_kill_the_session() {
    let h = harness().await;
    let c = signed_in(&h, "me@example.com", false).await;
    let (s, _) = c.req("POST", "/logout", json!({})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = c.req("GET", "/me", Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let victim = signed_in(&h, "victim@example.com", false).await;
    let op = signed_in(&h, "op@example.com", true).await;
    let (_, accounts_list) = op.req("GET", "/admin/accounts", Value::Null).await;
    let id = accounts_list
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["email"] == "victim@example.com")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    let (s, _) = op
        .req(
            "POST",
            &format!("/admin/accounts/{id}/disabled"),
            json!({"enabled": false}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = victim.req("GET", "/me", Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (_, r) = Client::login(&h.app, "victim@example.com", "correct horse battery").await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn expired_sessions_are_rejected() {
    let h = harness().await;
    let c = signed_in(&h, "me@example.com", false).await;
    sqlx::query("update account_session set expires = ?")
        .bind(chrono::Utc::now() - chrono::Duration::seconds(5))
        .execute(&h.pool)
        .await
        .unwrap();
    let (s, _) = c.req("GET", "/me", Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn password_change_needs_the_current_password_and_signs_out_other_sessions() {
    let h = harness().await;
    make_account(&h, "me@example.com", "correct horse battery", false).await;
    let (a, _) = Client::login(&h.app, "me@example.com", "correct horse battery").await;
    let (b, _) = Client::login(&h.app, "me@example.com", "correct horse battery").await;
    let (s, _) = a
        .req(
            "POST",
            "/password",
            json!({"current": "wrong wrong", "new": "brand new password"}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = a
        .req(
            "POST",
            "/password",
            json!({"current": "correct horse battery", "new": "short"}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = a
        .req(
            "POST",
            "/password",
            json!({"current": "correct horse battery", "new": "brand new password"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(a.req("GET", "/me", Value::Null).await.0, StatusCode::OK);
    assert_eq!(
        b.req("GET", "/me", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (_, r) = Client::login(&h.app, "me@example.com", "brand new password").await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn invite_flow_creates_an_account_once_and_expires() {
    let h = harness().await;
    let (s, inv) = call(
        &h.app,
        "POST",
        "/admin/invites",
        Some(ADMIN),
        json!({"email": "New@Example.com"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{inv}");
    let url = inv["invite_url"].as_str().unwrap();
    let token = url.rsplit('/').next().unwrap();
    assert_eq!(inv["email"], "new@example.com");

    let anon = Client::anon(&h.app);
    let (s, info) = anon
        .req("GET", &format!("/invite/{token}"), Value::Null)
        .await;
    assert_eq!(
        (s, info["email"].as_str()),
        (StatusCode::OK, Some("new@example.com"))
    );
    let (s, _) = anon
        .req(
            "POST",
            "/signup",
            json!({"token": token, "password": "short"}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let r = send(
        &h.app,
        "POST",
        "/console/api/signup",
        None,
        &[],
        json!({"token": token, "password": "a good long password"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert!(r.headers.get("set-cookie").is_some());
    assert_eq!(r.json["is_operator"], false);

    let (s, _) = anon
        .req(
            "POST",
            "/signup",
            json!({"token": token, "password": "a good long password"}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "invites are single use");
    let (s, _) = anon
        .req("GET", &format!("/invite/{token}"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, dup) = call(
        &h.app,
        "POST",
        "/admin/invites",
        Some(ADMIN),
        json!({"email": "new@example.com"}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "{dup}");

    let (_, inv2) = call(
        &h.app,
        "POST",
        "/admin/invites",
        Some(ADMIN),
        json!({"email": "late@example.com"}),
    )
    .await;
    let t2 = inv2["invite_url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    sqlx::query("update invite set expires = ?")
        .bind(chrono::Utc::now() - chrono::Duration::seconds(5))
        .execute(&h.pool)
        .await
        .unwrap();
    let (s, _) = anon
        .req(
            "POST",
            "/signup",
            json!({"token": t2, "password": "a good long password"}),
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn invite_creation_needs_the_admin_token() {
    let h = harness().await;
    let (s, _) = call(
        &h.app,
        "POST",
        "/admin/invites",
        None,
        json!({"email": "a@b.co"}),
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let c = signed_in(&h, "cust@example.com", false).await;
    let (s, _) = c
        .req("POST", "/admin/invites", json!({"email": "a@b.co"}))
        .await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "customers cannot invite platform users"
    );
    let (s, _) = c.req("GET", "/admin/accounts", Value::Null).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn operator_invite_makes_an_operator_and_customers_cannot_self_promote() {
    let h = harness().await;
    let (_, inv) = call(
        &h.app,
        "POST",
        "/admin/invites",
        Some(ADMIN),
        json!({"email": "boss@example.com", "operator": true}),
    )
    .await;
    let token = inv["invite_url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    let r = send(
        &h.app,
        "POST",
        "/console/api/signup",
        None,
        &[],
        json!({"token": token, "password": "a good long password"}),
    )
    .await;
    assert_eq!(r.json["is_operator"], true);
}

#[tokio::test]
async fn projects_are_created_with_keys_that_work_on_the_public_api() {
    let h = harness().await;
    let c = signed_in(&h, "me@example.com", false).await;
    let (id, key, secret) = project(&c, "My shop").await;
    assert!(key.starts_with("pgw_") && secret.starts_with("whsec_"));
    let (s, _) = call(&h.app, "GET", "/v1/methods", Some(&key), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    let (_, projects) = c.req("GET", "/projects", Value::Null).await;
    assert_eq!(projects[0]["id"], id);
    assert_eq!(projects[0]["role"], "owner");
    let (_, p) = c.req("GET", &format!("/projects/{id}"), Value::Null).await;
    assert_eq!(p["trusted"], false, "customer projects start in test mode");
}

#[tokio::test]
async fn project_names_are_validated_and_the_per_account_limit_holds() {
    let h = harness().await;
    let c = signed_in(&h, "me@example.com", false).await;
    assert_eq!(
        c.req("POST", "/projects", json!({"name": "  "})).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        c.req("POST", "/projects", json!({"name": "x".repeat(65)}))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    for i in 0..5 {
        assert_eq!(
            c.req("POST", "/projects", json!({"name": format!("p{i}")}))
                .await
                .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        c.req("POST", "/projects", json!({"name": "p6"})).await.0,
        StatusCode::FORBIDDEN
    );
    let op = signed_in(&h, "op@example.com", true).await;
    for i in 0..6 {
        assert_eq!(
            op.req("POST", "/projects", json!({"name": format!("op{i}")}))
                .await
                .0,
            StatusCode::OK,
            "operators are not limited"
        );
    }
}

#[tokio::test]
async fn accounts_cannot_see_or_touch_each_others_projects() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let (pid, key, _) = project(&a, "A").await;
    let provider = a.req("POST", &format!("/projects/{pid}/providers"), json!({"method": "stripe", "provider_type": "stripe", "name": "s", "config": {"secret_key": "sk_live_x"}})).await;
    assert_eq!(provider.0, StatusCode::OK, "{}", provider.1);
    let pay = call(&h.app, "POST", "/v1/payments", Some(&key), eur("o", 100))
        .await
        .1;
    let payment = pay["id"].as_str().unwrap();

    for (m, path, body) in [
        ("GET", format!("/projects/{pid}"), Value::Null),
        ("GET", format!("/projects/{pid}/payments"), Value::Null),
        (
            "GET",
            format!("/projects/{pid}/payments/{payment}"),
            Value::Null,
        ),
        (
            "POST",
            format!("/projects/{pid}/payments/{payment}/cancel"),
            json!({}),
        ),
        ("GET", format!("/projects/{pid}/providers"), Value::Null),
        ("GET", format!("/projects/{pid}/stats"), Value::Null),
        ("GET", format!("/projects/{pid}/deliveries"), Value::Null),
        ("GET", format!("/projects/{pid}/members"), Value::Null),
        ("POST", format!("/projects/{pid}/rotate-key"), json!({})),
        ("POST", format!("/projects/{pid}/settings"), json!({})),
    ] {
        let (s, _) = b.req(m, &path, body).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{m} {path}");
    }
    let (_, mine) = b.req("GET", "/projects", Value::Null).await;
    assert!(mine.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn roles_gate_what_a_teammate_can_do() {
    let h = harness().await;
    let owner = signed_in(&h, "owner@example.com", false).await;
    let (pid, _, _) = project(&owner, "P").await;
    let viewer = signed_in(&h, "viewer@example.com", false).await;
    let admin = signed_in(&h, "admin@example.com", false).await;
    let viewer_id = accounts::account_by_email(&h.pool, "viewer@example.com")
        .await
        .unwrap()
        .unwrap()
        .id;
    let admin_id = accounts::account_by_email(&h.pool, "admin@example.com")
        .await
        .unwrap()
        .unwrap()
        .id;
    accounts::add_member(&h.pool, pid, viewer_id, accounts::Role::Viewer)
        .await
        .unwrap();
    accounts::add_member(&h.pool, pid, admin_id, accounts::Role::Admin)
        .await
        .unwrap();

    assert_eq!(
        viewer
            .req("GET", &format!("/projects/{pid}/payments"), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    for (m, path, body) in [
        ("POST", format!("/projects/{pid}/settings"), json!({})),
        ("POST", format!("/projects/{pid}/rotate-key"), json!({})),
        ("POST", format!("/projects/{pid}/payments"), eur("x", 5)),
        (
            "POST",
            format!("/projects/{pid}/invites"),
            json!({"email": "n@e.co", "role": "viewer"}),
        ),
        (
            "POST",
            format!("/projects/{pid}/providers"),
            json!({"method": "stripe", "provider_type": "stripe", "name": "s", "config": {"secret_key": "k"}}),
        ),
    ] {
        assert_eq!(
            viewer.req(m, &path, body).await.0,
            StatusCode::FORBIDDEN,
            "viewer {path}"
        );
    }
    assert_eq!(
        admin
            .req("POST", &format!("/projects/{pid}/rotate-key"), json!({}))
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        admin
            .req(
                "POST",
                &format!("/projects/{pid}/rotate-webhook-secret"),
                json!({})
            )
            .await
            .0,
        StatusCode::FORBIDDEN,
        "owner only"
    );
    let (s, _) = admin
        .req(
            "POST",
            &format!("/projects/{pid}/invites"),
            json!({"email": "x@e.co", "role": "owner"}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "cannot grant above own role");
    let (s, _) = admin
        .req(
            "POST",
            &format!("/projects/{pid}/invites"),
            json!({"email": "x@e.co", "role": "viewer"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let owner_id = accounts::account_by_email(&h.pool, "owner@example.com")
        .await
        .unwrap()
        .unwrap()
        .id;
    let (s, _) = admin
        .req(
            "DELETE",
            &format!("/projects/{pid}/members/{owner_id}"),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "cannot remove an owner");
}

#[tokio::test]
async fn teammate_invites_join_the_project_and_the_last_owner_cannot_leave() {
    let h = harness().await;
    let owner = signed_in(&h, "owner@example.com", false).await;
    let (pid, _, _) = project(&owner, "P").await;
    let (_, inv) = owner
        .req(
            "POST",
            &format!("/projects/{pid}/invites"),
            json!({"email": "mate@example.com", "role": "admin"}),
        )
        .await;
    let token = inv["invite_url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string();
    let r = send(
        &h.app,
        "POST",
        "/console/api/signup",
        None,
        &[],
        json!({"token": token, "password": "a good long password"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["projects"][0]["role"], "admin");

    let (_, members) = owner
        .req("GET", &format!("/projects/{pid}/members"), Value::Null)
        .await;
    assert_eq!(members.as_array().unwrap().len(), 2);
    let owner_id = accounts::account_by_email(&h.pool, "owner@example.com")
        .await
        .unwrap()
        .unwrap()
        .id;
    let (s, e) = owner
        .req(
            "DELETE",
            &format!("/projects/{pid}/members/{owner_id}"),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT, "{e}");
}

#[tokio::test]
async fn provider_secrets_are_write_only_and_survive_edits() {
    let h = harness().await;
    let c = signed_in(&h, "me@example.com", false).await;
    let (pid, _, _) = project(&c, "P").await;
    let (s, created) = c
        .req(
            "POST",
            &format!("/projects/{pid}/providers"),
            json!({
                "method": "stripe", "provider_type": "stripe", "name": "main", "fee_rate": 1.5,
                "config": {"secret_key": "sk_live_topsecret", "webhook_secret": "whsec_topsecret"}
            }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{created}");
    assert!(
        created["hook_url"]
            .as_str()
            .unwrap()
            .contains(&format!("/hook/{pid}/"))
    );
    let provider = created["id"].as_u64().unwrap();

    let (_, list) = c
        .req("GET", &format!("/projects/{pid}/providers"), Value::Null)
        .await;
    assert!(
        !list.to_string().contains("topsecret"),
        "secrets never come back"
    );
    assert_eq!(list[0]["config"]["secret_key"], "set");

    let (s, upd) = c
        .req(
            "POST",
            &format!("/projects/{pid}/providers/{provider}"),
            json!({
                "method": "stripe", "provider_type": "stripe", "name": "renamed", "fee_rate": 2.0,
                "config": {"secret_key": "set", "webhook_secret": "whsec_newsecret"}
            }),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{upd}");
    assert_eq!(upd["name"], "renamed");
    assert!(!upd.to_string().contains("newsecret"));
    let tenant = payment_gateway::db::get_tenant(&h.pool, pid)
        .await
        .unwrap()
        .unwrap();
    let row = payment_gateway::db::get_tenant_provider(&h.pool, tenant.id, provider)
        .await
        .unwrap()
        .unwrap();
    let stored = h.service.registry.open_config(&row).unwrap();
    assert_eq!(
        stored["secret_key"], "sk_live_topsecret",
        "'set' keeps the old secret"
    );
    assert_eq!(stored["webhook_secret"], "whsec_newsecret");

    let (s, _) = c
        .req(
            "POST",
            &format!("/projects/{pid}/providers/{provider}"),
            json!({"method": "revolut", "provider_type": "revolut", "name": "x", "config": {}}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "type cannot change");
}

#[tokio::test]
async fn customers_cannot_point_providers_or_webhooks_at_internal_addresses_or_files() {
    let h = harness().await;
    let c = signed_in(&h, "me@example.com", false).await;
    let (pid, _, _) = project(&c, "P").await;
    let cert = "-----BEGIN CERTIFICATE-----\nx\n-----END CERTIFICATE-----";
    for cfg in [
        json!({"url": "https://10.245.31.103:10009", "cert": cert, "macaroon": "0a"}),
        json!({"url": "https://lnd-gateway:10009", "cert": cert, "macaroon": "0a"}),
        json!({"url": "https://localhost:10009", "cert": cert, "macaroon": "0a"}),
        json!({"url": "https://1.1.1.1:10009", "cert_path": "/etc/passwd", "macaroon": "0a", "cert": cert}),
        json!({"url": "https://1.1.1.1:10009", "cert": cert, "macaroon_path": "/lnd/admin.macaroon"}),
    ] {
        let (s, r) = c.req("POST", &format!("/projects/{pid}/providers"), json!({"method": "lightning", "provider_type": "lnd", "name": "n", "config": cfg.clone()})).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{cfg} -> {r}");
    }
    let (s, _) = c
        .req(
            "POST",
            &format!("/projects/{pid}/providers"),
            json!({"method": "stripe", "provider_type": "mock", "name": "n", "config": {}}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "no mocks for customers");
    let (s, _) = c.req("POST", &format!("/projects/{pid}/providers"), json!({"method": "stripe", "provider_type": "stripe", "name": "n", "config": {"secret_key": "k", "url": "http://10.0.0.5"}})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "no custom stripe url");

    for url in [
        "http://example.com/hook",
        "https://127.0.0.1/hook",
        "https://10.245.31.103/hook",
        "https://169.254.169.254/x",
        "https://localhost/hook",
    ] {
        let (s, r) = c
            .req(
                "POST",
                &format!("/projects/{pid}/settings"),
                json!({"webhook_url": url}),
            )
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{url} -> {r}");
    }
}

#[tokio::test]
async fn operators_may_use_internal_providers_and_lift_the_test_limits() {
    let h = harness().await;
    let op = signed_in(&h, "op@example.com", true).await;
    let (pid, _, _) = project(&op, "Internal").await;
    let (s, r) = op
        .req(
            "POST",
            &format!("/admin/projects/{pid}/trusted"),
            json!({"trusted": true}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (s, r) = op.req("POST", &format!("/projects/{pid}/providers"), json!({"method": "lightning", "provider_type": "lnd", "name": "n",
        "config": {"url": "https://lnd.bitcoin:10009", "cert_path": "/lnd/tls.cert", "macaroon_path": "/lnd/admin.macaroon"}})).await;
    assert_eq!(s, StatusCode::OK, "{r}");

    let cust = signed_in(&h, "cust@example.com", false).await;
    let (cpid, _, _) = project(&cust, "C").await;
    let (s, _) = cust
        .req(
            "POST",
            &format!("/admin/projects/{cpid}/trusted"),
            json!({"trusted": true}),
        )
        .await;
    assert_eq!(
        s,
        StatusCode::FORBIDDEN,
        "customers cannot lift their own limits"
    );
}

#[tokio::test]
async fn untrusted_webhook_delivery_refuses_internal_targets() {
    let h = harness().await;
    let c = signed_in(&h, "me@example.com", false).await;
    let (pid, key, _) = project(&c, "P").await;
    sqlx::query("update tenant set webhook_url = 'http://127.0.0.1:9/hook' where id = ?")
        .bind(pid)
        .execute(&h.pool)
        .await
        .unwrap();
    let st = c.req("POST", &format!("/projects/{pid}/providers"), json!({"method": "stripe", "provider_type": "stripe", "name": "s", "config": {"secret_key": "k"}})).await;
    assert_eq!(st.0, StatusCode::OK);
    let tenant = payment_gateway::db::get_tenant(&h.pool, pid)
        .await
        .unwrap()
        .unwrap();
    let payment = call(&h.app, "POST", "/v1/payments", Some(&key), eur("o", 100))
        .await
        .1;
    let id = pid_of(&payment);
    h.service.cancel_payment(&tenant, &id).await.unwrap();
    assert_eq!(h.service.deliver_due().await.unwrap(), 0);
    let rows = payment_gateway::db::list_deliveries(&h.pool, Some(pid), true)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0]
            .last_error
            .as_deref()
            .unwrap_or("")
            .contains("non-public"),
        "{:?}",
        rows[0].last_error
    );
}

fn pid_of(p: &Value) -> [u8; 32] {
    payment_gateway::db::parse_payment_id(p["id"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn payments_list_filters_pages_and_never_leaks_other_projects() {
    let h = harness().await;
    let c = signed_in(&h, "me@example.com", false).await;
    let (pid, key, _) = project(&c, "P").await;
    let (_, other_key, _) = project(&c, "Q").await;
    payment_gateway::db::set_tenant_trusted(&h.pool, pid, true)
        .await
        .unwrap();
    for i in 0..53 {
        call(
            &h.app,
            "POST",
            "/v1/payments",
            Some(&key),
            eur(&format!("order-{i:03}"), 100 + i),
        )
        .await;
    }
    call(
        &h.app,
        "POST",
        "/v1/payments",
        Some(&other_key),
        eur("order-other", 1),
    )
    .await;

    let (_, first) = c
        .req("GET", &format!("/projects/{pid}/payments"), Value::Null)
        .await;
    assert_eq!(first["payments"].as_array().unwrap().len(), 50);
    let next = first["next"].as_str().unwrap();
    let (_, second) = c
        .req(
            "GET",
            &format!("/projects/{pid}/payments?before={next}"),
            Value::Null,
        )
        .await;
    assert_eq!(second["payments"].as_array().unwrap().len(), 3);
    assert!(second["next"].is_null());
    let seen: std::collections::HashSet<String> = first["payments"]
        .as_array()
        .unwrap()
        .iter()
        .chain(second["payments"].as_array().unwrap())
        .map(|p| p["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(seen.len(), 53, "no duplicates or gaps across pages");
    assert!(!first.to_string().contains("order-other"));

    let (_, found) = c
        .req(
            "GET",
            &format!("/projects/{pid}/payments?q=order-007"),
            Value::Null,
        )
        .await;
    assert_eq!(found["payments"].as_array().unwrap().len(), 1);
    let (_, none) = c
        .req(
            "GET",
            &format!("/projects/{pid}/payments?q=%25"),
            Value::Null,
        )
        .await;
    assert!(
        none["payments"].as_array().unwrap().is_empty(),
        "wildcards are escaped"
    );
    let (_, pending) = c
        .req(
            "GET",
            &format!("/projects/{pid}/payments?status=paid"),
            Value::Null,
        )
        .await;
    assert!(pending["payments"].as_array().unwrap().is_empty());
    assert_eq!(
        c.req(
            "GET",
            &format!("/projects/{pid}/payments?status=bogus"),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        c.req(
            "GET",
            &format!("/projects/{pid}/payments?before=yesterday"),
            Value::Null
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn payment_detail_stats_and_actions_work_from_the_console() {
    let h = harness().await;
    let op = signed_in(&h, "op@example.com", true).await;
    let (pid, key, _) = project(&op, "P").await;
    op.req(
        "POST",
        &format!("/admin/projects/{pid}/trusted"),
        json!({"trusted": true}),
    )
    .await;
    let (s, prov) = op
        .req(
            "POST",
            &format!("/projects/{pid}/providers"),
            json!({"method": "lightning", "provider_type": "mock", "name": "ln", "config": {}}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{prov}");
    let (s, p) = op
        .req(
            "POST",
            &format!("/projects/{pid}/payments"),
            eur("from-console", 2500),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{p}");
    let id = p["id"].as_str().unwrap();
    let (s, p2) = op
        .req(
            "POST",
            &format!("/projects/{pid}/payments/{id}/attempts"),
            json!({"method": "lightning"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{p2}");
    let hash = p2["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    use payment_gateway::providers::{EventSink, ProviderEvent};
    payment_gateway::service::ProviderSink {
        service: h.service.clone(),
        provider_id: prov["id"].as_u64().unwrap(),
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

    let (_, d) = op
        .req(
            "GET",
            &format!("/projects/{pid}/payments/{id}"),
            Value::Null,
        )
        .await;
    assert_eq!(d["payment"]["status"], "paid");
    assert_eq!(d["receipts"].as_array().unwrap().len(), 1);
    assert_eq!(d["receipts"][0]["confirmed"], true);

    let (_, st) = op
        .req("GET", &format!("/projects/{pid}/stats?days=7"), Value::Null)
        .await;
    assert_eq!(st["by_status"][0]["status"], "paid");
    assert_eq!(st["by_status"][0]["total"], 2500);
    assert_eq!(st["paid_by_day"][0]["count"], 1);
    assert_eq!(st["by_method"][0]["method"], "lightning");

    let (s, r) = op
        .req(
            "POST",
            &format!("/projects/{pid}/payments/{id}/refund"),
            json!({"amount": 1000, "destination": "lnbc1x"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "completed");
    let (_, d) = op
        .req(
            "GET",
            &format!("/projects/{pid}/payments/{id}"),
            Value::Null,
        )
        .await;
    assert_eq!(d["payment"]["status"], "partially_refunded");
    let _ = key;
}

#[tokio::test]
async fn webhook_log_and_replay_are_scoped_to_the_project() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let (pa, key_a, _) = project(&a, "A").await;
    let (pb, _, _) = project(&b, "B").await;
    let hook = Receiver::start().await;
    sqlx::query("update tenant set webhook_url = ?, trusted = 1 where id = ?")
        .bind(&hook.url)
        .bind(pa)
        .execute(&h.pool)
        .await
        .unwrap();
    let tenant = payment_gateway::db::get_tenant(&h.pool, pa)
        .await
        .unwrap()
        .unwrap();
    let p = call(&h.app, "POST", "/v1/payments", Some(&key_a), eur("o", 100))
        .await
        .1;
    h.service
        .cancel_payment(&tenant, &pid_of(&p))
        .await
        .unwrap();
    h.service.deliver_due().await.unwrap();

    let (_, rows) = a
        .req("GET", &format!("/projects/{pa}/deliveries"), Value::Null)
        .await;
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["event_type"], "payment.cancelled");
    let delivery = rows[0]["id"].as_u64().unwrap();
    let (_, none) = b
        .req("GET", &format!("/projects/{pb}/deliveries"), Value::Null)
        .await;
    assert!(none.as_array().unwrap().is_empty());
    let (s, _) = b
        .req(
            "POST",
            &format!("/projects/{pb}/deliveries/{delivery}/replay"),
            json!({}),
        )
        .await;
    assert_eq!(
        s,
        StatusCode::NOT_FOUND,
        "cannot replay another project's delivery by guessing the id"
    );
    let (s, _) = a
        .req(
            "POST",
            &format!("/projects/{pa}/deliveries/{delivery}/replay"),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(h.service.deliver_due().await.unwrap(), 1);
}

#[tokio::test]
async fn rotating_the_key_from_the_console_revokes_the_old_one() {
    let h = harness().await;
    let c = signed_in(&h, "me@example.com", false).await;
    let (pid, old, _) = project(&c, "P").await;
    let (_, r) = c
        .req("POST", &format!("/projects/{pid}/rotate-key"), json!({}))
        .await;
    let new = r["api_key"].as_str().unwrap();
    assert_eq!(
        call(&h.app, "GET", "/v1/methods", Some(&old), Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(&h.app, "GET", "/v1/methods", Some(new), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn providers_with_history_cannot_be_deleted_but_unused_ones_can() {
    let h = harness().await;
    let op = signed_in(&h, "op@example.com", true).await;
    let (pid, key, _) = project(&op, "P").await;
    op.req(
        "POST",
        &format!("/admin/projects/{pid}/trusted"),
        json!({"trusted": true}),
    )
    .await;
    let (_, used) = op
        .req(
            "POST",
            &format!("/projects/{pid}/providers"),
            json!({"method": "lightning", "provider_type": "mock", "name": "used", "config": {}}),
        )
        .await;
    let (_, spare) = op
        .req(
            "POST",
            &format!("/projects/{pid}/providers"),
            json!({"method": "onchain", "provider_type": "mock", "name": "spare", "config": {}}),
        )
        .await;
    let p = call(&h.app, "POST", "/v1/payments", Some(&key), eur("o", 100))
        .await
        .1;
    call(
        &h.app,
        "POST",
        &format!("/v1/payments/{}/attempts", p["id"].as_str().unwrap()),
        Some(&key),
        json!({"method": "lightning"}),
    )
    .await;
    let (s, _) = op
        .req(
            "DELETE",
            &format!("/projects/{pid}/providers/{}", used["id"]),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = op
        .req(
            "DELETE",
            &format!("/projects/{pid}/providers/{}", spare["id"]),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
}

#[test]
fn signup_leaves_the_invite_url_so_the_used_invite_is_not_rechecked() {
    let auth = include_str!("../web/console/auth.js");
    let app = include_str!("../web/console/app.js");
    assert!(auth.contains(r#"location.replace("/console#/")"#));
    assert!(app.contains("if (m && !me)"));
}

#[tokio::test]
async fn every_console_script_is_served_and_loaded_by_the_shell() {
    let h = harness().await;
    let shell = send(&h.app, "GET", "/console", None, &[], Value::Null).await;
    for name in [
        "core",
        "auth",
        "projects",
        "payments",
        "providers",
        "webhooks",
        "team",
        "settings",
        "account",
        "admin",
        "activity",
        "app",
    ] {
        let path = format!("/console/{name}.js");
        assert!(shell.text.contains(&path), "{path}");
        let r = send(&h.app, "GET", &path, None, &[], Value::Null).await;
        assert_eq!(r.headers["content-type"], "text/javascript", "{path}");
        assert!(!r.text.is_empty(), "{path}");
    }
}

#[tokio::test]
async fn project_limits_show_usage_and_only_operators_can_edit_them() {
    let h = harness().await;
    let op = signed_in(&h, "op2@example.com", true).await;
    let cust = signed_in(&h, "cust2@example.com", false).await;
    let other = signed_in(&h, "other2@example.com", false).await;
    let (pid, key, _) = project(&cust, "Mine").await;

    let (s, v) = cust
        .req("GET", &format!("/projects/{pid}/limits"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["limits"]["pending_payments"], 50);
    assert_eq!(v["defaults"]["pending_payments"], 50);
    assert_eq!(v["usage"]["payments_per_day"], 0);

    let (s, _) = call(&h.app, "POST", "/v1/payments", Some(&key), eur("a", 100)).await;
    assert_eq!(s, StatusCode::CREATED);
    let (_, v) = cust
        .req("GET", &format!("/projects/{pid}/limits"), Value::Null)
        .await;
    assert_eq!(v["usage"]["payments_per_day"], 1);
    assert_eq!(v["usage"]["pending_payments"], 1);
    assert_eq!(v["usage"]["volume_per_day"], 100);

    let (s, _) = cust
        .req(
            "POST",
            &format!("/admin/projects/{pid}/limits"),
            json!({"pending_payments": 1000}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = other
        .req("GET", &format!("/projects/{pid}/limits"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, v) = op
        .req(
            "POST",
            &format!("/admin/projects/{pid}/limits"),
            json!({"pending_payments": 1}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["limits"]["pending_payments"], 1);
    assert_eq!(v["limits"]["providers"], 5);
    let (s, r) = call(&h.app, "POST", "/v1/payments", Some(&key), eur("b", 100)).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    assert_eq!(r["code"], "limit_exceeded");

    let (_, v) = op
        .req(
            "POST",
            &format!("/admin/projects/{pid}/limits"),
            json!({"pending_payments": null}),
        )
        .await;
    assert_eq!(v["limits"]["pending_payments"], 50);

    let rows: Vec<(String, Option<u64>, Option<String>)> = sqlx::query_as(
        "select action, tenant_id, details from audit_event where action = 'limits.changed' order by id",
    )
    .fetch_all(&h.pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].1, Some(pid));
    let details: Value = serde_json::from_str(rows[0].2.as_deref().unwrap()).unwrap();
    assert_eq!(details, json!({"changed": ["pending_payments"]}));
    assert!(!rows[0].2.as_deref().unwrap().contains("1000"));
}

#[tokio::test]
async fn untrusted_projects_get_one_webhook_endpoint_and_trusted_projects_more() {
    let h = harness().await;
    let c = signed_in(&h, "wh@example.com", false).await;
    let (pid, _, _) = project(&c, "W").await;
    let (_, v) = c
        .req("GET", &format!("/projects/{pid}/limits"), Value::Null)
        .await;
    assert_eq!(v["limits"]["webhook_endpoints"], 1);
    let (s, r) = c
        .req(
            "POST",
            &format!("/projects/{pid}/settings"),
            json!({"webhook_url": "https://example.com/hook"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
}
