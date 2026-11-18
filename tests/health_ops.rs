mod common;

use common::*;
use payment_gateway::api::ops::Ops;
use payment_gateway::api::{self, AppState};
use serde_json::{Value, json};

const METRICS: &str = "metrics-token-metrics-token-1";

async fn app_with(h: &Harness, ops: Ops) -> axum::Router {
    api::router(AppState::new(h.service.clone(), ADMIN.into(), true, None).with_ops(ops))
}

#[tokio::test]
async fn request_id_is_echoed_or_generated() {
    let h = harness().await;
    let r = send(
        &h.app,
        "GET",
        "/health",
        None,
        &[("x-request-id", "abc-123")],
        Value::Null,
    )
    .await;
    assert_eq!(r.headers["x-request-id"], "abc-123");
    let r = send(&h.app, "GET", "/health", None, &[], Value::Null).await;
    assert_eq!(r.headers["x-request-id"].to_str().unwrap().len(), 16);
    let r = send(
        &h.app,
        "GET",
        "/health",
        None,
        &[("x-request-id", "bad id")],
        Value::Null,
    )
    .await;
    assert_ne!(r.headers["x-request-id"], "bad id");
}

#[tokio::test]
async fn metrics_need_a_token_and_show_request_and_settlement_series() {
    let h = harness().await;
    let app = app_with(
        &h,
        Ops {
            metrics_token: Some(METRICS.into()),
            ..Default::default()
        },
    )
    .await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(&h, &t, eur("m1", 1000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let _ = send(
        &app,
        "GET",
        &format!("/v1/payments/{id}"),
        Some(&t.key),
        &[],
        Value::Null,
    )
    .await;

    assert_eq!(
        send(&app, "GET", "/metrics", None, &[], Value::Null)
            .await
            .status,
        401
    );
    assert_eq!(
        send(&app, "GET", "/metrics", Some("nope"), &[], Value::Null)
            .await
            .status,
        401
    );
    assert_eq!(
        send(&app, "GET", "/metrics", Some(&t.key), &[], Value::Null)
            .await
            .status,
        401
    );
    let by_metrics = send(&app, "GET", "/metrics", Some(METRICS), &[], Value::Null).await;
    assert_eq!(by_metrics.status, 200);
    let by_admin = send(&app, "GET", "/metrics", Some(ADMIN), &[], Value::Null).await;
    assert_eq!(by_admin.status, 200);
    let text = by_admin.text;
    assert!(text.contains("gateway_http_requests_total{method=\"GET\",route=\"/v1/payments/{id}\",status=\"200\"} 1"), "{text}");
    assert!(
        text.contains("gateway_http_request_duration_seconds_count{route=\"/v1/payments/{id}\"} 1")
    );
    assert!(text.contains("gateway_db_pool_connections{state=\"open\"}"));
    assert!(text.contains("gateway_webhook_pending"));
    assert!(text.contains("gateway_webhook_oldest_pending_age_seconds"));
    assert!(!text.contains(&t.key));
}

#[tokio::test]
async fn settlement_and_provider_calls_are_counted() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(&h, &t, eur("m2", 1000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let _ = ln;
    let (s, r) = attempt(&h, &t, &id, json!({"method": "lightning"})).await;
    assert!(s.is_success(), "{r}");
    let a = r["attempts"][0].clone();
    let before = h.service.metrics.counter("gateway_settlements_total", &[]);
    let attempt_id = a["id"].as_u64().unwrap();
    h.service
        .settle_attempt(attempt_id, "ref", None, true)
        .await
        .unwrap();
    assert_eq!(
        h.service.metrics.counter("gateway_settlements_total", &[]),
        before + 1
    );
    let text = h.service.metrics.render();
    assert!(
        text.contains("gateway_settlement_duration_seconds_count 1"),
        "{text}"
    );
    assert!(
        text.contains(
            "gateway_provider_calls_total{method=\"lightning\",op=\"create\",outcome=\"ok\"} 1"
        ),
        "{text}"
    );
}

#[tokio::test]
async fn ready_reports_db_migrations_and_shutdown() {
    let h = harness().await;
    let ops = Ops::default();
    let app = app_with(&h, ops).await;
    let r = send(&app, "GET", "/ready", None, &[], Value::Null).await;
    assert_eq!(r.status, 200, "{}", r.text);
    assert_eq!(r.json["db"], true);
    assert_eq!(r.json["migrations"], true);

    sqlx::query("delete from _sqlx_migrations where version = (select v from (select max(version) v from _sqlx_migrations) x)")
        .execute(&h.pool)
        .await
        .unwrap();
    let r = send(&app, "GET", "/ready", None, &[], Value::Null).await;
    assert_eq!(r.status, 503);
    assert_eq!(r.json["migrations"], false);
}

#[tokio::test]
async fn ready_flips_when_the_database_is_down_and_health_stays_cheap() {
    let h = harness().await;
    let app = app_with(&h, Ops::default()).await;
    assert_eq!(
        send(&app, "GET", "/ready", None, &[], Value::Null)
            .await
            .status,
        200
    );
    h.pool.close().await;
    let r = send(&app, "GET", "/ready", None, &[], Value::Null).await;
    assert_eq!(r.status, 503);
    assert_eq!(r.json["db"], false);
    assert_eq!(r.json["ready"], false);
    assert_eq!(
        send(&app, "GET", "/health", None, &[], Value::Null)
            .await
            .status,
        200
    );
}

#[tokio::test]
async fn ready_goes_503_while_shutting_down() {
    let h = harness().await;
    let ops = Ops::default();
    let state = AppState::new(h.service.clone(), ADMIN.into(), true, None).with_ops(ops);
    let ops = state.ops.clone();
    let app = api::router(state);
    assert_eq!(
        send(&app, "GET", "/ready", None, &[], Value::Null)
            .await
            .status,
        200
    );
    ops.begin_shutdown();
    let r = send(&app, "GET", "/ready", None, &[], Value::Null).await;
    assert_eq!(r.status, 503);
    assert_eq!(r.json["shutting_down"], true);
}

#[tokio::test]
async fn status_is_admin_only_and_has_a_html_view() {
    let h = harness().await;
    let app = app_with(&h, Ops::default()).await;
    assert_eq!(
        send(&app, "GET", "/status", None, &[], Value::Null)
            .await
            .status,
        401
    );
    let r = send(&app, "GET", "/status", Some(ADMIN), &[], Value::Null).await;
    assert_eq!(r.status, 200);
    assert_eq!(r.json["db"], true);
    assert!(r.json["webhooks"]["pending"].is_number());
    let r = send(
        &app,
        "GET",
        "/status",
        Some(ADMIN),
        &[("accept", "text/html")],
        Value::Null,
    )
    .await;
    assert!(r.text.starts_with("<!doctype html>"));
    assert!(!r.text.contains(ADMIN));
}

#[tokio::test]
async fn bodies_over_the_limit_are_rejected_on_every_route() {
    let h = harness().await;
    let app = app_with(
        &h,
        Ops {
            body_limit: 2048,
            ..Default::default()
        },
    )
    .await;
    let t = tenant(&h).await;
    let big = json!({"reference": "x".repeat(4000), "amount": 1, "currency": "EUR"});
    let r = send(&app, "POST", "/v1/payments", Some(&t.key), &[], big.clone()).await;
    assert_eq!(r.status, 413);
    let r = send(
        &app,
        "POST",
        "/admin/tenants",
        Some(ADMIN),
        &[],
        json!({"name": "y".repeat(4000)}),
    )
    .await;
    assert_eq!(r.status, 413);
    let r = send(
        &app,
        "POST",
        "/hook/1/1",
        None,
        &[],
        json!({"x": "z".repeat(4000)}),
    )
    .await;
    assert_eq!(r.status, 413);
    let r = send(
        &app,
        "POST",
        "/console/api/login",
        None,
        &[],
        json!({"x": "z".repeat(4000)}),
    )
    .await;
    assert_eq!(r.status, 413);
}

#[tokio::test]
async fn slow_handlers_time_out_with_504() {
    let h = harness().await;
    let app = app_with(
        &h,
        Ops {
            request_timeout: std::time::Duration::ZERO,
            ..Default::default()
        },
    )
    .await;
    let t = tenant(&h).await;
    let _ = t;
    let r = send(&app, "GET", "/status", Some(ADMIN), &[], Value::Null).await;
    assert_eq!(r.status, 504);
}

#[tokio::test]
async fn tenant_requests_carry_tenant_and_request_id_in_logs() {
    use payment_gateway::logging;
    let h = harness().await;
    let t = tenant(&h).await;
    let lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let sink = lines.clone();
    let logger = logging::Logger::new(
        log::LevelFilter::Info,
        true,
        Box::new(move |l| sink.lock().unwrap().push(l.to_string())),
    );
    let _ = log::set_boxed_logger(Box::new(logger));
    log::set_max_level(log::LevelFilter::Info);
    let _ = send(
        &h.app,
        "GET",
        "/v1/methods",
        Some(&t.key),
        &[("x-request-id", "rid-log-1")],
        Value::Null,
    )
    .await;
    let all = lines.lock().unwrap().join("\n");
    let line = all
        .lines()
        .find(|l| l.contains("rid-log-1"))
        .expect("request log line");
    let v: Value = serde_json::from_str(line).unwrap();
    assert_eq!(v["tenant_id"], t.id);
    assert!(!all.contains(&t.key));
}

#[tokio::test]
async fn exhausted_db_pool_fails_fast_instead_of_hanging() {
    let h = harness().await;
    let name: (String,) = sqlx::query_as("select database()")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    let base = std::env::var("TEST_DATABASE_URL").unwrap();
    let (prefix, _) = base.rsplit_once('/').unwrap();
    let pool = payment_gateway::db::connect_with(
        &format!("{prefix}/{}", name.0),
        1,
        std::time::Duration::from_millis(300),
    )
    .await
    .unwrap();
    let _held = pool.acquire().await.unwrap();
    let started = std::time::Instant::now();
    let err = sqlx::query("select 1").execute(&pool).await.unwrap_err();
    assert!(matches!(err, sqlx::Error::PoolTimedOut), "{err}");
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}
