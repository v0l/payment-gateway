mod common;
use axum::http::StatusCode;
use common::console::*;
use common::*;
use payment_gateway::db;
use serde_json::{Value, json};

async fn saved(h: &Harness, p: &Project, customer: &str, ext: &str) -> u64 {
    let t = TenantFixture {
        id: p.id,
        key: p.key.clone(),
        webhook_secret: p.secret.clone(),
        receiver: Receiver::start().await,
    };
    let provider_id = provider(h, &t, "stripe", json!({}), None).await;
    db::upsert_saved_method(
        &h.pool,
        &db::NewSavedMethod {
            tenant_id: p.id,
            customer_ref: customer,
            provider_config_id: provider_id,
            external_customer_id: "cus_x",
            external_id: ext,
            brand: Some("visa"),
            last4: Some("4242"),
            exp_month: Some(12),
            exp_year: Some(2030),
        },
    )
    .await
    .unwrap();
    sqlx::query_scalar("select id from saved_method where external_id = ?")
        .bind(ext)
        .fetch_one(&h.pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn customer_page_lists_payments_and_saved_methods() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    let mut body = eur("c1", 500);
    body["customer_ref"] = json!("cus_1");
    body["customer_email"] = json!("x@example.com");
    let id = payment(&h, &p, body).await;
    force_paid(&h, &id, 5).await;
    let mut other = eur("c2", 700);
    other["customer_ref"] = json!("cus_2");
    payment(&h, &p, other).await;
    saved(&h, &p, "cus_1", "pm_1").await;

    let (s, list) = c
        .req("GET", &format!("/projects/{}/customers", p.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(list["customers"].as_array().unwrap().len(), 2);
    let (_, list) = c
        .req(
            "GET",
            &format!("/projects/{}/customers?q=cus_2", p.id),
            Value::Null,
        )
        .await;
    assert_eq!(list["customers"].as_array().unwrap().len(), 1);

    let (s, r) = c
        .req(
            "GET",
            &format!("/projects/{}/customers/cus_1", p.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["email"], "x@example.com");
    assert_eq!(r["payments"].as_array().unwrap().len(), 1);
    assert_eq!(r["paid_totals"]["EUR"], 500);
    assert_eq!(r["saved_methods"][0]["last4"], "4242");
    assert_eq!(r["saved_methods"][0]["brand"], "visa");
    assert!(r["subscriptions"].as_array().unwrap().is_empty());
    let (s, _) = c
        .req(
            "GET",
            &format!("/projects/{}/customers/nobody", p.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deleting_a_saved_method_is_audited_and_tenant_scoped() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let pa = project(&a, "a").await;
    let pb = project(&b, "b").await;
    let mid = saved(&h, &pa, "cus_1", "pm_1").await;
    let path = |p: &Project, cust: &str| {
        format!("/projects/{}/customers/{cust}/saved-methods/{mid}", p.id)
    };
    let (s, _) = b.req("DELETE", &path(&pb, "cus_1"), Value::Null).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = b.req("DELETE", &path(&pa, "cus_1"), Value::Null).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = a.req("DELETE", &path(&pa, "cus_other"), Value::Null).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = a.req("DELETE", &path(&pa, "cus_1"), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = a
        .req(
            "GET",
            &format!("/projects/{}/customers/cus_1", pa.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let n: i64 = sqlx::query_scalar(
        "select count(*) from audit_event where action = 'saved_method.deleted'",
    )
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn customers_of_another_project_are_invisible() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let pa = project(&a, "a").await;
    let pb = project(&b, "b").await;
    let mut body = eur("c1", 500);
    body["customer_ref"] = json!("cus_1");
    payment(&h, &pa, body).await;
    let (_, list) = b
        .req(
            "GET",
            &format!("/projects/{}/customers", pb.id),
            Value::Null,
        )
        .await;
    assert!(list["customers"].as_array().unwrap().is_empty());
    let (s, _) = b
        .req(
            "GET",
            &format!("/projects/{}/customers", pa.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = b
        .req(
            "GET",
            &format!("/projects/{}/customers/cus_1", pb.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn customer_page_lists_subscriptions_with_plan_status_and_next_charge() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let pa = project(&a, "a").await;
    let pb = project(&b, "b").await;
    let now = chrono::Utc::now();
    let plan_id = db::insert_plan(
        &h.pool,
        &db::NewPlan {
            tenant_id: pa.id,
            name: "Pro",
            description: None,
            amount: 1900,
            currency: "EUR",
            interval_unit: "month",
            interval_count: 1,
            trial_days: 0,
            grace_days: 3,
            created: now,
        },
    )
    .await
    .unwrap();
    let next = now + chrono::Duration::days(20);
    let sub_id = db::insert_subscription(
        &h.pool,
        &db::NewSubscription {
            tenant_id: pa.id,
            customer_ref: "cus_sub",
            plan_id,
            saved_method_id: None,
            status: "active",
            period_index: 0,
            anchor: now,
            anchor_index: 0,
            current_period_start: now,
            current_period_end: next,
            trial_end: None,
            next_charge: Some(next),
            setup_payment_id: None,
            created: now,
        },
    )
    .await
    .unwrap();
    let path = |p: &Project| format!("/projects/{}/customers/cus_sub", p.id);
    let (s, r) = a.req("GET", &path(&pa), Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let sub = &r["subscriptions"][0];
    assert_eq!(sub["id"], sub_id);
    assert_eq!(sub["status"], "active");
    assert_eq!(sub["plan_name"], "Pro");
    assert_eq!(sub["amount"], 1900);
    assert!(sub["next_charge"].is_string());
    let (s, _) = b.req("GET", &path(&pb), Value::Null).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = b.req("GET", &path(&pa), Value::Null).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
