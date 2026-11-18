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
async fn console_plans_and_subscriptions_are_role_gated_and_private() {
    let h = harness().await;
    let owner = signed_in(&h, "owner@example.com", false).await;
    let (pid, _, _) = project(&owner, "P").await;
    let viewer = signed_in(&h, "viewer@example.com", false).await;
    let other = signed_in(&h, "other@example.com", false).await;
    let viewer_id = accounts::account_by_email(&h.pool, "viewer@example.com")
        .await
        .unwrap()
        .unwrap()
        .id;
    accounts::add_member(&h.pool, pid, viewer_id, accounts::Role::Viewer)
        .await
        .unwrap();

    let (s, plan) = owner
        .req(
            "POST",
            &format!("/projects/{pid}/plans"),
            json!({"name": "Pro", "amount": 1500, "currency": "EUR"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{plan}");
    let plan_id = plan["id"].as_u64().unwrap();
    let (_, plans) = viewer
        .req("GET", &format!("/projects/{pid}/plans"), Value::Null)
        .await;
    assert_eq!(plans.as_array().unwrap().len(), 1);
    assert_eq!(
        viewer
            .req(
                "POST",
                &format!("/projects/{pid}/plans"),
                json!({"name": "x", "amount": 1, "currency": "EUR"})
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );

    let (s, sub) = owner
        .req(
            "POST",
            &format!("/projects/{pid}/subscriptions"),
            json!({"customer_ref": "c1", "plan_id": plan_id}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{sub}");
    assert_eq!(sub["status"], "incomplete");
    let sid = sub["id"].as_u64().unwrap();
    assert_eq!(
        viewer
            .req(
                "POST",
                &format!("/projects/{pid}/subscriptions/{sid}/cancel"),
                json!({})
            )
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (s, cancelled) = owner
        .req(
            "POST",
            &format!("/projects/{pid}/subscriptions/{sid}/cancel"),
            json!({}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{cancelled}");
    assert_eq!(cancelled["status"], "cancelled");
    let (_, detail) = viewer
        .req(
            "GET",
            &format!("/projects/{pid}/subscriptions/{sid}"),
            Value::Null,
        )
        .await;
    assert_eq!(detail["periods"].as_array().unwrap().len(), 1);

    for (m, path) in [
        ("GET", format!("/projects/{pid}/plans")),
        ("GET", format!("/projects/{pid}/subscriptions")),
        ("GET", format!("/projects/{pid}/subscriptions/{sid}")),
        ("POST", format!("/projects/{pid}/subscriptions/{sid}/pause")),
        ("POST", format!("/projects/{pid}/plans/{plan_id}/archive")),
    ] {
        assert_eq!(
            other.req(m, &path, json!({})).await.0,
            StatusCode::NOT_FOUND,
            "{m} {path}"
        );
    }
}
