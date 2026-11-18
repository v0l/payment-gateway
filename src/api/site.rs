use super::AppState;
use axum::Router;
use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::get;

const LANDING: &str = include_str!("../../web/site/landing.html");
const SITE_CSS: &str = include_str!("../../web/site.css");

macro_rules! pages {
    ($($section:literal => [$($slug:literal),*]),*) => {
        const PAGES: &[(&str, &str, &str)] = &[
            $($(($section, $slug, include_str!(concat!("../../web/site/", $section, "-", $slug, ".html"))),)*)*
        ];
    };
}

pages! {
    "guide" => ["index", "concepts", "quickstart-api", "quickstart-hosted", "quickstart-iframe", "webhooks", "going-live", "refunds-saved", "subscriptions", "custody", "errors", "faq", "reference"],
    "operator" => ["index", "deploy", "lnd", "providers", "shared-providers", "backups", "platform-billing", "monitoring", "runbooks"],
    "legal" => ["index", "terms", "privacy", "acceptable-use", "custody"]
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(|| async { html(LANDING) }))
        .route(
            "/assets/site.css",
            get(|| async { with_headers(SITE_CSS, "text/css") }),
        )
        .route("/{section}", get(section_redirect))
        .route("/{section}/", get(section_index))
        .route("/{section}/{slug}", get(section_page))
}

fn with_headers(body: &'static str, content_type: &'static str) -> Response {
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
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        ),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    r
}

fn html(body: &'static str) -> Response {
    with_headers(body, "text/html; charset=utf-8")
}

fn find(section: &str, slug: &str) -> Option<&'static str> {
    PAGES
        .iter()
        .find(|(s, p, _)| *s == section && *p == slug)
        .map(|(_, _, body)| *body)
}

fn not_found() -> Response {
    (StatusCode::NOT_FOUND, "not found").into_response()
}

async fn section_redirect(Path(section): Path<String>) -> Response {
    if PAGES.iter().any(|(s, _, _)| *s == section) {
        Redirect::permanent(&format!("/{section}/")).into_response()
    } else {
        not_found()
    }
}

async fn section_index(Path(section): Path<String>) -> Response {
    find(&section, "index").map(html).unwrap_or_else(not_found)
}

async fn section_page(Path((section, slug)): Path<(String, String)>) -> Response {
    find(&section, &slug).map(html).unwrap_or_else(not_found)
}
