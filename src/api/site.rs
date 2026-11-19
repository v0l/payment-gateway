use super::AppState;
use axum::Router;
use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;

const SECTIONS: [&str; 3] = ["guide", "operator", "legal"];

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(|| async { page("landing") }))
        .route("/{section}", get(section_redirect))
        .route("/{section}/", get(section_index))
        .route("/{section}/{slug}", get(section_page))
}

fn with_headers(body: &'static [u8], content_type: &'static str) -> Response {
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
            "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        ),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    r
}

fn page(name: &str) -> Response {
    crate::web::site_page(name)
        .map(|body| with_headers(body, "text/html; charset=utf-8"))
        .unwrap_or_else(not_found)
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

async fn section_redirect(Path(section): Path<String>) -> Response {
    if SECTIONS.contains(&section.as_str()) {
        Redirect::permanent(&format!("/{section}/")).into_response()
    } else {
        not_found()
    }
}

async fn section_index(Path(section): Path<String>) -> Response {
    page(&format!("{section}-index"))
}

async fn section_page(Path((section, slug)): Path<(String, String)>) -> Response {
    page(&format!("{section}-{slug}"))
}
