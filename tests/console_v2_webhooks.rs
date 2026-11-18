mod common;
use axum::http::StatusCode;
use common::console::*;
use common::*;
use payment_gateway::db;
use payment_gateway::service::webhook_signature;
use serde_json::{Value, json};

async fn trusted_project_with_endpoint(
    h: &Harness,
    c: &Client,
    name: &str,
    receiver: &Receiver,
) -> Project {
    let p = project(c, name).await;
    db::set_tenant_trusted(&h.pool, p.id, true).await.unwrap();
    let (s, e) = c
        .req(
            "POST",
            &format!("/projects/{}/webhook-endpoints", p.id),
            json!({"url": receiver.url}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{e}");
    p
}

#[tokio::test]
async fn delivery_detail_records_request_response_and_timing() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let rx = Receiver::start().await;
    let p = trusted_project_with_endpoint(&h, &c, "shop", &rx).await;
    let id = payment(&h, &p, eur("r1", 500)).await;
    let (s, _) = c
        .req("POST", &format!("/projects/{}/payments/{id}/cancel", p.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::OK);
    rx.fail_with(500);
    h.service.deliver_due().await.unwrap();
    let (_, list) = c
        .req("GET", &format!("/projects/{}/deliveries", p.id), Value::Null)
        .await;
    let did = list[0]["id"].as_u64().unwrap();
    let (s, d) = c
        .req("GET", &format!("/projects/{}/deliveries/{did}", p.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::OK, "{d}");
    assert_eq!(d["response"]["status"], 500);
    assert!(d["response"]["duration_ms"].is_number());
    assert_eq!(d["request"]["method"], "POST");
    assert!(d["request"]["body"].as_str().unwrap().contains("payment.cancelled"));
    assert!(d["time_to_deliver_ms"].is_null());

    rx.fail_with(200);
    let (_, r) = c
        .req(
            "POST",
            &format!("/projects/{}/deliveries/{did}/replay", p.id),
            Value::Null,
        )
        .await;
    assert_eq!(r["queued"], true);
    h.service.deliver_due().await.unwrap();
    let (_, d) = c
        .req("GET", &format!("/projects/{}/deliveries/{did}", p.id), Value::Null)
        .await;
    assert_eq!(d["response"]["status"], 200);
    assert!(d["time_to_deliver_ms"].is_number());
}

#[tokio::test]
async fn delivery_detail_is_tenant_scoped() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let rx = Receiver::start().await;
    let pa = trusted_project_with_endpoint(&h, &a, "a", &rx).await;
    let pb = project(&b, "b").await;
    let id = payment(&h, &pa, eur("r1", 500)).await;
    a.req("POST", &format!("/projects/{}/payments/{id}/cancel", pa.id), Value::Null)
        .await;
    let (_, list) = a
        .req("GET", &format!("/projects/{}/deliveries", pa.id), Value::Null)
        .await;
    let did = list[0]["id"].as_u64().unwrap();
    let (s, _) = b
        .req("GET", &format!("/projects/{}/deliveries/{did}", pb.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = b
        .req("GET", &format!("/projects/{}/deliveries/{did}", pa.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = b
        .req("POST", &format!("/projects/{}/deliveries-resend-failed", pa.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resend_failed_requeues_abandoned_deliveries_for_admins_only() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let rx = Receiver::start().await;
    let p = trusted_project_with_endpoint(&h, &c, "shop", &rx).await;
    let id = payment(&h, &p, eur("r1", 500)).await;
    c.req("POST", &format!("/projects/{}/payments/{id}/cancel", p.id), Value::Null)
        .await;
    sqlx::query("update webhook_delivery set abandoned_at = now(), last_error = 'boom' where tenant_id = ?")
        .bind(p.id)
        .execute(&h.pool)
        .await
        .unwrap();
    let (_, health) = c
        .req("GET", &format!("/projects/{}/webhook-health", p.id), Value::Null)
        .await;
    assert_eq!(health["endpoints"][0]["state"], "failing");

    let viewer = signed_in(&h, "v@example.com", false).await;
    let (_, vid) = c.req("GET", &format!("/projects/{}/members", p.id), Value::Null).await;
    let _ = vid;
    let (s, r) = c
        .req("POST", &format!("/projects/{}/deliveries-resend-failed", p.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["queued"], 1);
    h.service.deliver_due().await.unwrap();
    assert!(rx.types().contains(&"payment.cancelled".to_string()));
    let (_, r) = c
        .req("POST", &format!("/projects/{}/deliveries-resend-failed", p.id), Value::Null)
        .await;
    assert_eq!(r["queued"], 0);
    let (s, _) = viewer
        .req("POST", &format!("/projects/{}/deliveries-resend-failed", p.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn resend_payment_webhook_requeues_the_latest_event() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let rx = Receiver::start().await;
    let p = trusted_project_with_endpoint(&h, &c, "shop", &rx).await;
    let id = payment(&h, &p, eur("r1", 500)).await;
    let path = format!("/projects/{}/payments/{id}/resend-webhook", p.id);
    let (s, _) = c.req("POST", &path, Value::Null).await;
    assert_eq!(s, StatusCode::CONFLICT);
    c.req("POST", &format!("/projects/{}/payments/{id}/cancel", p.id), Value::Null)
        .await;
    h.service.deliver_due().await.unwrap();
    assert_eq!(rx.types().len(), 1);
    let (s, r) = c.req("POST", &path, Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    h.service.deliver_due().await.unwrap();
    assert_eq!(rx.types().len(), 2);
}

#[tokio::test]
async fn signature_helper_verifies_a_pasted_body_and_secret() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = project(&c, "shop").await;
    let body = r#"{"id":"x"}"#;
    let sig = webhook_signature("whsec_abc", 1700000000, body);
    let path = format!("/projects/{}/webhook-verify", p.id);
    let (s, r) = c
        .req(
            "POST",
            &path,
            json!({"body": body, "secret": "whsec_abc", "signature": format!("t=1700000000,v1={sig}")}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["valid"], true);
    let (_, r) = c
        .req(
            "POST",
            &path,
            json!({"body": body, "secret": "whsec_other", "signature": format!("t=1700000000,v1={sig}")}),
        )
        .await;
    assert_eq!(r["valid"], false);
    let (s, _) = c
        .req("POST", &path, json!({"body": body, "secret": "x", "signature": "nope"}))
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
