use payment_gateway_client::*;
use serde_json::Value;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn vectors() -> Value {
    serde_json::from_str(include_str!("../../test-vectors/webhook.json")).unwrap()
}

#[test]
fn signatures_match_server_vectors() {
    for v in vectors()["vectors"].as_array().unwrap() {
        let sig = sign_webhook(
            v["body"].as_str().unwrap().as_bytes(),
            v["secret"].as_str().unwrap(),
            v["timestamp"].as_i64().unwrap(),
        );
        assert_eq!(sig, v["signature"].as_str().unwrap());
    }
}

#[test]
fn verifies_server_vectors() {
    for v in vectors()["vectors"].as_array().unwrap() {
        let body = v["body"].as_str().unwrap();
        if body.is_empty() {
            continue;
        }
        let ts = v["timestamp"].as_i64().unwrap();
        verify_webhook_at(
            body.as_bytes(),
            v["header"].as_str(),
            v["secret"].as_str().unwrap(),
            300,
            ts + 5,
        )
        .unwrap();
    }
}

#[test]
fn rejects_tampering_and_bad_timestamps() {
    let all = vectors();
    let v = &all["vectors"][0];
    let body = v["body"].as_str().unwrap();
    let header = v["header"].as_str();
    let secret = v["secret"].as_str().unwrap();
    let ts = v["timestamp"].as_i64().unwrap();
    assert_eq!(
        verify_webhook_at(format!("{body} ").as_bytes(), header, secret, 300, ts),
        Err(WebhookError::SignatureMismatch)
    );
    assert_eq!(
        verify_webhook_at(body.as_bytes(), header, "wrong", 300, ts),
        Err(WebhookError::SignatureMismatch)
    );
    let replay = header
        .unwrap()
        .replace(&format!("t={ts}"), &format!("t={}", ts + 1));
    assert_eq!(
        verify_webhook_at(body.as_bytes(), Some(&replay), secret, 300, ts),
        Err(WebhookError::SignatureMismatch)
    );
    assert_eq!(
        verify_webhook_at(body.as_bytes(), header, secret, 300, ts + 301),
        Err(WebhookError::TimestampOutsideTolerance)
    );
    assert_eq!(
        verify_webhook_at(body.as_bytes(), header, secret, 300, ts - 301),
        Err(WebhookError::TimestampOutsideTolerance)
    );
    assert!(verify_webhook_at(body.as_bytes(), header, secret, 300, ts + 300).is_ok());
    assert!(verify_webhook_at(body.as_bytes(), header, secret, 0, ts + 1_000_000).is_ok());
    assert_eq!(
        verify_webhook_at(body.as_bytes(), None, secret, 300, ts),
        Err(WebhookError::MissingHeader)
    );
    assert_eq!(
        verify_webhook_at(body.as_bytes(), Some("t=abc,v1=00"), secret, 300, ts),
        Err(WebhookError::MalformedHeader)
    );
}

async fn scripted_server(
    responses: Vec<(u16, &'static str, &'static str)>,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let mut seen = vec![];
        for (status, extra_headers, body) in responses {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = vec![0u8; 8192];
            let n = sock.read(&mut buf).await.unwrap();
            seen.push(String::from_utf8_lossy(&buf[..n]).to_string());
            let reply = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\n{extra_headers}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            sock.write_all(reply.as_bytes()).await.unwrap();
        }
        seen
    });
    (url, handle)
}

fn fast(g: Gateway) -> Gateway {
    g.with_retries(3, Duration::from_millis(1), Duration::from_millis(5))
}

const PAYMENT: &str =
    r#"{"id":"p1","reference":"r","amount":1,"currency":"EUR","status":"pending","pay_url":"u"}"#;

#[tokio::test]
async fn retries_429_and_5xx_with_the_same_idempotency_key() {
    let (url, server) = scripted_server(vec![
        (429, "retry-after: 0\r\n", r#"{"error":"slow"}"#),
        (503, "", r#"{"error":"down"}"#),
        (201, "", PAYMENT),
    ])
    .await;
    let g = fast(Gateway::new(&url, "k"));
    let p = g
        .create_payment(&CreatePayment::new(1, "EUR"))
        .await
        .unwrap();
    assert_eq!(p.id, "p1");
    let seen = server.await.unwrap();
    let keys: Vec<_> = seen
        .iter()
        .map(|r| {
            r.lines()
                .find(|l| l.to_lowercase().starts_with("idempotency-key"))
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(keys[0], keys[1]);
    assert_eq!(keys[1], keys[2]);
}

#[tokio::test]
async fn api_error_exposes_code_and_field_without_retrying_4xx() {
    let body = r#"{"error":"bad","error_detail":{"code":"invalid_request","message":"bad","field":"amount"}}"#;
    let (url, server) = scripted_server(vec![(400, "", body)]).await;
    let g = fast(Gateway::new(&url, "k"));
    let err = g
        .create_payment(&CreatePayment::new(0, "EUR"))
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(400));
    assert_eq!(err.code(), Some("invalid_request"));
    assert_eq!(server.await.unwrap().len(), 1);
}

#[tokio::test]
async fn pager_follows_next_cursor() {
    let (url, server) = scripted_server(vec![
        (
            200,
            "x-next-cursor: c1\r\n",
            &*Box::leak(format!("[{PAYMENT}]").into_boxed_str()),
        ),
        (
            200,
            "",
            &*Box::leak(format!("[{PAYMENT}]").into_boxed_str()),
        ),
    ])
    .await;
    let g = fast(Gateway::new(&url, "k"));
    let all = g
        .list_payments(&ListPayments::default())
        .collect()
        .await
        .unwrap();
    assert_eq!(all.len(), 2);
    let seen = server.await.unwrap();
    assert!(seen[1].starts_with("GET /v1/payments?cursor=c1"));
}

#[tokio::test]
async fn line_items_are_serialised_as_the_api_schema_says() {
    let stored = r#"{"id":"p1","reference":"r","amount":1230,"currency":"EUR","status":"pending","pay_url":"u","line_items":[{"name":"Widget","quantity":2,"unit_amount":500}]}"#;
    let (url, server) = scripted_server(vec![(201, "", stored)]).await;
    let g = fast(Gateway::new(&url, "k"));
    let mut input = CreatePayment::new(0, "EUR");
    input.line_items = Some(vec![
        LineItemInput {
            quantity: Some(2),
            tax_rate: Some(23.0),
            sku: Some("sku-1".into()),
            description: Some("d".into()),
            ..LineItemInput::new("Widget", 500)
        },
        LineItemInput {
            tax_amount: Some(7),
            ..LineItemInput::new("Plain", 100)
        },
    ]);
    let created = g.create_payment(&input).await.unwrap();
    assert_eq!(created.line_items[0]["name"], "Widget");
    let seen = server.await.unwrap();
    let body: Value = serde_json::from_str(seen[0].split("\r\n\r\n").nth(1).unwrap()).unwrap();
    assert_eq!(
        body["line_items"],
        serde_json::json!([
            {"name": "Widget", "description": "d", "quantity": 2, "unit_amount": 500, "tax_rate": 23.0, "sku": "sku-1"},
            {"name": "Plain", "unit_amount": 100, "tax_amount": 7}
        ])
    );
    let plain = serde_json::to_value(CreatePayment::new(1, "EUR")).unwrap();
    assert!(plain.get("line_items").is_none());
}
