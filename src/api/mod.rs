mod admin;
mod public;
mod tenant;

use crate::service::{Error, Service};
use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub service: Arc<Service>,
    pub admin_token: String,
    pub allow_mock: bool,
}

impl AppState {
    pub fn pool(&self) -> &sqlx::MySqlPool {
        &self.service.pool
    }
}

pub struct ApiError(pub StatusCode, pub String);

impl ApiError {
    pub fn unauthorized() -> Self {
        Self(StatusCode::UNAUTHORIZED, "unauthorized".into())
    }
    pub fn not_found() -> Self {
        Self(StatusCode::NOT_FOUND, "not found".into())
    }
    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, msg.into())
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        match e {
            Error::NotFound => Self::not_found(),
            Error::Invalid(m) => Self(StatusCode::BAD_REQUEST, m),
            Error::Conflict(m) => Self(StatusCode::CONFLICT, m),
            Error::Upstream(m) => {
                log::warn!("upstream error: {m}");
                Self(StatusCode::BAD_GATEWAY, m)
            }
            Error::Internal(e) => {
                log::error!("{e:#}");
                Self(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
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
        (self.0, axum::Json(json!({ "error": self.1 }))).into_response()
    }
}

pub type ApiResult<T> = Result<axum::Json<T>, ApiError>;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(|| async { "ok" }))
        .nest("/admin", admin::router())
        .nest("/v1", tenant::router())
        .merge(public::router())
        .with_state(state)
}

pub fn bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
