mod common;
use axum::http::StatusCode;
use common::console::*;
use common::*;
use payment_gateway::db;
use serde_json::{Value, json};

fn pay(p: &Project, extra: Value) -> Value {
    let mut base = eur("r", 1000);
    for (k, v) in extra.as_object().unwrap() {
        base[k] = v.clone();
    }
    let _ = p;
    base
}

async fn ids(c: &Client, p: &Project, query: &str) -> Vec<String> {
    let (s, r) = c
        .req("GET", &format!("/projects/{}/payments?{query}", p.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    r["payments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["reference"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn payment_list_filters_by_amount_customer_date_and_method() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    payment(&h, &p, pay(&p, json!({"reference": "small", "amount": 100, "customer_ref": "cus_1"}))).await;
    payment(&h, &p, pay(&p, json!({"reference": "big", "amount": 9000, "customer_ref": "cus_2"}))).await;
    let old = payment(&h, &p, pay(&p, json!({"reference": "old", "amount": 500}))).await;
    sqlx::query("update payment set created = created - interval 40 day where id = ?")
        .bind(&pid(&old)[..])
        .execute(&h.pool)
        .await
        .unwrap();
    assert_eq!(ids(&c, &p, "min_amount=1000").await, vec!["big"]);
    assert_eq!(ids(&c, &p, "max_amount=200").await, vec!["small"]);
    assert_eq!(ids(&c, &p, "customer=cus_2").await, vec!["big"]);
    let from = (chrono::Utc::now() - chrono::Duration::days(10)).format("%Y-%m-%d").to_string();
    assert_eq!(ids(&c, &p, &format!("from={from}")).await.len(), 2);
    let to = (chrono::Utc::now() - chrono::Duration::days(10)).format("%Y-%m-%d").to_string();
    assert_eq!(ids(&c, &p, &format!("to={to}")).await, vec!["old"]);
    assert!(ids(&c, &p, "method=lightning").await.is_empty());
    for bad in ["min_amount=abc", "method=nope", "from=yesterday", "min_amount=5&max_amount=1", "status=weird"] {
        let (s, _) = c
            .req("GET", &format!("/projects/{}/payments?{bad}", p.id), Value::Null)
            .await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn csv_export_is_scoped_escaped_and_audited() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let pa = project(&a, "a").await;
    let pb = project(&b, "b").await;
    payment(&h, &pa, pay(&pa, json!({"reference": "=cmd|' /C calc'!A0", "description": "hello, \"world\""}))).await;
    payment(&h, &pb, pay(&pb, json!({"reference": "other-tenant"}))).await;
    let r = a
        .raw("GET", &format!("/projects/{}/payments-export", pa.id), Value::Null)
        .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.headers["content-type"].to_str().unwrap().starts_with("text/csv"));
    assert!(r.text.starts_with("id,reference,status"));
    assert!(r.text.contains("'=cmd|' /C calc'!A0"));
    assert!(!r.text.contains(",=cmd"));
    assert!(r.text.contains("\"hello, \"\"world\"\"\""));
    assert!(!r.text.contains("other-tenant"));
    let (s, _) = b
        .req("GET", &format!("/projects/{}/payments-export", pa.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let n: i64 = sqlx::query_scalar("select count(*) from audit_event where action = 'payments.exported' and tenant_id = ?")
        .bind(pa.id)
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn csv_export_streams_more_than_one_page() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    for i in 0..3 {
        payment(&h, &p, pay(&p, json!({"reference": format!("r{i}")}))).await;
    }
    let r = c
        .raw("GET", &format!("/projects/{}/payments-export?status=pending", p.id), Value::Null)
        .await;
    assert_eq!(r.text.lines().count(), 4);
}

#[tokio::test]
async fn notes_are_stored_audited_and_tenant_scoped() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let pa = project(&a, "a").await;
    let pb = project(&b, "b").await;
    let id = payment(&h, &pa, eur("n1", 100)).await;
    let path = format!("/projects/{}/payments/{id}/notes", pa.id);
    let (s, _) = a.req("POST", &path, json!({"body": "  called the customer "})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = a.req("POST", &path, json!({"body": " "})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (_, r) = a.req("GET", &path, Value::Null).await;
    assert_eq!(r["notes"][0]["body"], "called the customer");
    let (s, _) = b.req("GET", &path, Value::Null).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = b
        .req("POST", &format!("/projects/{}/payments/{id}/notes", pb.id), json!({"body": "x"}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let n: i64 = sqlx::query_scalar("select count(*) from audit_event where action = 'payment.note_added'")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn manual_mark_paid_is_off_by_default_and_emits_a_manual_event() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let rx = Receiver::start().await;
    let p = project(&c, "shop").await;
    db::set_tenant_trusted(&h.pool, p.id, true).await.unwrap();
    c.req("POST", &format!("/projects/{}/webhook-endpoints", p.id), json!({"url": rx.url}))
        .await;
    let id = payment(&h, &p, eur("m1", 700)).await;
    let path = format!("/projects/{}/payments/{id}/mark-paid", p.id);
    let (s, _) = c.req("POST", &path, json!({"reason": "bank transfer"})).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = c
        .req("POST", &format!("/projects/{}/manual-paid", p.id), json!({"enabled": true}))
        .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = c.req("POST", &path, json!({"reason": "x"})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, r) = c.req("POST", &path, json!({"reason": "bank transfer"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "paid");
    let (s, _) = c.req("POST", &path, json!({"reason": "again please"})).await;
    assert_eq!(s, StatusCode::CONFLICT);
    h.service.deliver_due().await.unwrap();
    let events = rx.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], "payment.paid");
    assert_eq!(events[0]["data"]["detail"]["manual"], true);
    let n: i64 = sqlx::query_scalar("select count(*) from audit_event where action = 'payment.marked_paid' and tenant_id = ?")
        .bind(p.id)
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn manual_mark_paid_cannot_touch_another_tenant() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let pa = project(&a, "a").await;
    let pb = project(&b, "b").await;
    db::set_manual_paid_allowed(&h.pool, pb.id, true).await.unwrap();
    let id = payment(&h, &pa, eur("m1", 700)).await;
    let (s, _) = b
        .req("POST", &format!("/projects/{}/payments/{id}/mark-paid", pb.id), json!({"reason": "sneaky"}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = b
        .req("POST", &format!("/projects/{}/payments/{id}/mark-paid", pa.id), json!({"reason": "sneaky"}))
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bulk_cancel_only_touches_stale_pending_payments_of_this_project() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let pa = project(&a, "a").await;
    let pb = project(&b, "b").await;
    let stale = payment(&h, &pa, eur("stale", 100)).await;
    let fresh = payment(&h, &pa, eur("fresh", 100)).await;
    let other = payment(&h, &pb, eur("other", 100)).await;
    age(&h, &stale, 48).await;
    age(&h, &other, 48).await;
    let path = format!("/projects/{}/payments-bulk-cancel", pa.id);
    let (s, _) = a.req("POST", &path, json!({"older_than_hours": 0})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, r) = a.req("POST", &path, json!({"older_than_hours": 24})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["cancelled"], 1);
    let status = |id: String| {
        let pool = h.pool.clone();
        async move {
            sqlx::query_scalar::<_, String>("select status from payment where id = ?")
                .bind(&pid(&id)[..])
                .fetch_one(&pool)
                .await
                .unwrap()
        }
    };
    assert_eq!(status(stale).await, "cancelled");
    assert_eq!(status(fresh).await, "pending");
    assert_eq!(status(other).await, "pending");
    let (s, _) = b.req("POST", &path, json!({"older_than_hours": 24})).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn saved_views_are_per_account_validated_and_deletable() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let pa = project(&a, "a").await;
    let path = format!("/projects/{}/saved-views", pa.id);
    let (s, _) = a.req("POST", &path, json!({"name": "big", "filter": {"min_amount": "1000"}})).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = a.req("POST", &path, json!({"name": "bad", "filter": {"min_amount": "x"}})).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (_, r) = a.req("GET", &path, Value::Null).await;
    assert_eq!(r["views"][0]["filter"]["min_amount"], "1000");
    let (s, _) = b.req("GET", &path, Value::Null).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let vid = r["views"][0]["id"].as_u64().unwrap();
    let (s, _) = a.req("DELETE", &format!("{path}/{vid}"), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    let (_, r) = a.req("GET", &path, Value::Null).await;
    assert!(r["views"].as_array().unwrap().is_empty());
}
