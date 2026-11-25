mod common;
use axum::http::StatusCode;
use common::console::*;
use common::*;
use payment_gateway::db;
use serde_json::{Value, json};

async fn paid_payment_for(h: &Harness, p: &Project) -> db::PaymentRow {
    let id = payment(
        h,
        p,
        json!({"reference": unique("r"), "amount": 500, "currency": "EUR", "customer_email": "buyer@example.com"}),
    )
    .await;
    h.service
        .payment_for_tenant(&h.service.tenant(p.id).await.unwrap(), &pid(&id))
        .await
        .unwrap()
}

async fn set_receipts(c: &Client, p: &Project, on: bool) -> (StatusCode, Value) {
    c.req(
        "POST",
        &format!("/projects/{}/settings", p.id),
        json!({"customer_receipts": on}),
    )
    .await
}

#[tokio::test]
async fn existing_projects_default_to_customer_receipts_on() {
    let h = harness().await;
    let t = tenant(&h).await;
    let row = db::get_tenant(&h.pool, t.id).await.unwrap().unwrap();
    assert!(row.customer_receipts);
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    let (_, r) = c
        .req("GET", &format!("/projects/{}", p.id), Value::Null)
        .await;
    assert_eq!(r["customer_receipts"], true);
}

#[tokio::test]
async fn receipt_mail_is_sent_when_on_and_not_when_off() {
    let h = harness_with_mail().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    let row = paid_payment_for(&h, &p).await;
    let tenant = h.service.tenant(p.id).await.unwrap();
    h.mailer.clear();
    h.service.send_receipt_mail(&tenant, &row);
    let sent = h.mailer.wait_for(1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "buyer@example.com");

    let (s, r) = set_receipts(&c, &p, false).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["customer_receipts"], false);
    let tenant = h.service.tenant(p.id).await.unwrap();
    h.mailer.clear();
    h.service.send_receipt_mail(&tenant, &row);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!(h.mailer.sent().is_empty());
}

#[tokio::test]
async fn owner_notifications_still_work_with_receipts_off() {
    let h = harness_with_mail().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    set_receipts(&c, &p, false).await;
    h.mailer.clear();
    h.service.notify_refund_failed(p.id, "boom").await;
    let sent = h.mailer.wait_for(1).await;
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].to, "a@example.com");
}

#[tokio::test]
async fn saving_other_settings_keeps_the_switch() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    set_receipts(&c, &p, false).await;
    let (s, r) = c
        .req(
            "POST",
            &format!("/projects/{}/settings", p.id),
            json!({"display_name": "Shop"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["customer_receipts"], false);
}

#[tokio::test]
async fn admin_creates_a_project_with_receipts_off_and_tenant_api_shows_it() {
    let h = harness().await;
    let (s, t) = call(
        &h.app,
        "POST",
        "/admin/tenants?receipts=off",
        Some(ADMIN),
        json!({"name": unique("quiet")}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{t}");
    let key = t["api_key"].as_str().unwrap();
    let (s, r) = call(&h.app, "GET", "/v1/project", Some(key), Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["customer_receipts"], false);

    let normal = common::tenant(&h).await;
    let (_, r) = call(&h.app, "GET", "/v1/project", Some(&normal.key), Value::Null).await;
    assert_eq!(r["customer_receipts"], true);
    let (_, r) = call(
        &h.app,
        "GET",
        &format!("/admin/tenants/{}", normal.id),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(r["customer_receipts"], true);
}

#[tokio::test]
async fn one_project_switch_does_not_touch_another() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let a = project(&c, "a").await;
    let b = project(&c, "b").await;
    set_receipts(&c, &a, false).await;
    let (_, r) = call(&h.app, "GET", "/v1/project", Some(&b.key), Value::Null).await;
    assert_eq!(r["customer_receipts"], true);
    let other = signed_in(&h, "x@example.com", false).await;
    let (s, _) = set_receipts(&other, &b, false).await;
    assert_ne!(s, StatusCode::OK);
    let (_, r) = call(&h.app, "GET", "/v1/project", Some(&b.key), Value::Null).await;
    assert_eq!(r["customer_receipts"], true);
}
