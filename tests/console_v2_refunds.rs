mod common;
use axum::http::StatusCode;
use common::console::*;
use common::*;
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::ProviderSink;
use serde_json::{Value, json};

async fn paid_project(h: &Harness, c: &Client, fee: bool) -> (Project, String) {
    let p = project(c, "shop").await;
    let t = TenantFixture {
        id: p.id,
        key: p.key.clone(),
        webhook_secret: p.secret.clone(),
        receiver: Receiver::start().await,
    };
    let ln = provider(
        h,
        &t,
        "lightning",
        json!({}),
        if fee { Some((0.1, 0, "EUR")) } else { None },
    )
    .await;
    let pay = new_payment(h, &t, eur("r1", 1000)).await;
    let id = pay["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, &t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"].as_str().unwrap().to_string();
    ProviderSink { service: h.service.clone(), provider_id: ln }
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
    (p, id)
}

#[tokio::test]
async fn preview_shows_refundable_amount_and_partial_and_full_numbers() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let (p, id) = paid_project(&h, &c, false).await;
    let path = format!("/projects/{}/payments/{id}/refund-preview", p.id);
    let (s, r) = c.req("GET", &path, Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["refundable"], 1000);
    assert_eq!(r["amount"], 1000);
    assert_eq!(r["full"], true);
    assert_eq!(r["remaining_after"], 0);
    assert_eq!(r["destination_required"], true);
    let (_, r) = c.req("GET", &format!("{path}?amount=400"), Value::Null).await;
    assert_eq!(r["full"], false);
    assert_eq!(r["remaining_after"], 600);
    let (s, _) = c.req("GET", &format!("{path}?amount=1001"), Value::Null).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = c.req("GET", &format!("{path}?amount=0"), Value::Null).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn preview_rejects_malformed_destinations() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let (p, id) = paid_project(&h, &c, false).await;
    let path = format!("/projects/{}/payments/{id}/refund-preview", p.id);
    let (s, r) = c.req("GET", &format!("{path}?destination=notaninvoice"), Value::Null).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
}

#[tokio::test]
async fn preview_is_tenant_scoped_and_needs_a_paid_payment() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let (pa, id) = paid_project(&h, &a, false).await;
    let pb = project(&b, "b").await;
    let (s, _) = b
        .req("GET", &format!("/projects/{}/payments/{id}/refund-preview", pb.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = b
        .req("GET", &format!("/projects/{}/payments/{id}/refund-preview", pa.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let pending = payment(&h, &pa, eur("p", 100)).await;
    let (s, _) = a
        .req("GET", &format!("/projects/{}/payments/{pending}/refund-preview", pa.id), Value::Null)
        .await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[test]
fn onchain_destination_must_be_a_bitcoin_address() {
    use payment_gateway::service::validate_destination;
    assert!(validate_destination("onchain", 1000, "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4").is_ok());
    assert!(validate_destination("onchain", 1000, "not-an-address").is_err());
    assert!(validate_destination("lightning", 1000, "lnbc1garbage").is_err());
}
