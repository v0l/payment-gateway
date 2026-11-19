use super::*;

const HTML: &str = include_str!("../../../web/console.html");
const CSS: &str = include_str!("../../../web/console.css");
const TOKENS_CSS: &str = include_str!("../../../web/src/tokens.css");
const COMPONENTS_CSS: &str = include_str!("../../../web/components.css");
const STYLEGUIDE_HTML: &str = include_str!("../../../web/styleguide.html");
const STYLEGUIDE_JS: &str = include_str!("../../../web/styleguide.js");

pub const SCRIPTS: &[(&str, &str)] = &[
    ("core.js", include_str!("../../../web/console/core.js")),
    ("shell.js", include_str!("../../../web/console/shell.js")),
    ("auth.js", include_str!("../../../web/console/auth.js")),
    (
        "projects.js",
        include_str!("../../../web/console/projects.js"),
    ),
    (
        "payments.js",
        include_str!("../../../web/console/payments.js"),
    ),
    (
        "paydetail.js",
        include_str!("../../../web/console/paydetail.js"),
    ),
    (
        "paylist.js",
        include_str!("../../../web/console/paylist.js"),
    ),
    (
        "paytools.js",
        include_str!("../../../web/console/paytools.js"),
    ),
    ("chart.js", include_str!("../../../web/console/chart.js")),
    ("home.js", include_str!("../../../web/console/home.js")),
    (
        "customers.js",
        include_str!("../../../web/console/customers.js"),
    ),
    (
        "operator.js",
        include_str!("../../../web/console/operator.js"),
    ),
    ("danger.js", include_str!("../../../web/console/danger.js")),
    (
        "providers.js",
        include_str!("../../../web/console/providers.js"),
    ),
    ("keys.js", include_str!("../../../web/console/keys.js")),
    (
        "webhooks.js",
        include_str!("../../../web/console/webhooks.js"),
    ),
    ("team.js", include_str!("../../../web/console/team.js")),
    (
        "settings.js",
        include_str!("../../../web/console/settings.js"),
    ),
    (
        "branding.js",
        include_str!("../../../web/console/branding.js"),
    ),
    (
        "account.js",
        include_str!("../../../web/console/account.js"),
    ),
    ("admin.js", include_str!("../../../web/console/admin.js")),
    (
        "activity.js",
        include_str!("../../../web/console/activity.js"),
    ),
    (
        "platform-billing.js",
        include_str!("../../../web/console/platform-billing.js"),
    ),
    (
        "balance.js",
        include_str!("../../../web/console/balance.js"),
    ),
    ("shared.js", include_str!("../../../web/console/shared.js")),
    (
        "payouts.js",
        include_str!("../../../web/console/payouts.js"),
    ),
    (
        "billing.js",
        include_str!("../../../web/console/billing.js"),
    ),
    (
        "treasury.js",
        include_str!("../../../web/console/treasury.js"),
    ),
    ("app.js", include_str!("../../../web/console/app.js")),
];

const FONTS: &[(&str, &[u8])] = &[
    (
        "fonts/plex-sans-300.woff2",
        include_bytes!("../../../web/public/fonts/plex-sans-300.woff2"),
    ),
    (
        "fonts/plex-sans-400.woff2",
        include_bytes!("../../../web/public/fonts/plex-sans-400.woff2"),
    ),
    (
        "fonts/plex-sans-500.woff2",
        include_bytes!("../../../web/public/fonts/plex-sans-500.woff2"),
    ),
    (
        "fonts/plex-sans-600.woff2",
        include_bytes!("../../../web/public/fonts/plex-sans-600.woff2"),
    ),
    (
        "fonts/plex-mono-400.woff2",
        include_bytes!("../../../web/public/fonts/plex-mono-400.woff2"),
    ),
    (
        "fonts/plex-mono-500.woff2",
        include_bytes!("../../../web/public/fonts/plex-mono-500.woff2"),
    ),
];

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
    html(HTML)
}

fn html(body: &'static str) -> Response {
    let mut r = (StatusCode::OK, body).into_response();
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    security_headers(r.headers_mut());
    r
}

pub(super) async fn page_or_asset(Path(rest): Path<String>) -> Response {
    if let Some((_, js)) = SCRIPTS.iter().find(|(name, _)| *name == rest) {
        return asset(js, "text/javascript");
    }
    if let Some((_, bytes)) = FONTS.iter().find(|(name, _)| *name == rest) {
        return font(bytes);
    }
    match rest.as_str() {
        "app.css" => asset(CSS, "text/css"),
        "tokens.css" => asset(TOKENS_CSS, "text/css"),
        "components.css" => asset(COMPONENTS_CSS, "text/css"),
        "styleguide.js" => asset(STYLEGUIDE_JS, "text/javascript"),
        "styleguide" => html(STYLEGUIDE_HTML),
        _ => page().await,
    }
}

fn font(body: &'static [u8]) -> Response {
    let mut r = (StatusCode::OK, body).into_response();
    r.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("font/woff2"));
    r.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    r.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    r
}

fn asset(body: &'static str, content_type: &'static str) -> Response {
    let mut r = (StatusCode::OK, body).into_response();
    r.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    r
}
