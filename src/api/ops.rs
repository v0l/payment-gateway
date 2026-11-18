use super::{ApiError, AppState, bearer};
use crate::db;
use crate::lease::Lease;
use crate::logging::{CONTEXT, RequestContext};
use axum::Router;
use axum::extract::{MatchedPath, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use serde_json::{Value, json};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;

pub struct Ops {
    pub metrics_token: Option<String>,
    pub lease: OnceLock<Arc<Lease>>,
    pub shutting_down: AtomicBool,
    pub shutdown: tokio::sync::watch::Sender<bool>,
    pub started: Instant,
    pub body_limit: usize,
    pub request_timeout: Duration,
    pub instance: String,
}

impl Default for Ops {
    fn default() -> Self {
        let (shutdown, _) = tokio::sync::watch::channel(false);
        Self {
            metrics_token: None,
            lease: OnceLock::new(),
            shutting_down: AtomicBool::new(false),
            shutdown,
            started: Instant::now(),
            body_limit: 1_048_576,
            request_timeout: Duration::from_secs(60),
            instance: "gateway".into(),
        }
    }
}

impl Ops {
    pub fn begin_shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        let _ = self.shutdown.send(true);
    }

    pub fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::SeqCst)
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/ready", get(ready))
        .route("/status", get(status))
        .route("/metrics", get(metrics))
}

fn valid_request_id(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 64
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn new_request_id() -> String {
    use rand::RngCore;
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    hex::encode(b)
}

fn is_streaming(path: &str) -> bool {
    path == "/pay/{id}/events"
}

pub async fn observe(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .filter(|v| valid_request_id(v))
        .map(str::to_string)
        .unwrap_or_else(new_request_id);
    let route = req
        .extensions()
        .get::<MatchedPath>()
        .map(|m| m.as_str().to_string())
        .unwrap_or_else(|| "unmatched".into());
    let method = req.method().to_string();
    let ctx = Arc::new(Mutex::new(RequestContext {
        request_id: request_id.clone(),
        tenant_id: None,
    }));
    let metrics = s.service.metrics.clone();
    let timeout = s.ops.request_timeout;
    let started = Instant::now();
    let scoped = ctx.clone();
    let mut response = CONTEXT
        .scope(ctx, async move {
            let streaming = is_streaming(&route);
            let outcome = if streaming {
                Ok(next.run(req).await)
            } else {
                tokio::time::timeout(timeout, next.run(req)).await
            };
            let response = match outcome {
                Ok(r) => r,
                Err(_) => {
                    log::error!("request timed out: {method} {route}");
                    ApiError::new(StatusCode::GATEWAY_TIMEOUT, "request timed out".into())
                        .into_response()
                }
            };
            let elapsed = started.elapsed();
            let status = response.status().as_u16().to_string();
            metrics.inc(
                "gateway_http_requests_total",
                &[("method", &method), ("route", &route), ("status", &status)],
            );
            metrics.observe(
                "gateway_http_request_duration_seconds",
                &[("route", &route)],
                elapsed.as_secs_f64(),
            );
            if route != "/health" && route != "/ready" && route != "/metrics" {
                log::info!(
                    "request {method} {route} status={status} ms={}",
                    elapsed.as_millis()
                );
            }
            response
        })
        .await;
    drop(scoped);
    if let Ok(v) = HeaderValue::from_str(&request_id) {
        response.headers_mut().insert("x-request-id", v);
    }
    response
}

async fn db_ok(s: &AppState) -> bool {
    tokio::time::timeout(Duration::from_secs(2), async {
        sqlx::query("select 1").execute(s.pool()).await.is_ok()
    })
    .await
    .unwrap_or(false)
}

async fn migrations_ok(s: &AppState) -> bool {
    let expected = db::expected_migrations();
    let applied: Result<(i64,), _> =
        sqlx::query_as("select count(*) from _sqlx_migrations where success = 1")
            .fetch_one(s.pool())
            .await;
    matches!(applied, Ok((n,)) if n as usize >= expected)
}

async fn ready(State(s): State<AppState>) -> Response {
    let shutting_down = s.ops.is_shutting_down();
    let db = db_ok(&s).await;
    let migrations = db && migrations_ok(&s).await;
    let leader = s.ops.lease.get().is_some_and(|l| l.is_held());
    let ok = db && migrations && !shutting_down;
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        axum::Json(json!({
            "ready": ok,
            "db": db,
            "migrations": migrations,
            "shutting_down": shutting_down,
            "leader": leader,
        })),
    )
        .into_response()
}

fn admin_ok(s: &AppState, headers: &HeaderMap) -> bool {
    let Some(t) = bearer(headers) else {
        return false;
    };
    t.as_bytes().ct_eq(s.admin_token.as_bytes()).into()
}

fn metrics_ok(s: &AppState, headers: &HeaderMap) -> bool {
    let Some(t) = bearer(headers) else {
        return false;
    };
    if admin_ok(s, headers) {
        return true;
    }
    s.ops
        .metrics_token
        .as_ref()
        .is_some_and(|m| t.as_bytes().ct_eq(m.as_bytes()).into())
}

async fn refresh_gauges(s: &AppState) {
    let m = &s.service.metrics;
    let pool = s.pool();
    m.set(
        "gateway_db_pool_connections",
        &[("state", "open")],
        pool.size() as f64,
    );
    m.set(
        "gateway_db_pool_connections",
        &[("state", "idle")],
        pool.num_idle() as f64,
    );
    m.set(
        "gateway_uptime_seconds",
        &[],
        s.ops.started.elapsed().as_secs_f64(),
    );
    m.set(
        "gateway_leader",
        &[],
        if s.ops.lease.get().is_some_and(|l| l.is_held()) {
            1.0
        } else {
            0.0
        },
    );
    if let Ok(Some((pending, oldest))) = db::pending_delivery_stats(pool, chrono::Utc::now()).await
    {
        m.set("gateway_webhook_pending", &[], pending as f64);
        m.set(
            "gateway_webhook_oldest_pending_age_seconds",
            &[],
            oldest as f64,
        );
    }
}

async fn metrics(State(s): State<AppState>, headers: HeaderMap) -> Response {
    if !metrics_ok(&s, &headers) {
        return ApiError::unauthorized().into_response();
    }
    refresh_gauges(&s).await;
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        s.service.metrics.render(),
    )
        .into_response()
}

async fn status_json(s: &AppState) -> Value {
    refresh_gauges(s).await;
    let db = db_ok(s).await;
    let lease = match s.ops.lease.get() {
        Some(l) => {
            let row = l.current().await.ok().flatten();
            json!({
                "this_instance": l.holder(),
                "held_by_this_instance": l.is_held(),
                "holder": row.as_ref().map(|r| r.holder.clone()),
                "expires": row.as_ref().map(|r| r.expires),
            })
        }
        None => json!(null),
    };
    let (pending, oldest) = db::pending_delivery_stats(s.pool(), chrono::Utc::now())
        .await
        .ok()
        .flatten()
        .unwrap_or((0, 0));
    json!({
        "version": env!("CARGO_PKG_VERSION"),
        "instance": s.ops.instance,
        "uptime_seconds": s.ops.started.elapsed().as_secs(),
        "shutting_down": s.ops.is_shutting_down(),
        "db": db,
        "db_pool": { "open": s.pool().size(), "idle": s.pool().num_idle() },
        "lease": lease,
        "webhooks": { "pending": pending, "oldest_pending_age_seconds": oldest },
        "watchers": s.service.metrics.gauges("gateway_watcher_up").into_iter()
            .map(|(labels, up)| json!({"labels": labels, "up": up >= 1.0}))
            .collect::<Vec<_>>(),
    })
}

fn escape_html(v: &str) -> String {
    v.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

async fn status(State(s): State<AppState>, headers: HeaderMap) -> Response {
    if !admin_ok(&s, &headers) {
        return ApiError::unauthorized().into_response();
    }
    let body = status_json(&s).await;
    let wants_html = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.contains("text/html"));
    if wants_html {
        let pretty = serde_json::to_string_pretty(&body).unwrap_or_default();
        return (
            [(header::CACHE_CONTROL, "no-store")],
            Html(format!(
                "<!doctype html><title>gateway status</title><pre>{}</pre>",
                escape_html(&pretty)
            )),
        )
            .into_response();
    }
    axum::Json(body).into_response()
}

pub fn body_limit_layer(limit: usize) -> axum::extract::DefaultBodyLimit {
    axum::extract::DefaultBodyLimit::max(limit)
}
