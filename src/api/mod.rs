mod admin;
mod tenant;

use crate::crypto::Sealer;
use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde_json::json;
use sqlx::MySqlPool;

#[derive(Clone)]
pub struct AppState {
    pub pool: MySqlPool,
    pub sealer: Sealer,
    pub admin_token: String,
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

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        match e.downcast::<sqlx::Error>() {
            Ok(e) => e.into(),
            Err(e) => {
                log::error!("{e:#}");
                Self(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
            }
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(e: sqlx::Error) -> Self {
        if let sqlx::Error::Database(d) = &e {
            if d.is_unique_violation() {
                return Self(StatusCode::CONFLICT, "already exists".into());
            }
        }
        log::error!("{e:#}");
        Self(StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
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
        .with_state(state)
}

pub fn bearer(headers: &axum::http::HeaderMap) -> Option<&str> {
    headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
}
