use payment_gateway::service::webhook_signature;
use serde_json::json;

fn main() {
    let cases = [
        (
            "whsec_test_secret",
            1700000000i64,
            r#"{"id":"evt_1","type":"payment.paid","created":1700000000,"data":{"payment":{"id":"abc","amount":1000,"currency":"EUR"}}}"#,
        ),
        (
            "another-secret-value",
            1700000500i64,
            r#"{"id":"evt_2","type":"refund.completed","data":{}}"#,
        ),
        (
            "whsec_test_secret",
            1800000000i64,
            "{\"description\":\"caf\u{e9} \u{26a1} \\\"quoted\\\"\",\"line\":\"a\\nb\"}",
        ),
        ("whsec_test_secret", 1700000000i64, ""),
    ];
    let vectors: Vec<_> = cases
        .iter()
        .map(|(secret, ts, body)| {
            let sig = webhook_signature(secret, *ts, body);
            json!({
                "secret": secret,
                "timestamp": ts,
                "body": body,
                "signature": sig,
                "header": format!("t={ts},v1={sig}"),
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(
            &json!({ "header": "x-gateway-signature", "vectors": vectors })
        )
        .unwrap()
    );
}
