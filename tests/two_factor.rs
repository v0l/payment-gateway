mod common;
use axum::Router;
use axum::http::StatusCode;
use common::*;
use payment_gateway::{accounts, api, totp, twofactor};
use serde_json::{Value, json};

const PW: &str = "correct horse battery";

struct Client {
    app: Router,
    cookie: String,
    csrf: String,
}

impl Client {
    fn anon(app: &Router) -> Self {
        Self {
            app: app.clone(),
            cookie: String::new(),
            csrf: String::new(),
        }
    }

    async fn req(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let mut headers: Vec<(&str, String)> = vec![];
        if !self.cookie.is_empty() {
            headers.push(("cookie", format!("pgw_session={}", self.cookie)));
            headers.push(("x-csrf-token", self.csrf.clone()));
        }
        let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let r = send(
            &self.app,
            method,
            &format!("/console/api{path}"),
            None,
            &h,
            body,
        )
        .await;
        (r.status, r.json)
    }

    async fn adopt(app: &Router, r: &Reply) -> Self {
        let mut c = Self::anon(app);
        let set = r.headers["set-cookie"].to_str().unwrap();
        c.cookie = set
            .split(';')
            .next()
            .unwrap()
            .trim_start_matches("pgw_session=")
            .to_string();
        c.csrf = r.json["csrf"].as_str().unwrap().to_string();
        c
    }
}

async fn password_login(app: &Router, email: &str, password: &str) -> Reply {
    send(
        app,
        "POST",
        "/console/api/login",
        None,
        &[],
        json!({"email": email, "password": password}),
    )
    .await
}

async fn second_step(app: &Router, challenge: &str, body: Value) -> Reply {
    let mut b = body;
    b["challenge"] = json!(challenge);
    send(app, "POST", "/console/api/login/2fa", None, &[], b).await
}

async fn make_account(h: &Harness, email: &str, operator: bool) -> u64 {
    accounts::insert_account(
        &h.pool,
        email,
        &accounts::hash_password(PW).unwrap(),
        operator,
    )
    .await
    .unwrap()
}

async fn signed_in(h: &Harness, email: &str) -> (Client, u64) {
    let id = make_account(h, email, false).await;
    let r = password_login(&h.app, email, PW).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    (Client::adopt(&h.app, &r).await, id)
}

fn now() -> u64 {
    chrono::Utc::now().timestamp() as u64
}

fn code_at(secret: &[u8], offset: i64) -> String {
    totp::code_for_step(secret, (totp::step_at(now()) as i64 + offset) as u64)
}

async fn enrol(h: &Harness, c: &Client, id: u64) -> (Vec<u8>, Vec<String>) {
    let (s, setup) = c.req("POST", "/2fa/setup", json!({})).await;
    assert_eq!(s, StatusCode::OK, "{setup}");
    let secret = twofactor::pending_secret(&h.pool, &h.service.sealer, id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(setup["secret"], totp::base32(&secret));
    assert!(
        setup["otpauth_uri"]
            .as_str()
            .unwrap()
            .starts_with("otpauth://totp/")
    );
    assert!(setup["qr_svg"].as_str().unwrap().contains("<svg"));
    let (s, act) = c
        .req(
            "POST",
            "/2fa/activate",
            json!({"code": code_at(&secret, -1)}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{act}");
    let codes = act["recovery_codes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap().to_string())
        .collect();
    (secret, codes)
}

#[tokio::test]
async fn enrol_activate_then_login_needs_a_second_step() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    let (s, st) = c.req("GET", "/2fa", Value::Null).await;
    assert_eq!((s, st["enabled"].clone()), (StatusCode::OK, json!(false)));
    let (secret, codes) = enrol(&h, &c, id).await;
    assert_eq!(codes.len(), 10);
    let (_, st) = c.req("GET", "/2fa", Value::Null).await;
    assert_eq!(st["enabled"], true);
    assert_eq!(st["recovery_codes_remaining"], 10);

    let r = password_login(&h.app, "a@example.com", PW).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.headers.get("set-cookie").is_none());
    assert_eq!(r.json["two_factor"], true);
    let challenge = r.json["challenge"].as_str().unwrap().to_string();

    let ok = second_step(&h.app, &challenge, json!({"code": code_at(&secret, 0)})).await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    assert!(ok.headers.get("set-cookie").is_some());
    let c2 = Client::adopt(&h.app, &ok).await;
    assert_eq!(c2.req("GET", "/me", Value::Null).await.0, StatusCode::OK);

    let again = second_step(&h.app, &challenge, json!({"code": code_at(&secret, 1)})).await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn wrong_code_is_rejected_and_setup_must_precede_activation() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    let (s, _) = c
        .req("POST", "/2fa/activate", json!({"code": "123456"}))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    c.req("POST", "/2fa/setup", json!({})).await;
    let secret = twofactor::pending_secret(&h.pool, &h.service.sealer, id)
        .await
        .unwrap()
        .unwrap();
    let wrong = if code_at(&secret, 0) == "000000" {
        "000001"
    } else {
        "000000"
    };
    let (s, _) = c.req("POST", "/2fa/activate", json!({"code": wrong})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (_, st) = c.req("GET", "/2fa", Value::Null).await;
    assert_eq!(st["enabled"], false);
}

#[tokio::test]
async fn replayed_code_is_rejected() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    let (secret, _) = enrol(&h, &c, id).await;
    let code = code_at(&secret, 0);
    let ch1 = password_login(&h.app, "a@example.com", PW).await.json["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let ok = second_step(&h.app, &ch1, json!({"code": code})).await;
    assert_eq!(ok.status, StatusCode::OK);
    let ch2 = password_login(&h.app, "a@example.com", PW).await.json["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let replay = second_step(&h.app, &ch2, json!({"code": code})).await;
    assert_eq!(replay.status, StatusCode::UNAUTHORIZED);
    let older = second_step(&h.app, &ch2, json!({"code": code_at(&secret, -1)})).await;
    assert_eq!(older.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn one_step_of_drift_is_accepted_and_two_rejected() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    let (secret, _) = enrol(&h, &c, id).await;
    let ch = password_login(&h.app, "a@example.com", PW).await.json["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    for offset in [-2, 2] {
        let r = second_step(&h.app, &ch, json!({"code": code_at(&secret, offset)})).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED, "offset {offset}");
    }
    let r = second_step(&h.app, &ch, json!({"code": code_at(&secret, 1)})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
}

#[tokio::test]
async fn recovery_code_works_once() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    let (_, codes) = enrol(&h, &c, id).await;
    let ch = password_login(&h.app, "a@example.com", PW).await.json["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let ok = second_step(
        &h.app,
        &ch,
        json!({"recovery_code": codes[0].to_uppercase()}),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    let ch = password_login(&h.app, "a@example.com", PW).await.json["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let again = second_step(&h.app, &ch, json!({"recovery_code": codes[0]})).await;
    assert_eq!(again.status, StatusCode::UNAUTHORIZED);
    let other = second_step(&h.app, &ch, json!({"recovery_code": codes[1]})).await;
    assert_eq!(other.status, StatusCode::OK);
    let (_, st) = Client::adopt(&h.app, &other)
        .await
        .req("GET", "/2fa", Value::Null)
        .await;
    assert_eq!(st["recovery_codes_remaining"], 8);
}

#[tokio::test]
async fn failed_codes_lock_out_through_the_limiter() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    let (secret, _) = enrol(&h, &c, id).await;
    let ch = password_login(&h.app, "a@example.com", PW).await.json["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    for _ in 0..8 {
        let r = second_step(&h.app, &ch, json!({"code": "000000"})).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    let r = second_step(&h.app, &ch, json!({"code": code_at(&secret, 1)})).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    let r = password_login(&h.app, "a@example.com", PW).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn challenge_token_is_not_a_session() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    enrol(&h, &c, id).await;
    let r = password_login(&h.app, "a@example.com", PW).await;
    let challenge = r.json["challenge"].as_str().unwrap();
    let fake = Client {
        app: h.app.clone(),
        cookie: challenge.to_string(),
        csrf: "x".into(),
    };
    assert_eq!(
        fake.req("GET", "/me", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        fake.req("GET", "/projects", Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn disable_and_regenerate_need_password_and_current_code() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    let (secret, codes) = enrol(&h, &c, id).await;
    let (s, _) = c
        .req(
            "POST",
            "/2fa/recovery-codes",
            json!({"password": "nope nope nope", "code": code_at(&secret, 1)}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = c
        .req(
            "POST",
            "/2fa/disable",
            json!({"password": PW, "code": "000000"}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, r) = c
        .req(
            "POST",
            "/2fa/recovery-codes",
            json!({"password": PW, "code": code_at(&secret, 0)}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let fresh = r["recovery_codes"].as_array().unwrap();
    assert_eq!(fresh.len(), 10);
    assert!(
        !fresh
            .iter()
            .any(|f| codes.contains(&f.as_str().unwrap().to_string()))
    );
    let ch = password_login(&h.app, "a@example.com", PW).await.json["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    let old = second_step(&h.app, &ch, json!({"recovery_code": codes[0]})).await;
    assert_eq!(old.status, StatusCode::UNAUTHORIZED);

    let (s, _) = c
        .req(
            "POST",
            "/2fa/disable",
            json!({"password": PW, "code": code_at(&secret, 0)}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "code already used in this step");
    let (s, r) = c
        .req(
            "POST",
            "/2fa/disable",
            json!({"password": PW, "code": code_at(&secret, 1)}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let r = password_login(&h.app, "a@example.com", PW).await;
    assert!(r.json.get("challenge").is_none());
    assert!(r.headers.get("set-cookie").is_some());
}

#[tokio::test]
async fn require_2fa_blocks_members_without_it() {
    let h = harness().await;
    let (owner, owner_id) = signed_in(&h, "owner@example.com").await;
    let (_, p) = owner
        .req("POST", "/projects", json!({"name": "shop"}))
        .await;
    let pid = p["id"].as_u64().unwrap();
    let (s, _) = owner
        .req(
            "POST",
            &format!("/projects/{pid}/require-2fa"),
            json!({"required": true}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "owner needs 2fa first");
    enrol(&h, &owner, owner_id).await;
    let (s, r) = owner
        .req(
            "POST",
            &format!("/projects/{pid}/require-2fa"),
            json!({"required": true}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");

    let (member, mid) = signed_in(&h, "m@example.com").await;
    accounts::add_member(&h.pool, pid, mid, accounts::Role::Admin)
        .await
        .unwrap();
    let (s, _) = member
        .req("GET", &format!("/projects/{pid}"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = member
        .req("POST", &format!("/projects/{pid}/rotate-key"), json!({}))
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    enrol(&h, &member, mid).await;
    let (s, _) = member
        .req("GET", &format!("/projects/{pid}"), Value::Null)
        .await;
    assert_eq!(s, StatusCode::OK);
    let (_, view) = owner
        .req("GET", &format!("/projects/{pid}"), Value::Null)
        .await;
    assert_eq!(view["require_2fa"], true);

    let (s, _) = member
        .req(
            "POST",
            &format!("/projects/{pid}/require-2fa"),
            json!({"required": false}),
        )
        .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "only owners set it");
}

#[tokio::test]
async fn operators_need_2fa_unless_grace_is_on() {
    let h = harness().await;
    make_account(&h, "op@example.com", true).await;
    let mut state = api::AppState::new(h.service.clone(), ADMIN.into(), true, None);
    state.operator_2fa_grace = false;
    let strict = api::router(state);
    let r = password_login(&strict, "op@example.com", PW).await;
    let c = Client::adopt(&strict, &r).await;
    let (s, _) = c.req("GET", "/admin/accounts", Value::Null).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = c.req("GET", "/2fa", Value::Null).await;
    assert_eq!(s, StatusCode::OK);

    let graced = Client::adopt(&h.app, &password_login(&h.app, "op@example.com", PW).await).await;
    let (s, _) = graced.req("GET", "/admin/accounts", Value::Null).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn secrets_are_never_returned_after_enrolment() {
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    let (secret, codes) = enrol(&h, &c, id).await;
    let b32 = totp::base32(&secret);
    let mut seen = vec![];
    for path in ["/2fa", "/me", "/projects"] {
        let (_, v) = c.req("GET", path, Value::Null).await;
        seen.push(v.to_string());
    }
    let (s, _) = c.req("POST", "/2fa/setup", json!({})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let ch = password_login(&h.app, "a@example.com", PW).await;
    seen.push(ch.text.clone());
    for body in seen {
        assert!(!body.contains(&b32), "{body}");
        assert!(!body.contains("otpauth"), "{body}");
        assert!(!body.contains("totp_secret"), "{body}");
        for code in &codes {
            assert!(!body.contains(code), "{body}");
        }
    }
    let sealed: Vec<u8> = sqlx::query_scalar("select totp_secret from account where id = ?")
        .bind(id)
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_ne!(sealed, secret);
    let stored: Vec<String> =
        sqlx::query_scalar("select code_hash from account_recovery_code where account_id = ?")
            .bind(id)
            .fetch_all(&h.pool)
            .await
            .unwrap();
    assert!(stored.iter().all(|s| !codes.contains(s)));
}

#[tokio::test]
async fn two_factor_state_is_per_account() {
    let h = harness().await;
    let (a, aid) = signed_in(&h, "a@example.com").await;
    let (b, _) = signed_in(&h, "b@example.com").await;
    enrol(&h, &a, aid).await;
    let (_, st) = b.req("GET", "/2fa", Value::Null).await;
    assert_eq!(st["enabled"], false);
    let r = password_login(&h.app, "b@example.com", PW).await;
    assert!(r.headers.get("set-cookie").is_some());
}

#[tokio::test]
async fn two_factor_events_are_audited_without_secrets() {
    use payment_gateway::audit::{self, Filter};
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    let (secret, codes) = enrol(&h, &c, id).await;
    let (_, p) = c.req("POST", "/projects", json!({"name": "shop"})).await;
    let pid = p["id"].as_u64().unwrap();
    c.req(
        "POST",
        &format!("/projects/{pid}/require-2fa"),
        json!({"required": true}),
    )
    .await;

    let ch = password_login(&h.app, "a@example.com", PW).await.json["challenge"]
        .as_str()
        .unwrap()
        .to_string();
    second_step(&h.app, &ch, json!({"code": "000000"})).await;
    second_step(&h.app, &ch, json!({"recovery_code": codes[0]})).await;
    c.req(
        "POST",
        "/2fa/recovery-codes",
        json!({"password": PW, "code": code_at(&secret, 1)}),
    )
    .await;
    let all = audit::list(
        &h.pool,
        &Filter {
            limit: Some(200),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let names: Vec<&str> = all.iter().map(|e| e.action.as_str()).collect();
    for want in [
        "2fa.enabled",
        "2fa.recovery_used",
        "2fa.recovery_regenerated",
        "login.2fa_failed",
        "project.require_2fa_changed",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    let dump = serde_json::to_string(&all).unwrap();
    assert!(!dump.contains(&totp::base32(&secret)));
    for code in &codes {
        assert!(!dump.contains(code), "recovery code leaked");
        assert!(!dump.contains(&totp::normalize_recovery_code(code)));
    }
    assert!(!dump.contains("000000"));
}

#[tokio::test]
async fn disabling_is_audited() {
    use payment_gateway::audit::{self, Filter};
    let h = harness().await;
    let (c, id) = signed_in(&h, "a@example.com").await;
    let (secret, _) = enrol(&h, &c, id).await;
    let (s, r) = c
        .req(
            "POST",
            "/2fa/disable",
            json!({"password": PW, "code": code_at(&secret, 1)}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let all = audit::list(
        &h.pool,
        &Filter {
            limit: Some(50),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert!(all.iter().any(|e| e.action == "2fa.disabled"));
}
