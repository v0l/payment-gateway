mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::ProviderSink;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::PathBuf;

mod rust_sample {
    include!("../docs-site/samples/verify.rs");
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn markdown_files() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for dir in ["guide", "operator", "legal"] {
        let mut files: Vec<_> = std::fs::read_dir(root().join("docs-site").join(dir))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "md"))
            .collect();
        files.sort();
        out.extend(files);
    }
    out
}

fn blocks(md: &str, info: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in md.lines() {
        match (&mut current, line.strip_prefix("```")) {
            (None, Some(i)) if i.trim() == info => current = Some(Vec::new()),
            (Some(buf), Some(_)) => {
                out.push(buf.join("\n"));
                current = None;
            }
            (Some(buf), None) => buf.push(line),
            _ => {}
        }
    }
    out
}

async fn serve(h: &Harness) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = h.app.clone();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn openapi_snapshot_used_for_the_reference_page_matches_the_served_spec() {
    let path = root().join("docs-site/openapi.json");
    let want = serde_json::to_string_pretty(&payment_gateway::openapi::spec()).unwrap() + "\n";
    if std::env::var("UPDATE_DOCS").is_ok_and(|v| v == "1") {
        std::fs::write(&path, &want).unwrap();
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert_eq!(
        have, want,
        "run UPDATE_DOCS=1 cargo test --test docs_site, then node docs-site/build.mjs"
    );
}

#[test]
fn generated_pages_are_up_to_date() {
    let out = std::process::Command::new("node")
        .arg("docs-site/build.mjs")
        .arg("--check")
        .current_dir(root())
        .output()
        .expect("node is needed to build the docs");
    assert!(
        out.status.success(),
        "run node docs-site/build.mjs\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn docs_use_no_em_or_en_dashes() {
    for f in markdown_files() {
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(
            !text.contains('\u{2014}') && !text.contains('\u{2013}'),
            "{}",
            f.display()
        );
    }
}

#[tokio::test]
async fn every_internal_link_resolves() {
    let h = harness().await;
    let mut checked = HashSet::new();
    let mut sources = vec![std::fs::read_to_string(root().join("web/site/landing.html")).unwrap()];
    for f in markdown_files() {
        sources.push(std::fs::read_to_string(f).unwrap());
    }
    let md_link = regex_lite_links;
    for text in sources {
        for href in md_link(&text) {
            let path = href.split('#').next().unwrap().to_string();
            if path.is_empty() || path.starts_with("http") || !checked.insert(path.clone()) {
                continue;
            }
            let r = send(&h.app, "GET", &path, None, &[], Value::Null).await;
            assert_eq!(r.status, StatusCode::OK, "link {path}");
        }
    }
    assert!(checked.len() > 15, "{checked:?}");
}

fn regex_lite_links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find("](") {
        rest = &rest[i + 2..];
        if let Some(end) = rest.find(')') {
            out.push(rest[..end].to_string());
        }
    }
    let mut rest = text;
    while let Some(i) = rest.find("href=\"") {
        rest = &rest[i + 6..];
        if let Some(end) = rest.find('"') {
            out.push(rest[..end].to_string());
        }
    }
    out.into_iter().filter(|l| l.starts_with('/')).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runnable_shell_samples_work_against_the_gateway() {
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/settings", t.id),
        Some(ADMIN),
        json!({"allowed_origins": ["https://shop.example.com"]}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let base = serve(&h).await;

    let mut ran = 0;
    for f in markdown_files() {
        let md = std::fs::read_to_string(&f).unwrap();
        let samples = blocks(&md, "sh run");
        if samples.is_empty() {
            continue;
        }
        let script = format!("set -eu\n{}\n", samples.join("\n"));
        let out = std::process::Command::new("bash")
            .arg("-c")
            .arg(&script)
            .env("GW", &base)
            .env("KEY", &t.key)
            .env("SHOP_ORIGIN", "https://shop.example.com")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}:\n{script}\nstdout: {}\nstderr: {}",
            f.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        ran += samples.len();
    }
    assert!(ran >= 5, "ran {ran} samples");
}

fn compile_js_client() -> Option<PathBuf> {
    let out = std::env::temp_dir().join(format!("pgw-js-client-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&out);
    let status = std::process::Command::new("tsc")
        .args(["--outDir"])
        .arg(&out)
        .args(["--module", "es2020", "--target", "es2020", "--moduleResolution", "bundler", "--lib", "es2022,dom", "--skipLibCheck", "--declaration", "false"])
        .arg(root().join("clients/js/src/index.ts"))
        .output()
        .ok()?;
    let _ = status;
    std::fs::write(out.join("package.json"), "{\"type\":\"module\"}").ok()?;
    out.join("index.js").exists().then_some(out)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runnable_js_client_samples_work_against_the_gateway() {
    let Some(client) = compile_js_client() else {
        eprintln!("skipped: tsc not found");
        return;
    };
    let h = harness().await;
    let t = tenant(&h).await;
    provider(&h, &t, "lightning", json!({}), None).await;
    let base = serve(&h).await;
    let mut ran = 0;
    for f in markdown_files() {
        let md = std::fs::read_to_string(&f).unwrap();
        for (i, sample) in blocks(&md, "js run").iter().enumerate() {
            let code = sample.replace(
                "\"@payment-gateway/client\"",
                &format!("\"{}\"", client.join("index.js").display()),
            );
            let file = client.join(format!("sample-{ran}-{i}.mjs"));
            std::fs::write(&file, code).unwrap();
            let out = std::process::Command::new("node")
                .arg(&file)
                .env("GW", &base)
                .env("KEY", &t.key)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}:\n{sample}\nstderr: {}",
                f.display(),
                String::from_utf8_lossy(&out.stderr)
            );
            ran += 1;
        }
    }
    assert!(ran >= 1);
}

async fn signed_delivery(h: &Harness, t: &TenantFixture) -> (String, String, String) {
    let lightning = provider(h, t, "lightning", json!({}), None).await;
    let p = new_payment(h, t, eur("docs-sig", 1000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: h.service.clone(),
        provider_id: lightning,
    }
    .event(
        ProviderEvent::Paid {
            lookup: hash.clone(),
            reference: hash,
            amount: None,
            confirmed: true,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();
    h.service.deliver_due().await.unwrap();
    let got = t.receiver.got.lock().unwrap().clone();
    let (headers, body) = got[0].clone();
    let sig = headers["x-gateway-signature"].to_str().unwrap().to_string();
    let ts = sig
        .strip_prefix("t=")
        .unwrap()
        .split_once(',')
        .unwrap()
        .0
        .to_string();
    (sig, body, ts)
}

async fn node_verify(secret: &str, sig: &str, body: &str, now: i64) -> String {
    let script = "const {verify}=require('./docs-site/samples/verify.js');const [s,h,b,n]=process.argv.slice(1);console.log(verify(s,h,b,Number(n)))";
    let out = std::process::Command::new("node")
        .args(["-e", script, secret, sig, body, &now.to_string()])
        .current_dir(root())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

async fn python_verify(secret: &str, sig: &str, body: &str, now: i64) -> String {
    let script = "import sys\nsys.path.insert(0,'docs-site/samples')\nfrom verify import verify\ns,h,b,n=sys.argv[1:5]\nprint(str(verify(s,h,b,int(n))).lower())";
    let out = std::process::Command::new("python3")
        .args(["-c", script, secret, sig, body, &now.to_string()])
        .current_dir(root())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[tokio::test]
async fn webhook_verification_samples_accept_real_signatures_and_reject_bad_ones() {
    let h = harness().await;
    let t = tenant(&h).await;
    let (sig, body, ts) = signed_delivery(&h, &t).await;
    let now: i64 = ts.parse().unwrap();

    assert!(rust_sample::verify_webhook(
        &t.webhook_secret,
        &sig,
        &body,
        now,
        300
    ));
    assert!(!rust_sample::verify_webhook(
        "wrong-secret",
        &sig,
        &body,
        now,
        300
    ));
    assert!(!rust_sample::verify_webhook(
        &t.webhook_secret,
        &sig,
        &format!("{body} "),
        now,
        300
    ));
    assert!(!rust_sample::verify_webhook(
        &t.webhook_secret,
        &sig,
        &body,
        now + 301,
        300
    ));
    assert!(!rust_sample::verify_webhook(
        &t.webhook_secret,
        "garbage",
        &body,
        now,
        300
    ));

    for lang in ["node", "python"] {
        let run = |secret: &str, sig: &str, body: &str, now: i64| {
            let (secret, sig, body) = (secret.to_string(), sig.to_string(), body.to_string());
            async move {
                if lang == "node" {
                    node_verify(&secret, &sig, &body, now).await
                } else {
                    python_verify(&secret, &sig, &body, now).await
                }
            }
        };
        assert_eq!(run(&t.webhook_secret, &sig, &body, now).await, "true");
        assert_eq!(run("wrong-secret", &sig, &body, now).await, "false");
        assert_eq!(
            run(&t.webhook_secret, &sig, &format!("{body} "), now).await,
            "false"
        );
        assert_eq!(
            run(&t.webhook_secret, &sig, &body, now + 301).await,
            "false"
        );
        assert_eq!(run(&t.webhook_secret, "garbage", &body, now).await, "false");
    }
}

#[test]
fn webhook_page_contains_the_verified_samples_verbatim() {
    let md = std::fs::read_to_string(root().join("docs-site/guide/webhooks.md")).unwrap();
    for (file, lang) in [
        ("verify.js", "js"),
        ("verify.py", "python"),
        ("verify.rs", "rust"),
    ] {
        assert!(
            md.contains(&format!("@include samples/{file} {lang}")),
            "{file}"
        );
    }
    let html = std::fs::read_to_string(root().join("web/site/guide-webhooks.html")).unwrap();
    let js = std::fs::read_to_string(root().join("docs-site/samples/verify.js")).unwrap();
    assert!(html.contains("timingSafeEqual"));
    assert!(js.contains("timingSafeEqual"));
}

fn tags<'a>(html: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}");
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        if after.starts_with(|c: char| c.is_whitespace() || c == '>') {
            let end = after.find('>').unwrap();
            out.push(&after[..end]);
        }
        rest = after;
    }
    out
}

fn assert_accessible(page: &str, label: &str) {
    assert!(page.starts_with("<!doctype html>"), "{label}");
    assert!(page.contains("<html lang=\"en\">"), "{label}: lang");
    assert!(
        page.contains("<title>") && page.contains("name=\"viewport\""),
        "{label}"
    );
    assert_eq!(page.matches("<main").count(), 1, "{label}: one main");
    assert_eq!(page.matches("<h1").count(), 1, "{label}: one h1");
    assert!(
        page.contains("class=\"skip\"") && page.contains("id=\"content\""),
        "{label}: skip link"
    );
    assert!(!page.contains("<script"), "{label}: script");
    assert!(!page.contains("<style"), "{label}: inline style");
    assert!(!page.contains("style="), "{label}: style attribute");
    assert!(
        !page.contains("src=\"http") && !page.contains("href=\"http"),
        "{label}: external reference"
    );
    for tag in [
        "a", "button", "img", "input", "div", "p", "section", "nav", "li",
    ] {
        for attrs in tags(page, tag) {
            for handler in [" onclick=", " onload=", " onerror=", " onfocus="] {
                assert!(!attrs.contains(handler), "{label}: {handler}");
            }
        }
    }
    for attrs in tags(page, "img") {
        assert!(attrs.contains("alt="), "{label}: img without alt");
    }
    for nav in tags(page, "nav") {
        assert!(nav.contains("aria-label="), "{label}: unlabelled nav");
    }
    let mut ids = HashSet::new();
    let mut rest = page;
    while let Some(i) = rest.find(" id=\"") {
        rest = &rest[i + 5..];
        let id = &rest[..rest.find('"').unwrap()];
        assert!(ids.insert(id.to_string()), "{label}: duplicate id {id}");
    }
    let mut rest = page;
    while let Some(i) = rest.find("<a ") {
        rest = &rest[i..];
        let end = rest.find("</a>").unwrap();
        let inner = &rest[rest.find('>').unwrap() + 1..end];
        assert!(
            !inner.trim().is_empty(),
            "{label}: empty link {}",
            &rest[..end]
        );
        rest = &rest[end..];
    }
    let mut level = 0;
    let mut rest = page;
    while let Some(i) = rest.find("<h") {
        rest = &rest[i + 2..];
        if let Some(d) = rest.chars().next().and_then(|c| c.to_digit(10)) {
            assert!(
                level == 0 || d <= level + 1,
                "{label}: heading jumps to h{d}"
            );
            level = d;
        }
    }
    for href in regex_lite_links(page) {
        if let Some(id) = href.strip_prefix('#') {
            assert!(ids.contains(id), "{label}: dangling anchor {id}");
        }
    }
    let mut rest = page;
    while let Some(i) = rest.find("href=\"#") {
        rest = &rest[i + 7..];
        let id = &rest[..rest.find('"').unwrap()];
        assert!(ids.contains(id), "{label}: dangling anchor #{id}");
    }
}

#[tokio::test]
async fn landing_page_is_accessible_csp_safe_and_links_to_the_essentials() {
    let h = harness().await;
    let r = send(&h.app, "GET", "/", None, &[], Value::Null).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_accessible(&r.text, "/");
    for needed in [
        "href=\"/console\"",
        "href=\"/docs\"",
        "href=\"/guide/\"",
        "Sign in",
        "id=\"pricing\"",
        "/legal/terms",
        "/legal/privacy",
        "/legal/acceptable-use",
        "/legal/custody",
    ] {
        assert!(r.text.contains(needed), "landing lacks {needed}");
    }
    let csp = r.headers["content-security-policy"].to_str().unwrap();
    assert!(csp.contains("script-src 'self'") && csp.contains("frame-ancestors 'none'"));
    assert!(!csp.contains("unsafe"));
    assert_eq!(r.headers["x-content-type-options"], "nosniff");
    assert_eq!(r.headers["x-frame-options"], "DENY");
    let css = send(&h.app, "GET", "/assets/site.css", None, &[], Value::Null).await;
    assert_eq!(css.status, StatusCode::OK);
}

#[tokio::test]
async fn every_docs_and_legal_page_is_served_accessible_and_legal_is_marked_draft() {
    let h = harness().await;
    let nav: Value =
        serde_json::from_str(&std::fs::read_to_string(root().join("docs-site/nav.json")).unwrap())
            .unwrap();
    let mut pages = 0;
    for section in nav["sections"].as_array().unwrap() {
        let id = section["id"].as_str().unwrap();
        for p in section["pages"].as_array().unwrap() {
            let slug = p["slug"].as_str().unwrap();
            let uri = if slug == "index" {
                format!("/{id}/")
            } else {
                format!("/{id}/{slug}")
            };
            let r = send(&h.app, "GET", &uri, None, &[], Value::Null).await;
            assert_eq!(r.status, StatusCode::OK, "{uri}");
            assert_accessible(&r.text, &uri);
            let csp = r.headers["content-security-policy"].to_str().unwrap();
            assert!(
                csp.contains("script-src 'self'") && !csp.contains("unsafe"),
                "{uri}"
            );
            if id == "legal" && slug != "index" {
                assert!(r.text.contains("Draft template"), "{uri}");
                assert!(r.text.contains("lawyer"), "{uri}");
            }
            pages += 1;
        }
    }
    assert!(pages >= 25, "{pages}");
    let redirect = send(&h.app, "GET", "/guide", None, &[], Value::Null).await;
    assert!(redirect.status.is_redirection());
    let missing = send(&h.app, "GET", "/guide/nope", None, &[], Value::Null).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
    let unknown = send(&h.app, "GET", "/nothing/", None, &[], Value::Null).await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn error_reference_lists_every_code_the_api_returns() {
    let md = std::fs::read_to_string(root().join("docs-site/guide/errors.md")).unwrap();
    for code in [
        "invalid_request",
        "unauthorized",
        "forbidden",
        "not_found",
        "conflict",
        "payload_too_large",
        "rate_limited",
        "upstream_error",
        "internal_error",
        "insufficient_scope",
        "limit_exceeded",
    ] {
        assert!(md.contains(&format!("`{code}`")), "{code}");
    }
    let h = harness().await;
    let t = tenant(&h).await;
    let r = send(
        &h.app,
        "GET",
        "/v1/payments?limit=0",
        Some(&t.key),
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.json["error_detail"]["code"], "invalid_request");
    let r = send(
        &h.app,
        "GET",
        "/v1/payments",
        Some("nope"),
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.json["error_detail"]["code"], "unauthorized");
}
