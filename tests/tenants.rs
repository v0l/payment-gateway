use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use payment_gateway::api::{self, AppState};
use payment_gateway::crypto::Sealer;
use payment_gateway::db;
use serde_json::{Value, json};
use tower::ServiceExt;

const ADMIN: &str = "admin-token-admin-token-admin";

static MIGRATED: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

async fn app() -> Router {
    let url = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    MIGRATED
        .get_or_init(|| async {
            db::connect(&url).await.unwrap();
        })
        .await;
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    api::router(AppState {
        pool,
        sealer: Sealer::new(&[9u8; 32]),
        admin_token: ADMIN.into(),
    })
}

async fn call(app: &Router, method: &str, uri: &str, token: Option<&str>, body: Value) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn unique(prefix: &str) -> String {
    format!("{prefix}-{}", hex::encode(rand_bytes()))
}

fn rand_bytes() -> [u8; 6] {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut out = [0u8; 6];
    out.copy_from_slice(&n.to_be_bytes()[10..16]);
    out
}

#[tokio::test]
async fn admin_endpoints_reject_missing_or_wrong_token() {
    let app = app().await;
    let (s, _) = call(&app, "GET", "/admin/tenants", None, Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(&app, "GET", "/admin/tenants", Some("nope"), Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn tenant_key_authenticates_and_rotation_revokes_old_key() {
    let app = app().await;
    let (s, t) = call(
        &app,
        "POST",
        "/admin/tenants",
        Some(ADMIN),
        json!({"name": unique("yalr"), "webhook_url": "https://example.com/hook"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let key = t["api_key"].as_str().unwrap().to_string();
    let id = t["id"].as_u64().unwrap();

    let (s, _) = call(&app, "GET", "/v1/methods", Some(&key), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(&app, "GET", "/v1/methods", None, Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (s, r) = call(&app, "POST", &format!("/admin/tenants/{id}/rotate-key"), Some(ADMIN), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    let new_key = r["api_key"].as_str().unwrap();
    let (s, _) = call(&app, "GET", "/v1/methods", Some(&key), Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = call(&app, "GET", "/v1/methods", Some(new_key), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn disabled_tenant_is_rejected() {
    let app = app().await;
    let (_, t) = call(&app, "POST", "/admin/tenants", Some(ADMIN), json!({"name": unique("off")})).await;
    let key = t["api_key"].as_str().unwrap();
    let id = t["id"].as_u64().unwrap();
    call(&app, "POST", &format!("/admin/tenants/{id}/enabled"), Some(ADMIN), json!({"enabled": false})).await;
    let (s, _) = call(&app, "GET", "/v1/methods", Some(key), Value::Null).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn duplicate_tenant_name_conflicts_and_bad_webhook_url_rejected() {
    let app = app().await;
    let name = unique("dup");
    let (s, _) = call(&app, "POST", "/admin/tenants", Some(ADMIN), json!({"name": name})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(&app, "POST", "/admin/tenants", Some(ADMIN), json!({"name": name})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = call(
        &app,
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
    let app = app().await;
    let (_, a) = call(&app, "POST", "/admin/tenants", Some(ADMIN), json!({"name": unique("a")})).await;
    let (_, b) = call(&app, "POST", "/admin/tenants", Some(ADMIN), json!({"name": unique("b")})).await;
    let a_id = a["id"].as_u64().unwrap();
    let a_key = a["api_key"].as_str().unwrap();
    let b_key = b["api_key"].as_str().unwrap();

    let (s, p) = call(
        &app,
        "POST",
        &format!("/admin/tenants/{a_id}/providers"),
        Some(ADMIN),
        json!({
            "method": "stripe", "provider_type": "stripe", "name": "main",
            "config": {"secret_key": "sk_live_topsecret"}, "fee_rate": 1.5
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let pid = p["id"].as_u64().unwrap();

    let (_, ms) = call(&app, "GET", "/v1/methods", Some(a_key), Value::Null).await;
    assert_eq!(ms.as_array().unwrap().len(), 1);
    assert!(!ms.to_string().contains("topsecret"));
    let (_, ms) = call(&app, "GET", "/v1/methods", Some(b_key), Value::Null).await;
    assert!(ms.as_array().unwrap().is_empty());

    let (_, list) = call(&app, "GET", &format!("/admin/tenants/{a_id}/providers"), Some(ADMIN), Value::Null).await;
    assert!(!list.to_string().contains("topsecret"));

    call(&app, "POST", &format!("/admin/tenants/{a_id}/providers/{pid}/enabled"), Some(ADMIN), json!({"enabled": false})).await;
    let (_, ms) = call(&app, "GET", "/v1/methods", Some(a_key), Value::Null).await;
    assert!(ms.as_array().unwrap().is_empty());

    let (s, _) = call(
        &app,
        "POST",
        &format!("/admin/tenants/{a_id}/providers"),
        Some(ADMIN),
        json!({"method": "bogus", "provider_type": "x", "name": "n", "config": {}}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
