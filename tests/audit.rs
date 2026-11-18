mod common;
use axum::Router;
use axum::http::StatusCode;
use common::*;
use payment_gateway::accounts;
use payment_gateway::audit::{self, AuditEvent, Filter};
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
        let mut headers: Vec<(&str, String)> = vec![("user-agent", "audit-test/1".into())];
        if !self.cookie.is_empty() {
            headers.push(("cookie", format!("pgw_session={}", self.cookie)));
        }
        if !self.csrf.is_empty() {
            headers.push(("x-csrf-token", self.csrf.clone()));
        }
        let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let r = send(
            &self.app,
            method,
            &format!("/console/api{path}"),
            None,
            &h,
            body,
        )
        .await;
        (r.status, r.json)
    }

    async fn login(app: &Router, email: &str, password: &str) -> (Self, Reply) {
        let r = send(
            app,
            "POST",
            "/console/api/login",
            None,
            &[
                ("x-forwarded-for", "203.0.113.9"),
                ("user-agent", "audit-test/1"),
            ],
            json!({"email": email, "password": password}),
        )
        .await;
        let mut c = Self::anon(app);
        if let Some(set) = r.headers.get("set-cookie") {
            c.cookie = set
                .to_str()
                .unwrap()
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

const PASSWORD: &str = "correct horse battery";

async fn make_account(h: &Harness, email: &str, operator: bool) -> u64 {
    let hash = accounts::hash_password(PASSWORD).unwrap();
    accounts::insert_account(&h.pool, email, &hash, operator)
        .await
        .unwrap()
}

async fn signed_in(h: &Harness, email: &str, operator: bool) -> (Client, u64) {
    let id = make_account(h, email, operator).await;
    let (c, r) = Client::login(&h.app, email, PASSWORD).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    (c, id)
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

async fn events(h: &Harness, f: Filter) -> Vec<AuditEvent> {
    audit::list(
        &h.pool,
        &Filter {
            limit: Some(200),
            ..f
        },
    )
    .await
    .unwrap()
}

async fn find(h: &Harness, action: &str) -> Vec<AuditEvent> {
    events(
        h,
        Filter {
            action: Some(action.into()),
            ..Default::default()
        },
    )
    .await
}

async fn one(h: &Harness, action: &str) -> AuditEvent {
    let mut found = find(h, action).await;
    assert_eq!(found.len(), 1, "{action}: {found:?}");
    found.remove(0)
}

fn token_of(invite: &Value) -> String {
    invite["invite_url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .to_string()
}

async fn signup(h: &Harness, token: &str) -> Reply {
    send(
        &h.app,
        "POST",
        "/console/api/signup",
        None,
        &[],
        json!({"token": token, "password": PASSWORD}),
    )
    .await
}

#[tokio::test]
async fn login_logout_and_password_changes_are_recorded_with_actor_ip_and_agent() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "me@example.com", false).await;
    let ok = one(&h, "login.ok").await;
    assert_eq!(ok.actor_kind, "account");
    assert_eq!(ok.actor_id.as_deref(), Some(id.to_string().as_str()));
    assert_eq!(ok.target_kind.as_deref(), Some("account"));
    assert_eq!(ok.ip.as_deref(), Some("203.0.113.9"));
    assert_eq!(ok.user_agent.as_deref(), Some("audit-test/1"));
    assert_eq!(ok.tenant_id, None);

    Client::login(&h.app, "me@example.com", "wrong wrong wrong").await;
    Client::login(&h.app, "ghost@example.com", "wrong wrong wrong").await;
    let failed = find(&h, "login.failed").await;
    assert_eq!(failed.len(), 2);
    let known = failed.iter().find(|e| e.actor_kind == "account").unwrap();
    assert_eq!(known.actor_id.as_deref(), Some(id.to_string().as_str()));
    let unknown = failed.iter().find(|e| e.actor_kind == "system").unwrap();
    assert_eq!(unknown.target_id.as_deref(), Some("ghost@example.com"));

    let (s, _) = c
        .req(
            "POST",
            "/password",
            json!({"current": PASSWORD, "new": "another long password"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let pw = one(&h, "password.changed").await;
    assert_eq!(pw.actor_id.as_deref(), Some(id.to_string().as_str()));

    c.req("POST", "/logout", json!({})).await;
    let out = one(&h, "logout").await;
    assert_eq!(out.actor_id.as_deref(), Some(id.to_string().as_str()));
}

#[tokio::test]
async fn invites_and_member_changes_are_recorded_against_the_project() {
    let h = harness().await;
    let (owner, owner_id) = signed_in(&h, "owner@example.com", false).await;
    let (pid, _, _) = project(&owner, "P").await;
    let (_, inv) = owner
        .req(
            "POST",
            &format!("/projects/{pid}/invites"),
            json!({"email": "mate@example.com", "role": "admin"}),
        )
        .await;
    let created = one(&h, "invite.created").await;
    assert_eq!(created.tenant_id, Some(pid));
    assert_eq!(
        created.actor_id.as_deref(),
        Some(owner_id.to_string().as_str())
    );
    assert_eq!(created.details["role"], "admin");

    let r = signup(&h, &token_of(&inv)).await;
    let mate = r.json["id"].as_u64().unwrap();
    let used = one(&h, "invite.used").await;
    assert_eq!(used.actor_id.as_deref(), Some(mate.to_string().as_str()));
    let added = one(&h, "member.added").await;
    assert_eq!(added.tenant_id, Some(pid));
    assert_eq!(added.target_id.as_deref(), Some(mate.to_string().as_str()));
    assert_eq!(added.details["role"], "admin");

    let (_, inv) = owner
        .req(
            "POST",
            &format!("/projects/{pid}/invites"),
            json!({"email": "mate@example.com", "role": "viewer"}),
        )
        .await;
    signup(&h, &token_of(&inv)).await;
    let changed = one(&h, "member.role_changed").await;
    assert_eq!(changed.details["previous_role"], "admin");
    assert_eq!(changed.details["role"], "viewer");

    let (s, _) = owner
        .req(
            "DELETE",
            &format!("/projects/{pid}/members/{mate}"),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let removed = one(&h, "member.removed").await;
    assert_eq!(removed.tenant_id, Some(pid));
    assert_eq!(
        removed.actor_id.as_deref(),
        Some(owner_id.to_string().as_str())
    );
    assert_eq!(removed.target_kind.as_deref(), Some("account"));
    assert_eq!(
        removed.target_id.as_deref(),
        Some(mate.to_string().as_str())
    );
}

#[tokio::test]
async fn project_keys_and_settings_are_recorded_without_values() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "me@example.com", false).await;
    let (pid, key, secret) = project(&c, "P").await;
    let created = one(&h, "project.created").await;
    assert_eq!(created.tenant_id, Some(pid));
    assert_eq!(created.target_id.as_deref(), Some(pid.to_string().as_str()));
    assert_eq!(created.actor_id.as_deref(), Some(id.to_string().as_str()));

    let (s, rotated) = c
        .req("POST", &format!("/projects/{pid}/rotate-key"), json!({}))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, rotated_secret) = c
        .req(
            "POST",
            &format!("/projects/{pid}/rotate-webhook-secret"),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(one(&h, "key.rotated").await.tenant_id, Some(pid));
    assert_eq!(one(&h, "webhook_secret.rotated").await.tenant_id, Some(pid));

    let marker = format!("https://93.184.216.34/{}", unique("hook"));
    let (s, r) = c
        .req(
            "POST",
            &format!("/projects/{pid}/settings"),
            json!({"webhook_url": marker, "display_name": "Shop"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let settings = one(&h, "settings.changed").await;
    assert_eq!(
        settings.target_id.as_deref(),
        Some(pid.to_string().as_str())
    );
    assert_eq!(
        settings.details["changed"],
        json!(["webhook_url", "display_name"])
    );

    let dump = serde_json::to_string(&events(&h, Filter::default()).await).unwrap();
    for secret in [
        key,
        secret,
        rotated["api_key"].as_str().unwrap().to_string(),
        rotated_secret["webhook_secret"]
            .as_str()
            .unwrap()
            .to_string(),
        marker,
    ] {
        assert!(!dump.contains(&secret), "{secret} leaked into audit");
    }
}

#[tokio::test]
async fn provider_lifecycle_is_recorded_with_field_names_only() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "me@example.com", false).await;
    let (pid, _, _) = project(&c, "P").await;
    let sk = unique("sk_live_");
    let wh = unique("whsec_");
    let sk2 = unique("sk_live_");
    let (s, created) = c
        .req(
            "POST",
            &format!("/projects/{pid}/providers"),
            json!({"method": "stripe", "provider_type": "stripe", "name": "main",
                "config": {"secret_key": sk, "webhook_secret": wh}}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{created}");
    let prov = created["id"].as_u64().unwrap();
    let ev = one(&h, "provider.created").await;
    assert_eq!(ev.tenant_id, Some(pid));
    assert_eq!(ev.actor_id.as_deref(), Some(id.to_string().as_str()));
    assert_eq!(ev.target_kind.as_deref(), Some("provider"));
    assert_eq!(ev.target_id.as_deref(), Some(prov.to_string().as_str()));

    let (s, r) = c
        .req(
            "POST",
            &format!("/projects/{pid}/providers/{prov}"),
            json!({"method": "stripe", "provider_type": "stripe", "name": "main", "fee_rate": 2.5,
                "config": {"secret_key": sk2}}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let upd = one(&h, "provider.updated").await;
    assert_eq!(
        upd.details["changed"],
        json!(["config.secret_key", "fee_rate"])
    );

    for enabled in [false, true] {
        c.req(
            "POST",
            &format!("/projects/{pid}/providers/{prov}/enabled"),
            json!({"enabled": enabled}),
        )
        .await;
    }
    assert_eq!(
        one(&h, "provider.disabled").await.target_id.as_deref(),
        Some(prov.to_string().as_str())
    );
    assert_eq!(find(&h, "provider.enabled").await.len(), 1);

    let (s, _) = c
        .req(
            "DELETE",
            &format!("/projects/{pid}/providers/{prov}"),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(one(&h, "provider.deleted").await.tenant_id, Some(pid));

    let dump = serde_json::to_string(&events(&h, Filter::default()).await).unwrap();
    for secret in [&sk, &wh, &sk2] {
        assert!(
            !dump.contains(secret.as_str()),
            "{secret} leaked into audit"
        );
    }
}

#[tokio::test]
async fn cancel_and_refund_from_the_console_are_recorded() {
    let h = harness().await;
    let (op, op_id) = signed_in(&h, "op@example.com", true).await;
    let (pid, _, _) = project(&op, "P").await;
    op.req(
        "POST",
        &format!("/admin/projects/{pid}/trusted"),
        json!({"trusted": true}),
    )
    .await;
    let trust = one(&h, "project.trust_changed").await;
    assert_eq!(trust.tenant_id, Some(pid));
    assert_eq!(trust.actor_id.as_deref(), Some(op_id.to_string().as_str()));
    assert_eq!(trust.details["trusted"], true);

    let (_, prov) = op
        .req(
            "POST",
            &format!("/projects/{pid}/providers"),
            json!({"method": "lightning", "provider_type": "mock", "name": "ln", "config": {}}),
        )
        .await;
    let (_, p) = op
        .req("POST", &format!("/projects/{pid}/payments"), eur("a", 2500))
        .await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, _) = op
        .req(
            "POST",
            &format!("/projects/{pid}/payments/{id}/cancel"),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let cancelled = one(&h, "payment.cancelled").await;
    assert_eq!(cancelled.tenant_id, Some(pid));
    assert_eq!(cancelled.target_kind.as_deref(), Some("payment"));
    assert_eq!(cancelled.target_id.as_deref(), Some(id.as_str()));

    let (_, p) = op
        .req("POST", &format!("/projects/{pid}/payments"), eur("b", 2500))
        .await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, p2) = op
        .req(
            "POST",
            &format!("/projects/{pid}/payments/{id}/attempts"),
            json!({"method": "lightning"}),
        )
        .await;
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
    let (s, r) = op
        .req(
            "POST",
            &format!("/projects/{pid}/payments/{id}/refund"),
            json!({"amount": 1000, "destination": "lnbc1x"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let requested = one(&h, "refund.requested").await;
    assert_eq!(requested.target_id.as_deref(), Some(id.as_str()));
    assert_eq!(requested.details["amount"], 1000);
    let result = one(&h, "refund.result").await;
    assert_eq!(result.details["status"], "completed");
    assert_eq!(result.tenant_id, Some(pid));

    let (s, _) = op
        .req(
            "POST",
            &format!("/projects/{pid}/payments/{id}/refund"),
            json!({"amount": 99999999, "destination": "lnbc1x"}),
        )
        .await;
    assert_ne!(s, StatusCode::OK);
    let results = find(&h, "refund.result").await;
    assert_eq!(results.len(), 2);
    assert!(results.iter().any(|e| e.details["status"] == "rejected"));
    let dump = serde_json::to_string(&results).unwrap();
    assert!(!dump.contains("lnbc1x"));
}

#[tokio::test]
async fn operator_actions_and_admin_token_calls_are_recorded() {
    let h = harness().await;
    let (op, op_id) = signed_in(&h, "op@example.com", true).await;
    let (cust, cust_id) = signed_in(&h, "cust@example.com", false).await;
    let _ = cust;

    let (s, _) = op
        .req(
            "POST",
            &format!("/admin/accounts/{cust_id}/disabled"),
            json!({"enabled": false}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let disabled = one(&h, "account.disabled").await;
    assert_eq!(
        disabled.actor_id.as_deref(),
        Some(op_id.to_string().as_str())
    );
    assert_eq!(
        disabled.target_id.as_deref(),
        Some(cust_id.to_string().as_str())
    );

    let (s, _) = op
        .req(
            "POST",
            "/admin/invites",
            json!({"email": "new@example.com", "operator": false}),
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    let invite = one(&h, "invite.created").await;
    assert_eq!(invite.tenant_id, None);
    assert_eq!(invite.target_id.as_deref(), Some("new@example.com"));

    let t = tenant(&h).await;
    let created = one(&h, "project.created").await;
    assert_eq!(created.actor_kind, "admin_token");
    assert_eq!(created.actor_id, None);
    assert_eq!(created.tenant_id, Some(t.id));

    let (s, rotated) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/rotate-key", t.id),
        Some(ADMIN),
        json!({}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let key_event = one(&h, "key.rotated").await;
    assert_eq!(key_event.actor_kind, "admin_token");
    assert_eq!(
        key_event.target_id.as_deref(),
        Some(t.id.to_string().as_str())
    );

    let marker = unique("sk_admin_");
    let prov = provider(&h, &t, "lightning", json!({"note": marker}), None).await;
    let pe = one(&h, "provider.created").await;
    assert_eq!(pe.actor_kind, "admin_token");
    assert_eq!(pe.target_id.as_deref(), Some(prov.to_string().as_str()));
    for enabled in [false, true] {
        call(
            &h.app,
            "POST",
            &format!("/admin/tenants/{}/providers/{prov}/enabled", t.id),
            Some(ADMIN),
            json!({"enabled": enabled}),
        )
        .await;
    }
    assert_eq!(find(&h, "provider.disabled").await.len(), 1);
    call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/enabled", t.id),
        Some(ADMIN),
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(
        one(&h, "project.enabled_changed").await.details["enabled"],
        false
    );
    call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/settings", t.id),
        Some(ADMIN),
        json!({"display_name": "Shop"}),
    )
    .await;
    assert_eq!(
        one(&h, "settings.changed").await.details["changed"],
        json!(["webhook_url", "display_name"])
    );

    let (s, list) = call(
        &h.app,
        "GET",
        "/admin/audit?action=provider",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{list}");
    assert_eq!(list.as_array().unwrap().len(), 3);
    let (s, _) = call(&h.app, "GET", "/admin/audit", Some("wrong"), Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(&h.app, "GET", "/admin/audit", None, Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let dump = serde_json::to_string(&events(&h, Filter::default()).await).unwrap();
    for secret in [
        t.key.clone(),
        t.webhook_secret.clone(),
        rotated["api_key"].as_str().unwrap().to_string(),
        marker,
    ] {
        assert!(!dump.contains(&secret));
    }
}

#[tokio::test]
async fn passwords_never_reach_the_audit_log() {
    let h = harness().await;
    let marker = unique("pw-marker-");
    let hash = accounts::hash_password(&marker).unwrap();
    accounts::insert_account(&h.pool, "me@example.com", &hash, false)
        .await
        .unwrap();
    let (c, _) = Client::login(&h.app, "me@example.com", &marker).await;
    let new_marker = unique("pw-new-");
    c.req(
        "POST",
        "/password",
        json!({"current": marker, "new": new_marker}),
    )
    .await;
    let wrong = unique("pw-wrong-");
    Client::login(&h.app, "me@example.com", &wrong).await;
    let raw: Vec<(Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as("select actor_id, target_id, details from audit_event")
            .fetch_all(&h.pool)
            .await
            .unwrap();
    assert!(raw.len() >= 3);
    let dump = format!("{raw:?}");
    for m in [marker, new_marker, wrong] {
        assert!(!dump.contains(&m));
    }
}

#[tokio::test]
async fn activity_is_scoped_to_the_project_and_readable_by_viewers_only_for_members() {
    let h = harness().await;
    let (a, _) = signed_in(&h, "a@example.com", false).await;
    let (b, _) = signed_in(&h, "b@example.com", false).await;
    let (pa, _, _) = project(&a, "A").await;
    let (pb, _, _) = project(&b, "B").await;
    a.req("POST", &format!("/projects/{pa}/rotate-key"), json!({}))
        .await;
    b.req("POST", &format!("/projects/{pb}/rotate-key"), json!({}))
        .await;

    let (s, mine) = a
        .req("GET", &format!("/projects/{pa}/activity"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::OK);
    let rows = mine.as_array().unwrap();
    assert!(rows.len() >= 2);
    assert!(rows.iter().all(|e| e["tenant_id"] == pa));
    assert!(rows.iter().all(|e| e["action"] != "login.ok"));

    let (s, _) = b
        .req("GET", &format!("/projects/{pa}/activity"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = b.req("GET", "/admin/audit", Value::Null).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (_, invite) = a
        .req(
            "POST",
            &format!("/projects/{pa}/invites"),
            json!({"email": "viewer@example.com", "role": "viewer"}),
        )
        .await;
    let r = signup(&h, &token_of(&invite)).await;
    let cookie = r.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("pgw_session=")
        .to_string();
    let viewer = Client {
        app: h.app.clone(),
        cookie,
        csrf: r.json["csrf"].as_str().unwrap().to_string(),
    };
    let (s, rows) = viewer
        .req("GET", &format!("/projects/{pa}/activity"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|e| e["tenant_id"] == pa)
    );
}

#[tokio::test]
async fn activity_filters_by_action_and_actor_and_pages() {
    let h = harness().await;
    let (owner, owner_id) = signed_in(&h, "owner@example.com", false).await;
    let (pid, _, _) = project(&owner, "P").await;
    for _ in 0..3 {
        owner
            .req("POST", &format!("/projects/{pid}/rotate-key"), json!({}))
            .await;
    }
    let base = format!("/projects/{pid}/activity");
    let (_, rows) = owner
        .req("GET", &format!("{base}?action=key.rotated"), Value::Null)
        .await;
    assert_eq!(rows.as_array().unwrap().len(), 3);
    let (_, rows) = owner
        .req(
            "GET",
            &format!("{base}?actor=owner@example.com"),
            Value::Null,
        )
        .await;
    assert_eq!(rows.as_array().unwrap().len(), 4);
    assert_eq!(rows[0]["actor_email"], "owner@example.com");
    let (_, rows) = owner
        .req(
            "GET",
            &format!("{base}?actor={}", owner_id + 100),
            Value::Null,
        )
        .await;
    assert!(rows.as_array().unwrap().is_empty());

    let (_, page1) = owner
        .req("GET", &format!("{base}?limit=3"), Value::Null)
        .await;
    assert_eq!(page1.as_array().unwrap().len(), 3);
    let last = page1[2]["id"].as_u64().unwrap();
    let (_, page2) = owner
        .req("GET", &format!("{base}?limit=3&before={last}"), Value::Null)
        .await;
    assert_eq!(page2.as_array().unwrap().len(), 1);
    assert_eq!(page2[0]["action"], "project.created");
}

#[tokio::test]
async fn a_failing_audit_write_does_not_fail_the_request() {
    let h = harness().await;
    let (c, _) = signed_in(&h, "me@example.com", false).await;
    sqlx::query("rename table audit_event to audit_event_gone")
        .execute(&h.pool)
        .await
        .unwrap();
    let (s, _) = c
        .req("POST", "/projects", json!({"name": "still works"}))
        .await;
    assert_eq!(s, StatusCode::OK);
}

#[test]
fn app_code_never_updates_or_deletes_audit_events() {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|x| x == "rs") {
                out.push(p);
            }
        }
    }
    let mut files = vec![];
    walk(std::path::Path::new("src"), &mut files);
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap().to_lowercase();
        for banned in [
            "update audit_event",
            "delete from audit_event",
            "truncate table audit_event",
            "drop table audit_event",
        ] {
            assert!(!text.contains(banned), "{} contains {banned}", f.display());
        }
    }
}
