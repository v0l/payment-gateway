use payment_gateway_client::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

type Delivery = (Vec<u8>, Option<String>);

async fn start_receiver() -> (String, Arc<Mutex<Vec<Delivery>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    let store: Arc<Mutex<Vec<Delivery>>> = Arc::default();
    let shared = store.clone();
    tokio::spawn(async move {
        loop {
            let (mut sock, _) = listener.accept().await.unwrap();
            let shared = shared.clone();
            tokio::spawn(async move {
                let mut data = vec![];
                let mut buf = [0u8; 4096];
                loop {
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    data.extend_from_slice(&buf[..n]);
                    if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&data[..pos]).to_string();
                        let len = head
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if data.len() >= pos + 4 + len {
                            let signature = head.lines().find_map(|l| {
                                let lower = l.to_lowercase();
                                lower
                                    .starts_with("x-gateway-signature:")
                                    .then(|| l[20..].trim().to_string())
                            });
                            shared
                                .lock()
                                .unwrap()
                                .push((data[pos + 4..pos + 4 + len].to_vec(), signature));
                            let _ = sock
                                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                                .await;
                            return;
                        }
                    }
                }
            });
        }
    });
    (url, store)
}

fn var(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

#[tokio::test]
async fn conformance() {
    let Ok(url) = std::env::var("GATEWAY_URL") else {
        eprintln!("GATEWAY_URL not set, skipping");
        return;
    };
    let (hook_url, deliveries) = start_receiver().await;
    let http = reqwest::Client::new();
    let tenant = var("GATEWAY_TENANT_ID");
    let status = http
        .post(format!("{url}/admin/tenants/{tenant}/settings"))
        .bearer_auth(var("GATEWAY_ADMIN_TOKEN"))
        .json(&serde_json::json!({"webhook_url": hook_url}))
        .send()
        .await
        .unwrap()
        .status();
    assert_eq!(status, 200);

    let gw = Gateway::new(&url, &var("GATEWAY_API_KEY"));
    let ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let reference = format!("rs-{ms}");
    let key = format!("key-{reference}");

    let input = CreatePayment::new(1000, "EUR").reference(&reference);
    let created = gw.create_payment_with_key(&input, &key).await.unwrap();
    assert_eq!(created.status, "pending");
    let replay = gw.create_payment_with_key(&input, &key).await.unwrap();
    assert_eq!(replay.id, created.id);
    let conflict = gw
        .create_payment_with_key(&CreatePayment::new(2000, "EUR").reference(&reference), &key)
        .await
        .unwrap_err();
    assert_eq!(conflict.status(), Some(409));

    let with_attempt = gw
        .create_attempt(&created.id, &CreateAttempt::new("lightning"))
        .await
        .unwrap();
    let lookup = with_attempt.attempts[0].details["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let hook = http
        .post(format!(
            "{url}/hook/{tenant}/{}",
            var("GATEWAY_PROVIDER_ID")
        ))
        .header("x-mock-secret", var("GATEWAY_MOCK_SECRET"))
        .json(&serde_json::json!({"lookup": lookup}))
        .send()
        .await
        .unwrap();
    assert_eq!(hook.status(), 200);

    let secret = var("GATEWAY_WEBHOOK_SECRET");
    let mut delivery = None;
    for _ in 0..100 {
        delivery = deliveries
            .lock()
            .unwrap()
            .iter()
            .find(|(body, _)| String::from_utf8_lossy(body).contains(&created.id))
            .cloned();
        if delivery.is_some() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    let (body, signature) = delivery.expect("webhook delivered");
    let event = verify_webhook(&body, signature.as_deref(), &secret, 300).unwrap();
    assert_eq!(event["type"], "payment.paid");
    let mut tampered = body.clone();
    tampered.push(b' ');
    assert_eq!(
        verify_webhook(&tampered, signature.as_deref(), &secret, 300),
        Err(WebhookError::SignatureMismatch)
    );
    assert_eq!(
        verify_webhook(&body, signature.as_deref(), "whsec_wrong", 300),
        Err(WebhookError::SignatureMismatch)
    );

    assert_eq!(gw.get_payment(&created.id).await.unwrap().status, "paid");
    let refund = gw
        .refund_payment(
            &created.id,
            &RefundInput {
                amount: Some(500),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(refund.status, "completed");

    let listed = gw
        .list_payments(&ListPayments {
            reference: Some(reference),
            limit: Some(1),
            ..Default::default()
        })
        .collect()
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);

    let rejected = gw
        .create_withdrawal(&CreateWithdrawal {
            amount: 5_000_000,
            destination: Some("not-an-invoice".into()),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(matches!(rejected, Error::Api { status: 400, .. }));
    assert!(gw.list_withdrawals().collect().await.unwrap().is_empty());
}
