mod admin;
mod console;
mod docs;
pub mod ledger;
pub mod ops;
mod page;
mod public;
mod receipt;
mod sandbox;
mod tenant;

use crate::service::{Error, Service};
use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
pub(crate) use console::settings_diff;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<Service>,
    pub admin_token: String,
    pub allow_mock: bool,
    pub sandbox: Option<Arc<crate::sandbox::Sandbox>>,
    pub login_limiter: Arc<crate::limiter::Limiter>,
    pub max_projects: usize,
    pub rate_limits: Arc<crate::ratelimit::RateLimits>,
    pub operator_2fa_grace: bool,
    pub ops: Arc<ops::Ops>,
}

impl AppState {
    pub fn new(
        service: Arc<Service>,
        admin_token: String,
        allow_mock: bool,
        sandbox: Option<Arc<crate::sandbox::Sandbox>>,
    ) -> Self {
        Self {
            service,
            admin_token,
            allow_mock,
            sandbox,
            login_limiter: Arc::new(crate::limiter::Limiter::new(
                8,
                std::time::Duration::from_secs(900),
            )),
            max_projects: 5,
            rate_limits: Arc::new(crate::ratelimit::RateLimits::new(Default::default())),
            operator_2fa_grace: true,
            ops: Default::default(),
        }
    }

    pub fn with_ops(mut self, ops: ops::Ops) -> Self {
        self.ops = Arc::new(ops);
        self
    }

    pub fn pool(&self) -> &sqlx::MySqlPool {
        &self.service.pool
    }
}

pub struct ApiError {
    pub status: StatusCode,
    pub message: String,
    pub extra: Option<serde_json::Map<String, serde_json::Value>>,
    pub retry_after: Option<u64>,
    pub field: Option<String>,
}

fn default_code(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "invalid_request",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::CONFLICT => "conflict",
        StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
        StatusCode::TOO_MANY_REQUESTS => "rate_limited",
        StatusCode::BAD_GATEWAY => "upstream_error",
        s if s.is_server_error() => "internal_error",
        _ => "error",
    }
}

impl ApiError {
    pub fn with_field(mut self, field: &str) -> Self {
        self.field = Some(field.to_string());
        self
    }
    pub fn insufficient_scope(needed: &str) -> Self {
        let mut e = Self::new(
            StatusCode::FORBIDDEN,
            format!("api key lacks the {needed} scope"),
        );
        e.extra = json!({"code": "insufficient_scope", "scope": needed})
            .as_object()
            .cloned();
        e
    }
    pub fn new(status: StatusCode, message: String) -> Self {
        Self {
            status,
            message,
            extra: None,
            retry_after: None,
            field: None,
        }
    }
    pub fn rate_limited(retry_after: u64) -> Self {
        let mut e = Self::new(
            StatusCode::TOO_MANY_REQUESTS,
            "rate limit exceeded, slow down".into(),
        );
        e.retry_after = Some(retry_after);
        e.extra = json!({"code": "rate_limited"}).as_object().cloned();
        e
    }
    pub fn unauthorized() -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized".into())
    }
    pub fn unauthorized_with(msg: &str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, msg.into())
    }
    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not found".into())
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, msg.into())
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        match e {
            Error::NotFound => Self::not_found(),
            Error::Invalid(m) => Self::new(StatusCode::BAD_REQUEST, m),
            Error::Conflict(m) => Self::new(StatusCode::CONFLICT, m),
            Error::LimitExceeded(l) => {
                let mut e = Self::new(StatusCode::FORBIDDEN, l.message());
                e.extra = json!({
                    "code": "limit_exceeded",
                    "limit": l.name,
                    "current": l.current,
                    "max": l.max,
                })
                .as_object()
                .cloned();
                e
            }
            Error::Upstream(m) => {
                log::warn!("upstream error: {}", crate::logsafe::clean(&m));
                Self::new(StatusCode::BAD_GATEWAY, m)
            }
            Error::Internal(e) => {
                log::error!("{}", crate::logsafe::clean(&format!("{e:#}")));
                Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
            }
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Error::from(e).into()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        Error::from(e).into()
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = self.extra.unwrap_or_default();
        let code = body
            .get("code")
            .and_then(|c| c.as_str())
            .unwrap_or_else(|| default_code(self.status))
            .to_string();
        let mut detail = json!({"code": code, "message": self.message});
        if let Some(field) = &self.field {
            detail["field"] = json!(field);
        }
        body.insert("error_detail".into(), detail);
        body.insert("error".into(), json!(self.message));
        let mut res = (self.status, axum::Json(body)).into_response();
        if let Some(secs) = self.retry_after {
            res.headers_mut().insert(
                axum::http::header::RETRY_AFTER,
                axum::http::HeaderValue::from(secs),
            );
        }
        res
    }
}

pub type ApiResult<T> = Result<axum::Json<T>, ApiError>;

pub fn router(state: AppState) -> Router {
    let limit = state.ops.body_limit;
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .merge(ops::router())
        .nest("/admin", admin::router())
        .nest("/v1", tenant::router())
        .merge(public::router())
        .merge(docs::router())
        .merge(receipt::router())
        .merge(sandbox::router())
        .merge(console::router())
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            rate_limit,
        ))
        .layer(ops::body_limit_layer(limit))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            ops::observe,
        ))
        .with_state(state)
}

pub const MAX_BODY_BYTES: usize = 1024 * 1024;

pub fn client_ip(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next_back())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "local".into())
}

async fn rate_limit(
    axum::extract::State(s): axum::extract::State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let ip = client_ip(req.headers());
    match s
        .rate_limits
        .for_request(req.method().as_str(), req.uri().path(), &ip)
    {
        Ok(()) => next.run(req).await,
        Err(secs) => ApiError::rate_limited(secs).into_response(),
    }
}

pub fn bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
