use super::*;

pub(super) fn security_headers(h: &mut HeaderMap) {
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; img-src 'self' data: https:; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
        ),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
}

pub(super) async fn page() -> Response {
    html(crate::web::page("console"))
}

fn html(body: &'static [u8]) -> Response {
    let mut r = (StatusCode::OK, body).into_response();
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    security_headers(r.headers_mut());
    r
}

pub(super) async fn page_or_asset(Path(rest): Path<String>) -> Response {
    if rest.starts_with("fonts/")
        && let Some(r) = crate::web::font(&rest)
    {
        return r;
    }
    match rest.as_str() {
        "styleguide" => html(crate::web::page("styleguide")),
        _ => page().await,
    }
}
