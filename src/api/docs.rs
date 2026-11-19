use super::AppState;
use axum::Router;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

const HTML: &str = include_str!("../../web/docs.html");
const JS: &str = include_str!("../../web/docs.js");
const CSS: &str = include_str!("../../web/docs.css");

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/docs",
            get(|| async { asset(HTML, "text/html; charset=utf-8") }),
        )
        .route(
            "/docs/docs.js",
            get(|| async { asset(JS, "text/javascript") }),
        )
        .route("/docs/docs.css", get(|| async { asset(CSS, "text/css") }))
}

pub async fn openapi_json() -> Response {
    let mut r = (StatusCode::OK, axum::Json(crate::openapi::spec())).into_response();
    r.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=300"),
    );
    r
}

fn asset(body: &'static str, content_type: &'static str) -> Response {
    let mut r = (StatusCode::OK, body).into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'",
        ),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    r
}
