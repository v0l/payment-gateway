use hmac::{Hmac, Mac};
use payment_gateway::providers::Provider;
use payment_gateway::providers::revolut::RevolutProvider;
use payment_gateway::providers::stripe::StripeProvider;
use payment_gateway::rates::{MAX_AMOUNT, StaticRates, convert, processing_fee};
use payment_gateway::service::{sign_return, webhook_signature};
use serde_json::json;
use sha2::Sha256;
use std::collections::HashMap;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn bytes(&mut self, max: usize) -> Vec<u8> {
        let n = (self.next() as usize) % (max + 1);
        (0..n).map(|_| self.next() as u8).collect()
    }

    fn text(&mut self, max: usize) -> String {
        String::from_utf8_lossy(&self.bytes(max)).into_owned()
    }
}

fn rates() -> StaticRates {
    StaticRates::new(&[("EUR", 50_000.0), ("USD", 60_000.0), ("JPY", 7_000_000.0)])
}

#[tokio::test]
async fn conversion_never_panics_and_round_trips_within_one_unit_of_rounding() {
    let r = rates();
    let mut rng = Rng(0x9e3779b97f4a7c15);
    for _ in 0..2000 {
        let amount = match rng.next() % 4 {
            0 => rng.next(),
            1 => rng.next() % 1_000,
            2 => rng.next() % 10_000_000_000,
            _ => MAX_AMOUNT - rng.next() % 3,
        };
        for (from, to) in [
            ("EUR", "BTC"),
            ("BTC", "EUR"),
            ("EUR", "USD"),
            ("JPY", "BTC"),
            ("BTC", "BTC"),
        ] {
            if let Ok(v) = convert(&r, amount, from, to).await
                && from == to
            {
                assert_eq!(v, amount);
            }
        }
        if amount <= 100_000_000_000 {
            let msats = convert(&r, amount, "EUR", "BTC").await.unwrap();
            let back = convert(&r, msats, "BTC", "EUR").await.unwrap();
            assert!(
                back >= amount && back <= amount + 1,
                "{amount} -> {msats} -> {back}"
            );
        }
    }
}

#[tokio::test]
async fn conversion_is_monotonic_and_rejects_hostile_rates() {
    let r = rates();
    let mut last = 0;
    for amount in (0..5000).map(|i| i * 37) {
        let v = convert(&r, amount, "EUR", "BTC").await.unwrap();
        assert!(v >= last);
        last = v;
    }
    assert!(convert(&r, MAX_AMOUNT + 1, "EUR", "BTC").await.is_err());
    for bad in [f64::NAN, f64::INFINITY, 0.0, -1.0, f64::MIN_POSITIVE] {
        let hostile = StaticRates::new(&[("EUR", bad)]);
        if let Ok(v) = convert(&hostile, 1_000_000, "EUR", "BTC").await {
            assert!(v <= MAX_AMOUNT, "{bad}: {v}");
        }
    }
}

#[tokio::test]
async fn processing_fee_is_bounded_and_rejects_bad_rates() {
    let r = rates();
    let mut rng = Rng(42);
    for _ in 0..2000 {
        let amount = rng.next() % (MAX_AMOUNT + 1);
        let pct = (rng.next() % 10_001) as f64 / 100.0;
        let base = rng.next() % 1_000_000;
        let fee = processing_fee(&r, Some(pct), Some(base), Some("EUR"), amount, "EUR")
            .await
            .unwrap();
        assert!(fee >= base);
        assert!(fee <= amount + base + 1, "{amount} {pct} {base} {fee}");
    }
    for bad in [f64::NAN, f64::INFINITY, -0.1, 100.01, 1e300] {
        assert!(
            processing_fee(&r, Some(bad), None, None, 1000, "EUR")
                .await
                .is_err(),
            "{bad}"
        );
    }
}

#[test]
fn signatures_are_deterministic_hex_and_bound_to_every_input() {
    let mut rng = Rng(7);
    for _ in 0..500 {
        let secret = rng.text(40);
        let body = rng.text(200);
        let ts = rng.next() as i64;
        let sig = webhook_signature(&secret, ts, &body);
        assert_eq!(sig.len(), 64);
        assert!(sig.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(sig, webhook_signature(&secret, ts, &body));
        assert_ne!(sig, webhook_signature(&secret, ts.wrapping_add(1), &body));
        assert_ne!(sig, webhook_signature(&secret, ts, &format!("{body}x")));
        assert_ne!(sig, webhook_signature(&format!("{secret}x"), ts, &body));
        let r = sign_return(&secret, &body, "ref", "paid");
        assert_ne!(r, sign_return(&secret, &body, "ref", "cancelled"));
        assert_ne!(r, sign_return(&secret, &body, "ref2", "paid"));
    }
}

fn stripe_header(secret: &str, ts: i64, body: &[u8]) -> HashMap<String, String> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("{ts}.").as_bytes());
    mac.update(body);
    let sig = hex::encode(mac.finalize().into_bytes());
    HashMap::from([("stripe-signature".to_string(), format!("t={ts},v1={sig}"))])
}

#[tokio::test]
async fn stripe_webhook_parser_rejects_garbage_and_accepts_only_a_valid_signature() {
    let p = StripeProvider::new(&json!({"secret_key": "sk", "webhook_secret": "whsec_x"})).unwrap();
    let mut rng = Rng(99);
    for _ in 0..500 {
        let body = rng.bytes(300);
        let mut headers = HashMap::new();
        if rng.next().is_multiple_of(2) {
            headers.insert("stripe-signature".to_string(), rng.text(120));
        }
        assert!(p.handle_webhook(&headers, &body).await.is_err());
    }
    let shapes = [
        "t=,v1=",
        "t=abc,v1=zz",
        "v1=",
        "t=1",
        ",,,",
        "t=99999999999999999999,v1=00",
        "t=1,v1=00,v1=11",
    ];
    for h in shapes {
        let headers = HashMap::from([("stripe-signature".to_string(), h.to_string())]);
        assert!(p.handle_webhook(&headers, b"{}").await.is_err(), "{h}");
    }
    let body = br#"{"id":"evt_1","object":"event","type":"ignored.type","data":{"object":{}}}"#;
    let now = chrono::Utc::now().timestamp();
    let good = stripe_header("whsec_x", now, body);
    assert!(p.handle_webhook(&good, body).await.is_ok());
    let wrong = stripe_header("whsec_y", now, body);
    assert!(p.handle_webhook(&wrong, body).await.is_err());
    let stale = stripe_header("whsec_x", now - 86_400, body);
    assert!(p.handle_webhook(&stale, body).await.is_err());
    let mut tampered = body.to_vec();
    tampered[10] ^= 1;
    assert!(p.handle_webhook(&good, &tampered).await.is_err());
}

#[tokio::test]
async fn revolut_webhook_parser_rejects_garbage() {
    let p = RevolutProvider::new(&json!({
        "token": "t", "api_version": "2024-09-01", "webhook_secret": "wsk_x"
    }))
    .unwrap();
    let mut rng = Rng(1234);
    for _ in 0..500 {
        let body = rng.bytes(300);
        let mut headers = HashMap::new();
        for name in ["revolut-signature", "revolut-request-timestamp"] {
            if rng.next().is_multiple_of(2) {
                headers.insert(name.to_string(), rng.text(100));
            }
        }
        assert!(p.handle_webhook(&headers, &body).await.is_err());
    }
}
