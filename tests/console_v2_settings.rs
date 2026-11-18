mod common;
use axum::http::StatusCode;
use common::console::*;
use common::*;
use payment_gateway::db;
use serde_json::{Value, json};

#[tokio::test]
async fn legal_details_and_notifications_roundtrip_with_validation() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    let base = format!("/projects/{}/settings", p.id);
    let (_, r) = c.req("GET", &base, Value::Null).await;
    assert_eq!(r["notify_refund_failed"], true);
    assert_eq!(r["allow_manual_paid"], false);
    let (s, r) = c
        .req("POST", &format!("{base}/legal"), json!({"name": "Shop BV", "vat_id": "NL123", "email": "billing@shop.test"}))
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["legal_name"], "Shop BV");
    let (s, _) = c.req("POST", &format!("{base}/legal"), json!({"email": "not an email"})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = c.req("POST", &format!("{base}/legal"), json!({"name": "x".repeat(200)})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, r) = c
        .req("POST", &format!("{base}/notifications"), json!({"webhook_abandoned": false, "provider_failing": true, "refund_failed": false}))
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["notify_webhook_abandoned"], false);
    assert_eq!(r["legal_name"], "Shop BV");
}

#[tokio::test]
async fn settings_are_tenant_scoped() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let pa = project(&a, "a").await;
    project(&b, "b").await;
    for (m, path, body) in [
        ("GET", "settings", Value::Null),
        ("POST", "settings/legal", json!({"name": "x"})),
        ("POST", "settings/notifications", json!({"webhook_abandoned": true, "provider_failing": true, "refund_failed": true})),
        ("POST", "settings/enabled", json!({"enabled": false})),
        ("POST", "delete-request", Value::Null),
    ] {
        let (s, _) = b.req(m, &format!("/projects/{}/{path}", pa.id), body).await;
        assert_eq!(s, StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn disabled_notification_kinds_send_no_mail() {
    let h = harness_with_mail().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    h.mailer.clear();
    c.req(
        "POST",
        &format!("/projects/{}/settings/notifications", p.id),
        json!({"webhook_abandoned": false, "provider_failing": true, "refund_failed": true}),
    )
    .await;
    h.service.notify_webhook_abandoned(p.id, "boom").await;
    assert!(h.mailer.sent().is_empty());
    h.service.notify_refund_failed(p.id, "boom").await;
    assert_eq!(h.mailer.wait_for(1).await.len(), 1);
}

#[tokio::test]
async fn disabling_a_project_blocks_its_api_key_and_can_be_undone() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    let path = format!("/projects/{}/settings/enabled", p.id);
    let (s, _) = c.req("POST", &path, json!({"enabled": false})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = call(&h.app, "POST", "/v1/payments", Some(&p.key), eur("x", 100)).await;
    assert_ne!(s, StatusCode::CREATED);
    c.req("POST", &path, json!({"enabled": true})).await;
    let (s, _) = call(&h.app, "POST", "/v1/payments", Some(&p.key), eur("x", 100)).await;
    assert_eq!(s, StatusCode::CREATED);
}

#[tokio::test]
async fn project_deletion_needs_token_cooling_off_and_no_pending_or_balance() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    let id = payment(&h, &p, eur("p", 100)).await;
    let req = format!("/projects/{}/delete-request", p.id);
    let confirm = format!("/projects/{}/delete-confirm", p.id);
    let (s, r) = c.req("POST", &req, Value::Null).await;
    assert_eq!(s, StatusCode::CONFLICT, "{r}");
    assert!(r["error"].as_str().unwrap().contains("pending"));
    c.req("POST", &format!("/projects/{}/payments/{id}/cancel", p.id), Value::Null).await;

    let (s, _) = c.req("POST", &confirm, json!({"token": "del_x"})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, r) = c.req("POST", &req, Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let token = r["token"].as_str().unwrap().to_string();
    let stored: Option<String> = sqlx::query_scalar("select delete_token_hash from project_config where tenant_id = ?")
        .bind(p.id).fetch_one(&h.pool).await.unwrap();
    assert_ne!(stored.unwrap(), token);

    let (s, _) = c.req("POST", &confirm, json!({"token": "del_wrong"})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, r) = c.req("POST", &confirm, json!({"token": token})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{r}");
    assert!(r["error"].as_str().unwrap().contains("cooling-off"));

    sqlx::query("update project_config set delete_requested_at = delete_requested_at - interval 25 hour where tenant_id = ?")
        .bind(p.id).execute(&h.pool).await.unwrap();
    sqlx::query("insert into ledger_account (tenant_id, kind, currency, balance, created) values (?, 'tenant_balance', 'EUR', 500, now())")
        .bind(p.id).execute(&h.pool).await.unwrap();
    let (s, r) = c.req("POST", &confirm, json!({"token": token})).await;
    assert_eq!(s, StatusCode::CONFLICT, "{r}");
    assert!(r["error"].as_str().unwrap().contains("balance"));
    sqlx::query("update ledger_account set balance = 0 where tenant_id = ?").bind(p.id).execute(&h.pool).await.unwrap();

    let (s, r) = c.req("POST", &confirm, json!({"token": token})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let (s, _) = call(&h.app, "POST", "/v1/payments", Some(&p.key), eur("x", 100)).await;
    assert!(s == StatusCode::UNAUTHORIZED || s == StatusCode::FORBIDDEN, "{s}");
    let t = db::get_tenant(&h.pool, p.id).await.unwrap().unwrap();
    assert!(!t.enabled);
    let n: i64 = sqlx::query_scalar("select count(*) from audit_event where action in ('project.delete_requested','project.deleted') and tenant_id = ?")
        .bind(p.id).fetch_one(&h.pool).await.unwrap();
    assert_eq!(n, 2);
}
