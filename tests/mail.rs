mod common;
use axum::Router;
use axum::http::StatusCode;
use chrono::{Duration, Utc};
use common::*;
use payment_gateway::accounts;
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::ProviderSink;
use serde_json::{Value, json};
use std::time::Instant;

const PW: &str = "correct horse battery";

async fn post(app: &Router, path: &str, body: Value) -> Reply {
    send(app, "POST", &format!("/console/api{path}"), None, &[], body).await
}

async fn post_from(app: &Router, path: &str, ip: &str, body: Value) -> Reply {
    send(
        app,
        "POST",
        &format!("/console/api{path}"),
        None,
        &[("x-forwarded-for", ip)],
        body,
    )
    .await
}

async fn account(h: &Harness, email: &str, operator: bool) -> u64 {
    let hash = accounts::hash_password(PW).unwrap();
    accounts::insert_account(&h.pool, email, &hash, operator)
        .await
        .unwrap()
}

async fn login(app: &Router, email: &str, password: &str) -> Reply {
    post(app, "/login", json!({"email": email, "password": password})).await
}

fn token_in(text: &str) -> String {
    let start = text.find("/console/").expect("link");
    text[start..]
        .split_whitespace()
        .next()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .trim_end_matches(['"', '<'])
        .to_string()
}

async fn reset_token(h: &Harness, email: &str) -> String {
    h.mailer.clear();
    let r = post(&h.app, "/reset", json!({"email": email})).await;
    assert_eq!(r.status, StatusCode::OK);
    let sent = h.mailer.wait_for(1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, email);
    token_in(&sent[0].text)
}

#[tokio::test]
async fn reset_happy_path_sets_password_and_signs_out_everywhere() {
    let h = harness_with_mail().await;
    account(&h, "a@example.com", false).await;
    let first = login(&h.app, "a@example.com", PW).await;
    let cookie = first.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let me = send(
        &h.app,
        "GET",
        "/console/api/me",
        None,
        &[("cookie", &cookie)],
        Value::Null,
    )
    .await;
    assert_eq!(me.status, StatusCode::OK);

    let token = reset_token(&h, "a@example.com").await;
    let stale = reset_token(&h, "a@example.com").await;
    let r = post(
        &h.app,
        "/reset/confirm",
        json!({"token": token, "password": "a brand new password"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);

    let me = send(
        &h.app,
        "GET",
        "/console/api/me",
        None,
        &[("cookie", &cookie)],
        Value::Null,
    )
    .await;
    assert_eq!(me.status, StatusCode::UNAUTHORIZED, "sessions revoked");
    assert_eq!(
        login(&h.app, "a@example.com", PW).await.status,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        login(&h.app, "a@example.com", "a brand new password")
            .await
            .status,
        StatusCode::OK
    );
    let r = post(
        &h.app,
        "/reset/confirm",
        json!({"token": stale, "password": "another new password"}),
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::BAD_REQUEST,
        "other tokens invalidated"
    );
}

#[tokio::test]
async fn reset_tokens_are_stored_hashed() {
    let h = harness_with_mail().await;
    account(&h, "a@example.com", false).await;
    let token = reset_token(&h, "a@example.com").await;
    assert_eq!(token.len(), 64);
    let rows: Vec<String> = sqlx::query_scalar("select token_hash from account_token")
        .fetch_all(&h.pool)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_ne!(rows[0], token);
    assert_eq!(rows[0], payment_gateway::crypto::hash_api_key(&token));
}

#[tokio::test]
async fn expired_reset_token_is_rejected() {
    let h = harness_with_mail().await;
    account(&h, "a@example.com", false).await;
    let token = reset_token(&h, "a@example.com").await;
    sqlx::query("update account_token set expires = ?")
        .bind(Utc::now() - Duration::minutes(1))
        .execute(&h.pool)
        .await
        .unwrap();
    let r = post(
        &h.app,
        "/reset/confirm",
        json!({"token": token, "password": "a brand new password"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        login(&h.app, "a@example.com", PW).await.status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn reset_token_cannot_be_reused() {
    let h = harness_with_mail().await;
    account(&h, "a@example.com", false).await;
    let token = reset_token(&h, "a@example.com").await;
    let body = json!({"token": token, "password": "a brand new password"});
    assert_eq!(
        post(&h.app, "/reset/confirm", body.clone()).await.status,
        StatusCode::OK
    );
    let again = post(
        &h.app,
        "/reset/confirm",
        json!({"token": token, "password": "yet another password"}),
    )
    .await;
    assert_eq!(again.status, StatusCode::BAD_REQUEST);
    assert_eq!(
        login(&h.app, "a@example.com", "a brand new password")
            .await
            .status,
        StatusCode::OK
    );
}

#[tokio::test]
async fn bogus_and_short_password_confirms_are_rejected() {
    let h = harness_with_mail().await;
    account(&h, "a@example.com", false).await;
    let r = post(
        &h.app,
        "/reset/confirm",
        json!({"token": "nope", "password": "a brand new password"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let token = reset_token(&h, "a@example.com").await;
    let r = post(
        &h.app,
        "/reset/confirm",
        json!({"token": token, "password": "short"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    let r = post(
        &h.app,
        "/reset/confirm",
        json!({"token": token, "password": "a brand new password"}),
    )
    .await;
    assert_eq!(
        r.status,
        StatusCode::OK,
        "a weak password does not burn the token"
    );
}

#[tokio::test]
async fn unknown_email_gets_the_same_response_and_timing_class() {
    let h = harness_with_mail().await;
    account(&h, "known@example.com", false).await;
    h.mailer.set_delay(std::time::Duration::from_millis(400));

    let t = Instant::now();
    let known = post_from(
        &h.app,
        "/reset",
        "1.1.1.1",
        json!({"email": "known@example.com"}),
    )
    .await;
    let known_time = t.elapsed();
    let t = Instant::now();
    let unknown = post_from(
        &h.app,
        "/reset",
        "1.1.1.2",
        json!({"email": "nobody@example.com"}),
    )
    .await;
    let unknown_time = t.elapsed();

    assert_eq!(known.status, unknown.status);
    assert_eq!(known.json, unknown.json);
    assert_eq!(known.text, unknown.text);
    assert!(
        known_time < std::time::Duration::from_millis(300),
        "{known_time:?}"
    );
    assert!(
        unknown_time < std::time::Duration::from_millis(300),
        "{unknown_time:?}"
    );
    let bad = post_from(
        &h.app,
        "/reset",
        "1.1.1.3",
        json!({"email": "not an email"}),
    )
    .await;
    assert_eq!((bad.status, bad.text), (known.status, known.text));
    let sent = h.mailer.wait_for(1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "known@example.com");
}

#[tokio::test]
async fn reset_requests_are_rate_limited() {
    let h = harness_with_mail().await;
    account(&h, "a@example.com", false).await;
    let mut statuses = vec![];
    for i in 0..7 {
        let r = post_from(
            &h.app,
            "/reset",
            &format!("9.9.9.{i}"),
            json!({"email": "a@example.com"}),
        )
        .await;
        statuses.push(r.status);
    }
    assert_eq!(statuses[0], StatusCode::OK);
    assert_eq!(statuses[6], StatusCode::TOO_MANY_REQUESTS);
    let unknown: Vec<_> = {
        let mut v = vec![];
        for _ in 0..7 {
            v.push(
                post_from(
                    &h.app,
                    "/reset",
                    "8.8.8.8",
                    json!({"email": "ghost@example.com"}),
                )
                .await
                .status,
            );
        }
        v
    };
    assert_eq!(
        unknown[6],
        StatusCode::TOO_MANY_REQUESTS,
        "same limit for unknown emails"
    );
}

#[tokio::test]
async fn reset_with_mail_disabled_answers_the_same_and_sends_nothing() {
    let h = harness().await;
    account(&h, "a@example.com", false).await;
    let r = post(&h.app, "/reset", json!({"email": "a@example.com"})).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(h.mailer.sent().is_empty());
    let n: i64 = sqlx::query_scalar("select count(*) from account_token")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn disabled_accounts_get_no_reset_mail() {
    let h = harness_with_mail().await;
    let id = account(&h, "a@example.com", false).await;
    accounts::set_account_disabled(&h.pool, id, true)
        .await
        .unwrap();
    let r = post(&h.app, "/reset", json!({"email": "a@example.com"})).await;
    assert_eq!(r.status, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(h.mailer.sent().is_empty());
}

#[tokio::test]
async fn operator_invite_sends_an_email_and_still_returns_the_link() {
    let h = harness_with_mail().await;
    let (s, inv) = call(
        &h.app,
        "POST",
        "/admin/invites",
        Some(ADMIN),
        json!({"email": "New@Example.com"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{inv}");
    let url = inv["invite_url"].as_str().unwrap();
    let sent = h.mailer.wait_for(1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "new@example.com");
    assert!(sent[0].text.contains(url) && sent[0].html.contains(url));
    assert!(!sent[0].html.contains("http://") || !sent[0].html.contains("<img"));
}

#[tokio::test]
async fn team_invite_sends_an_email_and_disabled_mail_sends_none() {
    let h = harness_with_mail().await;
    account(&h, "owner@example.com", false).await;
    let r = login(&h.app, "owner@example.com", PW).await;
    let cookie = r.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = r.json["csrf"].as_str().unwrap().to_string();
    let hdrs = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let p = send(
        &h.app,
        "POST",
        "/console/api/projects",
        None,
        &hdrs,
        json!({"name": "Shop"}),
    )
    .await;
    let pid = p.json["id"].as_u64().unwrap();
    let r = send(
        &h.app,
        "POST",
        &format!("/console/api/projects/{pid}/invites"),
        None,
        &hdrs,
        json!({"email": "mate@example.com", "role": "viewer"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let sent = h.mailer.wait_for(1).await;
    assert_eq!(sent[0].to, "mate@example.com");
    assert!(
        sent[0]
            .text
            .contains(r.json["invite_url"].as_str().unwrap())
    );
    assert!(sent[0].text.contains("Shop"));

    let off = harness().await;
    let (s, inv) = call(
        &off.app,
        "POST",
        "/admin/invites",
        Some(ADMIN),
        json!({"email": "x@example.com"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert!(inv["invite_url"].as_str().is_some());
    assert!(off.mailer.sent().is_empty());
}

#[tokio::test]
async fn email_change_needs_confirmation_by_the_new_address() {
    let h = harness_with_mail().await;
    account(&h, "old@example.com", false).await;
    let r = login(&h.app, "old@example.com", PW).await;
    let cookie = r.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = r.json["csrf"].as_str().unwrap().to_string();
    let hdrs = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];

    let bad = send(
        &h.app,
        "POST",
        "/console/api/email",
        None,
        &hdrs,
        json!({"password": "wrong password!", "new_email": "new@example.com"}),
    )
    .await;
    assert_eq!(bad.status, StatusCode::FORBIDDEN);
    let ok = send(
        &h.app,
        "POST",
        "/console/api/email",
        None,
        &hdrs,
        json!({"password": PW, "new_email": "New@Example.com"}),
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{}", ok.text);
    let sent = h.mailer.wait_for(1).await;
    assert_eq!(sent[0].to, "new@example.com");
    assert_eq!(
        login(&h.app, "old@example.com", PW).await.status,
        StatusCode::OK,
        "not yet changed"
    );
    assert_eq!(
        login(&h.app, "new@example.com", PW).await.status,
        StatusCode::UNAUTHORIZED
    );

    let token = token_in(&sent[0].text);
    let confirm = post(&h.app, "/email/confirm", json!({"token": token})).await;
    assert_eq!(confirm.status, StatusCode::OK, "{}", confirm.text);
    assert_eq!(
        login(&h.app, "new@example.com", PW).await.status,
        StatusCode::OK
    );
    assert_eq!(
        login(&h.app, "old@example.com", PW).await.status,
        StatusCode::UNAUTHORIZED
    );
    let again = post(&h.app, "/email/confirm", json!({"token": token})).await;
    assert_eq!(again.status, StatusCode::BAD_REQUEST);
    let all = h.mailer.wait_for(2).await;
    assert_eq!(all[1].to, "old@example.com", "old address is told");
}

#[tokio::test]
async fn email_change_to_a_taken_address_is_refused_and_reset_token_is_not_an_email_token() {
    let h = harness_with_mail().await;
    account(&h, "a@example.com", false).await;
    account(&h, "b@example.com", false).await;
    let r = login(&h.app, "a@example.com", PW).await;
    let cookie = r.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = r.json["csrf"].as_str().unwrap().to_string();
    let hdrs = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let r = send(
        &h.app,
        "POST",
        "/console/api/email",
        None,
        &hdrs,
        json!({"password": PW, "new_email": "b@example.com"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    let token = reset_token(&h, "a@example.com").await;
    let r = post(&h.app, "/email/confirm", json!({"token": token})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

async fn owner_with_project(h: &Harness, email: &str, notify: bool) -> (u64, TenantFixture) {
    let id = account(h, email, false).await;
    let t = tenant(h).await;
    accounts::add_member(&h.pool, t.id, id, accounts::Role::Owner)
        .await
        .unwrap();
    if !notify {
        accounts::set_notify(&h.pool, id, false).await.unwrap();
    }
    (id, t)
}

async fn paid_and_failing_hook(h: &Harness, t: &TenantFixture) {
    let ln = provider(h, t, "lightning", json!({}), None).await;
    t.receiver.fail_with(500);
    let p = new_payment(h, t, eur("o", 1000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: h.service.clone(),
        provider_id: ln,
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
}

async fn abandon_all(h: &Harness) {
    sqlx::query("update webhook_delivery set created = ?, next_attempt = ?, abandoned_at = null")
        .bind(Utc::now() - Duration::days(4))
        .bind(Utc::now() - Duration::seconds(1))
        .execute(&h.pool)
        .await
        .unwrap();
    h.service.deliver_due().await.unwrap();
}

#[tokio::test]
async fn abandoned_webhooks_notify_owners_once_per_hour() {
    let h = harness_with_mail().await;
    let (_, t) = owner_with_project(&h, "owner@example.com", true).await;
    let viewer = account(&h, "viewer@example.com", false).await;
    accounts::add_member(&h.pool, t.id, viewer, accounts::Role::Viewer)
        .await
        .unwrap();
    paid_and_failing_hook(&h, &t).await;
    paid_and_failing_hook(&h, &t).await;
    abandon_all(&h).await;
    let sent = h.mailer.wait_for(1).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let sent2 = h.mailer.sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent2.len(), 1, "two abandoned deliveries, one email");
    assert_eq!(sent2[0].to, "owner@example.com");
    assert!(sent2[0].subject.contains("Webhook delivery abandoned"));

    paid_and_failing_hook(&h, &t).await;
    abandon_all(&h).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.mailer.sent().len(), 1, "throttled within the hour");

    sqlx::query("update notification_throttle set sent_at = ?")
        .bind(Utc::now() - Duration::minutes(61))
        .execute(&h.pool)
        .await
        .unwrap();
    paid_and_failing_hook(&h, &t).await;
    abandon_all(&h).await;
    assert_eq!(
        h.mailer.wait_for(2).await.len(),
        2,
        "allowed again after an hour"
    );
}

#[tokio::test]
async fn throttle_is_per_event_type_and_per_project() {
    let h = harness_with_mail().await;
    let (_, a) = owner_with_project(&h, "a@example.com", true).await;
    let (_, b) = owner_with_project(&h, "b@example.com", true).await;
    h.service
        .notify_owners(a.id, "webhook_abandoned", "s", "d")
        .await
        .unwrap();
    h.service
        .notify_owners(a.id, "refund_failed", "s", "d")
        .await
        .unwrap();
    h.service
        .notify_owners(b.id, "webhook_abandoned", "s", "d")
        .await
        .unwrap();
    h.service
        .notify_owners(a.id, "webhook_abandoned", "s", "d")
        .await
        .unwrap();
    let sent = h.mailer.wait_for(3).await;
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert_eq!(h.mailer.sent().len(), 3, "{sent:?}");
}

#[tokio::test]
async fn owners_who_turned_notifications_off_get_nothing() {
    let h = harness_with_mail().await;
    let (_, t) = owner_with_project(&h, "quiet@example.com", false).await;
    paid_and_failing_hook(&h, &t).await;
    abandon_all(&h).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(h.mailer.sent().is_empty());
}

#[tokio::test]
async fn notification_setting_is_changed_through_the_console() {
    let h = harness_with_mail().await;
    let (_, t) = owner_with_project(&h, "owner@example.com", true).await;
    let r = login(&h.app, "owner@example.com", PW).await;
    assert_eq!(r.json["notify"], true);
    let cookie = r.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = r.json["csrf"].as_str().unwrap().to_string();
    let hdrs = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    let r = send(
        &h.app,
        "POST",
        "/console/api/notifications",
        None,
        &hdrs,
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let me = send(&h.app, "GET", "/console/api/me", None, &hdrs, Value::Null).await;
    assert_eq!(me.json["notify"], false);
    paid_and_failing_hook(&h, &t).await;
    abandon_all(&h).await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(h.mailer.sent().is_empty());
}

#[tokio::test]
async fn failed_refunds_notify_owners_but_only_other_projects_owners_are_not_told() {
    let h = harness_with_mail().await;
    let (_, t) = owner_with_project(&h, "owner@example.com", true).await;
    let (_, other) = owner_with_project(&h, "other@example.com", true).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let p = new_payment(&h, &t, eur("o", 5000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(&h, &t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: h.service.clone(),
        provider_id: ln,
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
    *mock(&h, ln).fail_refund.lock().unwrap() = true;
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/refund"),
        Some(&t.key),
        json!({}),
    )
    .await;
    assert_eq!((s, r["status"].as_str()), (StatusCode::OK, Some("failed")));
    let sent = h.mailer.wait_for(1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "owner@example.com");
    assert!(sent[0].subject.contains("Refund failed"));
    let _ = other;
}

#[tokio::test]
async fn failing_providers_notify_after_repeated_failures() {
    let h = harness_with_mail().await;
    let (_, t) = owner_with_project(&h, "owner@example.com", true).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    h.service.provider_failed(ln, "boom").await;
    h.service.provider_failed(ln, "boom").await;
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(h.mailer.sent().is_empty());
    h.service.provider_failed(ln, "boom").await;
    h.service.provider_failed(ln, "boom").await;
    let sent = h.mailer.wait_for(1).await;
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    assert_eq!(h.mailer.sent().len(), 1);
    assert!(sent[0].subject.contains("Provider failing"));
    h.service.provider_recovered(ln);
}

async fn audit_actions(h: &Harness) -> Vec<String> {
    let mut found: Vec<String> = payment_gateway::audit::list(
        &h.pool,
        &payment_gateway::audit::Filter {
            limit: Some(200),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .into_iter()
    .map(|e| e.action)
    .collect();
    found.sort();
    found
}

#[tokio::test]
async fn account_mail_actions_are_audited_without_tokens() {
    let h = harness_with_mail().await;
    account(&h, "a@example.com", false).await;
    let ghost = post(&h.app, "/reset", json!({"email": "ghost@example.com"})).await;
    assert_eq!(ghost.status, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(
        audit_actions(&h).await.is_empty(),
        "unknown emails leave no trace"
    );

    let reset = reset_token(&h, "a@example.com").await;
    let r = post(
        &h.app,
        "/reset/confirm",
        json!({"token": reset, "password": "a brand new password"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);

    let r = login(&h.app, "a@example.com", "a brand new password").await;
    let cookie = r.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let csrf = r.json["csrf"].as_str().unwrap().to_string();
    let hdrs = [("cookie", cookie.as_str()), ("x-csrf-token", csrf.as_str())];
    h.mailer.clear();
    let r = send(
        &h.app,
        "POST",
        "/console/api/email",
        None,
        &hdrs,
        json!({"password": "a brand new password", "new_email": "b@example.com"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    let change = token_in(&h.mailer.wait_for(1).await[0].text);
    let r = post(&h.app, "/email/confirm", json!({"token": change})).await;
    assert_eq!(r.status, StatusCode::OK);
    let r = send(
        &h.app,
        "POST",
        "/console/api/notifications",
        None,
        &hdrs,
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);

    let actions = audit_actions(&h).await;
    for expected in [
        "password.reset_requested",
        "password.reset",
        "email.change_requested",
        "email.changed",
        "notifications.changed",
    ] {
        assert_eq!(
            actions.iter().filter(|a| *a == expected).count(),
            1,
            "{expected} in {actions:?}"
        );
    }
    let events = payment_gateway::audit::list(&h.pool, &Default::default())
        .await
        .unwrap();
    let details =
        serde_json::to_string(&events.iter().map(|e| &e.details).collect::<Vec<_>>()).unwrap();
    assert!(!details.contains("b@example.com"), "{details}");
    let dump = serde_json::to_string(&events).unwrap();
    for secret in [
        reset.as_str(),
        change.as_str(),
        payment_gateway::crypto::hash_api_key(&reset).as_str(),
        "a brand new password",
    ] {
        assert!(!dump.contains(secret), "{secret} leaked into audit");
    }
}
