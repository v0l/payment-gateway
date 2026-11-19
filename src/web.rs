use axum::extract::Path;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(RustEmbed)]
#[folder = "web/dist/"]
struct Dist;

#[derive(Deserialize)]
struct ManifestEntry {
    file: String,
    #[serde(default)]
    css: Vec<String>,
}

fn manifest() -> &'static HashMap<String, ManifestEntry> {
    static MANIFEST: OnceLock<HashMap<String, ManifestEntry>> = OnceLock::new();
    MANIFEST.get_or_init(|| {
        let file = Dist::get("manifest.json").expect("web/dist/manifest.json, run bun run build in web/");
        serde_json::from_slice(&file.data).expect("manifest.json")
    })
}

pub struct EntryAssets {
    pub script: String,
    pub styles: Vec<String>,
}

pub fn entry(name: &str) -> EntryAssets {
    let e = manifest()
        .get(&format!("{name}.html"))
        .unwrap_or_else(|| panic!("web entry {name} missing from manifest"));
    EntryAssets {
        script: format!("/static/{}", e.file),
        styles: e.css.iter().map(|c| format!("/static/{c}")).collect(),
    }
}

pub fn page(name: &str) -> &'static [u8] {
    match Dist::get(&format!("{name}.html")) {
        Some(f) => match f.data {
            std::borrow::Cow::Borrowed(b) => b,
            std::borrow::Cow::Owned(v) => Vec::leak(v),
        },
        None => panic!("web page {name} missing from dist"),
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "js" => "text/javascript",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "woff2" => "font/woff2",
        "json" => "application/json",
        "html" => "text/html; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn respond(path: &str, cache: &'static str) -> Response {
    match Dist::get(path) {
        Some(f) => {
            let mut r = (StatusCode::OK, f.data.into_owned()).into_response();
            let h = r.headers_mut();
            h.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static(content_type(path)),
            );
            h.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
            h.insert(
                header::X_CONTENT_TYPE_OPTIONS,
                HeaderValue::from_static("nosniff"),
            );
            r
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

const IMMUTABLE: &str = "public, max-age=31536000, immutable";

pub async fn static_file(Path(path): Path<String>) -> Response {
    if path == "manifest.json" || path.ends_with(".html") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let hashed = path.starts_with("assets/") || path.starts_with("fonts/");
    respond(&path, if hashed { IMMUTABLE } else { "no-cache" })
}

pub fn gateway_script() -> Response {
    let mut r = respond("gateway.js", "no-cache");
    r.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    r
}

pub fn favicon() -> Response {
    respond("favicon.svg", "no-cache")
}
