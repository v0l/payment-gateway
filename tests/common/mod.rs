#![allow(dead_code)]

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use payment_gateway::api::{self};
use payment_gateway::crypto::Sealer;
use payment_gateway::db;
use payment_gateway::mailer::{DisabledMailer, Mailer, RecordingMailer};
use payment_gateway::providers::mock::MockProvider;
use payment_gateway::providers::{DefaultFactory, Registry};
use payment_gateway::rates::StaticRates;
use payment_gateway::service::Service;
use serde_json::{Value, json};
use sqlx::MySqlPool;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

pub const ADMIN: &str = "admin-token-admin-token-admin";

pub struct Harness {
    pub app: Router,
    pub service: Arc<Service>,
    pub factory: Arc<DefaultFactory>,
    pub pool: MySqlPool,
    pub mailer: Arc<RecordingMailer>,
    pub db_guard: Arc<DbGuard>,
}

pub struct DbGuard {
    base: String,
    name: String,
}

impl Drop for DbGuard {
    fn drop(&mut self) {
        let base = self.base.clone();
        let name = self.name.clone();
        let _ = std::thread::spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            rt.block_on(async {
                if let Ok(admin) = MySqlPool::connect(&base).await {
                    let ids: Vec<i64> = sqlx::query_scalar(
                        "select cast(id as signed) from information_schema.processlist where db = ? and id <> connection_id()",
                    )
                    .bind(&name)
                    .fetch_all(&admin)
                    .await
                    .unwrap_or_default();
                    for id in ids {
                        let _ = sqlx::query(&format!("kill {id}")).execute(&admin).await;
                    }
                    let _ = sqlx::query("set session lock_wait_timeout = 10")
                        .execute(&admin)
                        .await;
                    let _ = sqlx::query(&format!("drop database if exists {name}"))
                        .execute(&admin)
                        .await;
                    admin.close().await;
                }
            });
        })
        .join();
    }
}

const STALE_AFTER_SECS: u64 = 3600;

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn test_db_age_secs(name: &str, now: u64) -> Option<u64> {
    let rest = name.strip_prefix("t_")?;
    let (secs, _) = rest.split_once('_')?;
    let created: u64 = secs.parse().ok()?;
    Some(now.saturating_sub(created))
}

async fn drop_stale_test_dbs(admin: &MySqlPool) {
    let names: Vec<String> = sqlx::query_scalar(
        "select cast(schema_name as char) from information_schema.schemata where schema_name like 't\\_%'",
    )
    .fetch_all(admin)
    .await
    .unwrap_or_default();
    let now = now_secs();
    for name in names {
        if test_db_age_secs(&name, now).is_some_and(|age| age > STALE_AFTER_SECS) {
            let _ = sqlx::query(&format!("drop database if exists `{name}`"))
                .execute(admin)
                .await;
        }
    }
}

static STALE_SWEEP: tokio::sync::OnceCell<()> = tokio::sync::OnceCell::const_new();

pub fn unique(prefix: &str) -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{prefix}{:x}{:x}", n, rand_u32())
}

fn rand_u32() -> u32 {
    use rand::RngCore;
    rand::thread_rng().next_u32()
}

pub async fn harness() -> Harness {
    build_harness(false).await
}

pub async fn harness_with_mail() -> Harness {
    build_harness(true).await
}

async fn build_harness(mail: bool) -> Harness {
    let mailer = Arc::new(RecordingMailer::new());
    let service_mailer: Arc<dyn Mailer> = if mail {
        mailer.clone()
    } else {
        Arc::new(DisabledMailer)
    };
    let base = std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL");
    let name = format!("t_{}_{:x}{:x}", now_secs(), std::process::id(), rand_u32());
    let admin = MySqlPool::connect(&base).await.unwrap();
    STALE_SWEEP
        .get_or_init(|| async { drop_stale_test_dbs(&admin).await })
        .await;
    sqlx::query(&format!("create database {name}"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    let (prefix, _) = base.rsplit_once('/').unwrap();
    let pool = db::connect(&format!("{prefix}/{name}")).await.unwrap();
    let sealer = Sealer::new(&[9u8; 32]);
    let factory = Arc::new(DefaultFactory::new(true));
    let registry = Registry::new(pool.clone(), sealer.clone(), factory.clone());
    let service = Service::with_mailer(
        pool.clone(),
        sealer,
        registry,
        Arc::new(StaticRates::new(&[("EUR", 50_000.0), ("USD", 60_000.0)])),
        "https://pay.test".to_string(),
        service_mailer,
    );
    let app = api::router(payment_gateway::api::AppState::new(
        service.clone(),
        ADMIN.into(),
        true,
        None,
    ));
    admin.close().await;
    Harness {
        app,
        service,
        factory,
        pool,
        mailer,
        db_guard: Arc::new(DbGuard { base, name }),
    }
}

pub struct Reply {
    pub status: StatusCode,
    pub json: Value,
    pub headers: HeaderMap,
    pub text: String,
}

pub async fn send(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    headers: &[(&str, &str)],
    body: Value,
) -> Reply {
    let mut req = Request::builder().method(method).uri(uri);
    if !body.is_null() {
        req = req.header("content-type", "application/json");
    }
    if let Some(t) = token {
        req = req.header("authorization", format!("Bearer {t}"));
    }
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let body = if body.is_null() {
        Body::empty()
    } else {
        Body::from(body.to_string())
    };
    let res = app.clone().oneshot(req.body(body).unwrap()).await.unwrap();
    let status = res.status();
    let headers = res.headers().clone();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8_lossy(&bytes).to_string();
    Reply {
        status,
        json: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        headers,
        text,
    }
}

pub async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let r = send(app, method, uri, token, &[], body).await;
    (r.status, r.json)
}

pub struct Receiver {
    pub url: String,
    pub got: Arc<Mutex<Vec<(HeaderMap, String)>>>,
    pub status: Arc<AtomicU16>,
}

impl Receiver {
    pub async fn start() -> Self {
        #[derive(Clone)]
        struct S {
            got: Arc<Mutex<Vec<(HeaderMap, String)>>>,
            status: Arc<AtomicU16>,
        }
        async fn handle(State(s): State<S>, headers: HeaderMap, body: String) -> StatusCode {
            s.got.lock().unwrap().push((headers, body));
            StatusCode::from_u16(s.status.load(Ordering::SeqCst)).unwrap()
        }
        let got: Arc<Mutex<Vec<(HeaderMap, String)>>> = Default::default();
        let status = Arc::new(AtomicU16::new(200));
        let app = Router::new()
            .route("/", axum::routing::post(handle))
            .with_state(S {
                got: got.clone(),
                status: status.clone(),
            });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        Self { url, got, status }
    }

    pub fn events(&self) -> Vec<Value> {
        self.got
            .lock()
            .unwrap()
            .iter()
            .map(|(_, b)| serde_json::from_str(b).unwrap())
            .collect()
    }

    pub fn types(&self) -> Vec<String> {
        self.events()
            .iter()
            .map(|e| e["type"].as_str().unwrap().to_string())
            .collect()
    }

    pub fn fail_with(&self, status: u16) {
        self.status.store(status, Ordering::SeqCst);
    }
}

pub struct TenantFixture {
    pub id: u64,
    pub key: String,
    pub webhook_secret: String,
    pub receiver: Receiver,
}

pub async fn tenant(h: &Harness) -> TenantFixture {
    let receiver = Receiver::start().await;
    let (s, t) = call(
        &h.app,
        "POST",
        "/admin/tenants",
        Some(ADMIN),
        json!({"name": unique("tenant"), "webhook_url": receiver.url}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{t}");
    TenantFixture {
        id: t["id"].as_u64().unwrap(),
        key: t["api_key"].as_str().unwrap().to_string(),
        webhook_secret: t["webhook_secret"].as_str().unwrap().to_string(),
        receiver,
    }
}

pub async fn provider(
    h: &Harness,
    t: &TenantFixture,
    method: &str,
    config: Value,
    fee: Option<(f64, u64, &str)>,
) -> u64 {
    let mut body = json!({
        "method": method, "provider_type": "mock", "name": unique(method), "config": config,
    });
    if let Some((rate, base, cur)) = fee {
        body["fee_rate"] = json!(rate);
        body["fee_base"] = json!(base);
        body["fee_currency"] = json!(cur);
    }
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/providers", t.id),
        Some(ADMIN),
        body,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    r["id"].as_u64().unwrap()
}

pub async fn new_payment(h: &Harness, t: &TenantFixture, body: Value) -> Value {
    let (s, r) = call(&h.app, "POST", "/v1/payments", Some(&t.key), body).await;
    assert_eq!(s, StatusCode::CREATED, "{r}");
    r
}

pub fn eur(reference: &str, cents: u64) -> Value {
    json!({"reference": reference, "amount": cents, "currency": "EUR"})
}

pub async fn attempt(h: &Harness, t: &TenantFixture, id: &str, body: Value) -> (StatusCode, Value) {
    call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/attempts"),
        Some(&t.key),
        body,
    )
    .await
}

pub async fn get_payment(h: &Harness, t: &TenantFixture, id: &str) -> Value {
    let (s, r) = call(
        &h.app,
        "GET",
        &format!("/v1/payments/{id}"),
        Some(&t.key),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    r
}

pub fn mock(h: &Harness, provider_id: u64) -> Arc<MockProvider> {
    h.factory.mocks.get(provider_id)
}

pub fn pid(hex: &str) -> [u8; 32] {
    db::parse_payment_id(hex).unwrap()
}
