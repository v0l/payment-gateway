mod common;
use axum::Router;
use axum::http::StatusCode;
use common::*;
use payment_gateway::accounts;
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::ProviderSink;
use serde_json::{Value, json};

const PASSWORD: &str = "correct horse battery";

async fn state(h: &Harness, id: &str) -> Value {
    let r = send(
        &h.app,
        "GET",
        &format!("/pay/{id}/state"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    r.json
}

async fn set_branding(h: &Harness, t: &TenantFixture, body: Value) -> (StatusCode, Value) {
    call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/branding", t.id),
        Some(ADMIN),
        body,
    )
    .await
}

struct Login {
    headers: Vec<(&'static str, String)>,
}

impl Login {
    async fn new(app: &Router, pool: &sqlx::MySqlPool, email: &str, operator: bool) -> Self {
        let hash = accounts::hash_password(PASSWORD).unwrap();
        accounts::insert_account(pool, email, &hash, operator)
            .await
            .unwrap();
        let r = send(
            app,
            "POST",
            "/console/api/login",
            None,
            &[],
            json!({"email": email, "password": PASSWORD}),
        )
        .await;
        let cookie = r.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_string();
        Self {
            headers: vec![
                ("cookie", cookie),
                ("x-csrf-token", r.json["csrf"].as_str().unwrap().to_string()),
            ],
        }
    }

    async fn req(&self, app: &Router, method: &str, path: &str, body: Value) -> Reply {
        let h: Vec<(&str, &str)> = self.headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        send(app, method, &format!("/console/api{path}"), None, &h, body).await
    }
}

#[tokio::test]
async fn favicon_is_served_and_linked_from_the_pay_page_and_the_console() {
    let h = harness().await;
    for path in ["/favicon.ico", "/favicon.svg"] {
        let r = send(&h.app, "GET", path, None, &[], Value::Null).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
        assert_eq!(r.headers["content-type"], "image/svg+xml");
        assert!(r.text.contains("<svg"));
    }
    let console = send(&h.app, "GET", "/console", None, &[], Value::Null).await;
    assert!(console.text.contains("rel=\"icon\""));
    let t = tenant(&h).await;
    let p = new_payment(&h, &t, eur("fav", 100)).await;
    let page = send(
        &h.app,
        "GET",
        &format!("/pay/{}", p["id"].as_str().unwrap()),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert!(page.text.contains("rel=\"icon\""));
}

#[tokio::test]
async fn default_branding_shows_powered_by_and_has_no_receipt_yet() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = new_payment(&h, &t, eur("b1", 100)).await;
    let s = state(&h, p["id"].as_str().unwrap()).await;
    let b = &s["branding"];
    assert_eq!(b["theme"], "auto");
    assert_eq!(b["radius"], "md");
    assert_eq!(b["font"], "system");
    assert_eq!(b["show_powered_by"], true);
    assert_eq!(b["footer_links"], json!([]));
    assert!(s["receipt_url"].is_null());
    assert!(s["server_time"].is_string());
}

#[tokio::test]
async fn branding_is_saved_validated_and_shown_in_the_pay_page_state() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = new_payment(&h, &t, eur("b2", 100)).await;
    let id = p["id"].as_str().unwrap();
    let (st, r) = set_branding(
        &h,
        &t,
        json!({
            "default_lang": "ga", "theme": "dark", "radius": "lg", "font": "serif",
            "accent_dark": "#88ccdd", "thanks_text": "Go raibh maith agat",
            "footer_links": [{"label": "Terms", "url": "https://shop.example/terms"}]
        }),
    )
    .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let b = state(&h, id).await["branding"].clone();
    assert_eq!(b["default_lang"], "ga");
    assert_eq!(b["theme"], "dark");
    assert_eq!(b["thanks_text"], "Go raibh maith agat");
    assert_eq!(b["footer_links"][0]["label"], "Terms");

    for bad in [
        json!({"theme": "neon"}),
        json!({"radius": "50%; x"}),
        json!({"font": "Papyrus"}),
        json!({"default_lang": "xx"}),
        json!({"accent_dark": "red"}),
        json!({"footer_links": [{"label": "x", "url": "javascript:alert(1)"}]}),
    ] {
        let (st, _) = set_branding(&h, &t, bad.clone()).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn branding_belongs_to_one_tenant() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    set_branding(&h, &a, json!({"thanks_text": "A only", "theme": "light"})).await;
    let pb = new_payment(&h, &b, eur("iso", 100)).await;
    let s = state(&h, pb["id"].as_str().unwrap()).await;
    assert!(s["branding"]["thanks_text"].is_null());
    assert_eq!(s["branding"]["theme"], "auto");
}

#[tokio::test]
async fn only_operators_may_hide_powered_by() {
    let h = harness().await;
    let owner = Login::new(&h.app, &h.pool, "owner@example.com", false).await;
    let op = Login::new(&h.app, &h.pool, "op@example.com", true).await;
    let created = owner
        .req(&h.app, "POST", "/projects", json!({"name": "branded"}))
        .await;
    assert_eq!(created.status, StatusCode::OK, "{}", created.text);
    let pid = created.json["id"].as_u64().unwrap();
    let key = created.json["api_key"].as_str().unwrap().to_string();
    let path = format!("/projects/{pid}/branding");

    let r = owner
        .req(
            &h.app,
            "POST",
            &path,
            json!({"show_powered_by": false, "thanks_text": "Thanks!"}),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["show_powered_by"], true);
    assert_eq!(r.json["can_hide_powered_by"], false);
    assert_eq!(r.json["thanks_text"], "Thanks!");

    let r = op
        .req(&h.app, "POST", &path, json!({"show_powered_by": false}))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["show_powered_by"], false);

    let again = owner
        .req(&h.app, "POST", &path, json!({"thanks_text": "Updated"}))
        .await;
    assert_eq!(again.json["show_powered_by"], false);

    let (_, payment) = call(&h.app, "POST", "/v1/payments", Some(&key), eur("po", 100)).await;
    let s = state(&h, payment["id"].as_str().unwrap()).await;
    assert_eq!(s["branding"]["show_powered_by"], false);
    assert_eq!(s["branding"]["thanks_text"], "Updated");
}

#[tokio::test]
async fn another_account_cannot_read_or_set_branding() {
    let h = harness().await;
    let a = Login::new(&h.app, &h.pool, "a@example.com", false).await;
    let b = Login::new(&h.app, &h.pool, "b@example.com", false).await;
    let created = a
        .req(&h.app, "POST", "/projects", json!({"name": "mine"}))
        .await;
    let pid = created.json["id"].as_u64().unwrap();
    let path = format!("/projects/{pid}/branding");
    let get = b.req(&h.app, "GET", &path, Value::Null).await;
    assert_eq!(get.status, StatusCode::NOT_FOUND);
    let set = b
        .req(&h.app, "POST", &path, json!({"thanks_text": "hijack"}))
        .await;
    assert_eq!(set.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn onchain_attempt_reports_received_and_confirmation_target_for_underpayment() {
    let h = harness().await;
    let t = tenant(&h).await;
    let oc = provider(&h, &t, "onchain", json!({"min_confirmations": 3}), None).await;
    let p = new_payment(
        &h,
        &t,
        json!({"reference": "u", "amount": 5_000_000, "currency": "BTC"}),
    )
    .await;
    let id = p["id"].as_str().unwrap();
    let (st, r) = attempt(&h, &t, id, json!({"method": "onchain"})).await;
    assert!(st.is_success(), "{r}");
    let address = r["attempts"][0]["details"]["address"]
        .as_str()
        .unwrap()
        .to_string();

    let before = state(&h, id).await;
    let prog = &before["attempts"][0]["progress"];
    assert_eq!(prog["received"], 0);
    assert_eq!(prog["required"], 5_000_000);
    assert_eq!(prog["min_confirmations"], 3);

    ProviderSink {
        service: h.service.clone(),
        provider_id: oc,
    }
    .event(
        ProviderEvent::Paid {
            lookup: address.clone(),
            reference: "tx1:0".into(),
            amount: Some(2_000_000),
            confirmed: false,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();
    let after = state(&h, id).await;
    assert_eq!(after["status"], "pending");
    let prog = &after["attempts"][0]["progress"];
    assert_eq!(prog["received"], 2_000_000);
    assert_eq!(prog["confirmed"], 0);
    assert_eq!(
        after["attempts"][0]["details"]["address"].as_str().unwrap(),
        address
    );
}

fn read_lang(lang: &str) -> serde_json::Map<String, Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("web/public/i18n")
        .join(format!("{lang}.json"));
    let raw = std::fs::read_to_string(path).unwrap();
    serde_json::from_str::<Value>(&raw)
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn every_translation_has_the_same_keys_and_none_is_unused() {
    let en = read_lang("en");
    for lang in payment_gateway::branding::LANGUAGES {
        let other = read_lang(lang);
        for k in en.keys() {
            assert!(other.contains_key(k), "{lang} is missing {k}");
        }
        for k in other.keys() {
            assert!(en.contains_key(k), "{lang} has extra {k}");
        }
        for v in other.values() {
            let s = v.as_str().unwrap();
            assert!(
                !s.contains('\u{2014}') && !s.contains('\u{2013}'),
                "{lang}: {s}"
            );
        }
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("web/src/pay");
    let js = ["App.tsx", "revolut.tsx"]
        .iter()
        .map(|f| std::fs::read_to_string(dir.join(f)).unwrap())
        .collect::<String>();
    for key in en.keys() {
        let used = js.contains(&format!("'{key}'")) || key.starts_with("method.");
        assert!(used, "unused translation key {key}");
    }
    let mut cursor = 0;
    while let Some(i) = js[cursor..].find("t('") {
        let start = cursor + i + 3;
        let end = start + js[start..].find('\'').unwrap();
        let key = &js[start..end];
        let before = js[..start - 3].chars().last().unwrap_or(' ');
        if before.is_alphanumeric() || before == '_' || before == '.' {
            cursor = end;
            continue;
        }
        assert!(
            key.ends_with('.') || en.contains_key(key),
            "pay app uses unknown key {key}"
        );
        cursor = end;
    }
}

#[test]
fn placeholders_match_across_languages() {
    let slots = |s: &str| -> Vec<String> {
        let mut v: Vec<String> = s
            .split('{')
            .skip(1)
            .filter_map(|r| r.split('}').next().map(String::from))
            .collect();
        v.sort();
        v
    };
    let en = read_lang("en");
    for lang in payment_gateway::branding::LANGUAGES {
        let other = read_lang(lang);
        for (k, v) in &en {
            assert_eq!(
                slots(v.as_str().unwrap()),
                slots(other[k].as_str().unwrap()),
                "{lang}:{k}"
            );
        }
    }
}
