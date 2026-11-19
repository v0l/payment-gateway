mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::providers::revolut::RevolutProvider;
use payment_gateway::providers::stripe::StripeProvider;
use payment_gateway::providers::{AttemptRequest, EventSink, Provider, ProviderEvent};
use payment_gateway::service::ProviderSink;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn body(reference: &str) -> Value {
    json!({
        "reference": reference,
        "currency": "EUR",
        "customer_email": "buyer@example.com",
        "line_items": [
            {"name": "Widget", "description": "Blue", "quantity": 3, "unit_amount": 333, "tax_rate": 8.25, "sku": "W-1"},
            {"name": "Shipping", "unit_amount": 500}
        ],
        "discount": {"percent": 10, "code": "SAVE10"}
    })
}

async fn post(h: &Harness, t: &TenantFixture, b: Value) -> (StatusCode, Value) {
    call(&h.app, "POST", "/v1/payments", Some(&t.key), b).await
}

#[tokio::test]
async fn amount_is_computed_from_lines_and_round_trips() {
    let h = harness().await;
    let t = tenant(&h).await;
    let (s, p) = post(&h, &t, body("li-1")).await;
    assert_eq!(s, StatusCode::CREATED, "{p}");
    assert_eq!(p["subtotal"], 1499);
    assert_eq!(p["discount"]["amount"], 150);
    assert_eq!(p["discount"]["percent"], 10.0);
    assert_eq!(p["discount"]["code"], "SAVE10");
    assert_eq!(p["line_items"].as_array().unwrap().len(), 2);
    assert_eq!(p["line_items"][0]["sku"], "W-1");
    assert_eq!(p["line_items"][0]["tax_rate"], 8.25);
    let lines_total: u64 = p["line_items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l["total"].as_u64().unwrap())
        .sum();
    assert_eq!(p["amount"].as_u64().unwrap(), lines_total);
    assert_eq!(
        p["amount"].as_u64().unwrap(),
        1499 - 150 + p["tax"].as_u64().unwrap()
    );
    assert!(p["receipt_url"].as_str().unwrap().contains("/receipt/"));

    let got = get_payment(&h, &t, p["id"].as_str().unwrap()).await;
    assert_eq!(got["line_items"], p["line_items"]);
    assert_eq!(got["amount"], p["amount"]);
}

#[tokio::test]
async fn matching_amount_is_accepted_and_mismatch_rejected() {
    let h = harness().await;
    let t = tenant(&h).await;
    let (_, p) = post(&h, &t, body("li-2")).await;
    let total = p["amount"].clone();
    let mut ok = body("li-3");
    ok["amount"] = total.clone();
    assert_eq!(post(&h, &t, ok).await.0, StatusCode::CREATED);
    let mut bad = body("li-4");
    bad["amount"] = json!(total.as_u64().unwrap() + 1);
    let (s, r) = post(&h, &t, bad).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "{r}");
    assert!(r["error"].as_str().unwrap().contains("does not match"));
    let mut bad_tax = body("li-5");
    bad_tax["tax"] = json!(1);
    assert_eq!(post(&h, &t, bad_tax).await.0, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn line_limits_are_enforced() {
    let h = harness().await;
    let t = tenant(&h).await;
    let line = |n: &str| json!({"name": n, "unit_amount": 10});
    let hundred: Vec<Value> = (0..100).map(|i| line(&format!("i{i}"))).collect();
    let (s, p) = post(
        &h,
        &t,
        json!({"reference": "l100", "currency": "EUR", "line_items": hundred}),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{p}");
    assert_eq!(p["amount"], 1000);
    let many: Vec<Value> = (0..101).map(|i| line(&format!("i{i}"))).collect();
    let (s, _) = post(
        &h,
        &t,
        json!({"reference": "l101", "currency": "EUR", "line_items": many}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(
        &h,
        &t,
        json!({"reference": "ln", "currency": "EUR", "line_items": [line(&"n".repeat(128))]}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(
        &h,
        &t,
        json!({"reference": "ld", "currency": "EUR", "discount": {"amount": 1}, "line_items": []}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = post(
        &h,
        &t,
        json!({"reference": "nodisc", "currency": "EUR", "amount": 100, "discount": {"amount": 1}}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn payments_without_lines_are_unchanged() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = new_payment(&h, &t, eur("plain", 1500)).await;
    assert_eq!(p["amount"], 1500);
    assert_eq!(p["line_items"], json!([]));
    assert!(p["discount"].is_null());
}

#[tokio::test]
async fn webhook_and_pay_page_carry_the_lines() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let (_, p) = post(&h, &t, body("li-wh")).await;
    let id = p["id"].as_str().unwrap().to_string();

    let (s, state) = call(
        &h.app,
        "GET",
        &format!("/pay/{id}/state"),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(state["line_items"].as_array().unwrap().len(), 2);
    assert_eq!(state["discount"]["code"], "SAVE10");
    assert_eq!(state["subtotal"], 1499);

    let (_, r) = attempt(&h, &t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    let sink = ProviderSink {
        service: h.service.clone(),
        provider_id: ln,
    };
    sink.event(
        ProviderEvent::Paid {
            lookup: hash.clone(),
            reference: hash.clone(),
            amount: None,
            confirmed: true,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();
    h.service.deliver_due().await.unwrap();
    let events = t.receiver.events();
    let payment = &events.last().unwrap()["data"]["payment"];
    assert_eq!(payment["line_items"][0]["name"], "Widget");
    assert_eq!(payment["line_items"].as_array().unwrap().len(), 2);
    assert_eq!(payment["discount"]["amount"], 150);
}

async fn paid_payment(h: &Harness, t: &TenantFixture, b: Value) -> String {
    let ln = provider(h, t, "lightning", json!({}), None).await;
    let (s, p) = post(h, t, b).await;
    assert_eq!(s, StatusCode::CREATED, "{p}");
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: h.service.clone(),
        provider_id: ln,
    }
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
    id
}

async fn set_seller(h: &Harness, t: &TenantFixture) {
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/settings", t.id),
        Some(ADMIN),
        json!({
            "legal_name": "Acme <b>Ltd</b>",
            "address": "1 Main St\nDublin",
            "tax_id": "IE1234567X",
            "support_email": "help@acme.test"
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["legal_name"], "Acme <b>Ltd</b>");
}

#[tokio::test]
async fn receipt_shows_the_sale_and_hides_secrets() {
    let h = harness().await;
    let t = tenant(&h).await;
    set_seller(&h, &t).await;
    let id = paid_payment(&h, &t, body("rcpt-1")).await;
    let r = send(
        &h.app,
        "GET",
        &format!("/receipt/{id}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(
        r.headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/html")
    );
    let html = r.text;
    for needle in [
        "Acme &lt;b&gt;Ltd&lt;/b&gt;",
        "1 Main St",
        "Dublin",
        "IE1234567X",
        "help@acme.test",
        "buyer@example.com",
        "Widget",
        "SKU W-1",
        "Shipping",
        "SAVE10",
        "Lightning",
        "rcpt-1",
        &id,
    ] {
        assert!(html.contains(needle), "missing {needle}");
    }
    assert!(!html.contains("<b>Ltd"));
    assert!(!html.contains("<script>"));
    for secret in [
        t.key.as_str(),
        t.webhook_secret.as_str(),
        "api_key",
        "bolt11",
        "payment_hash",
        "preimage",
        "metadata",
    ] {
        assert!(!html.contains(secret), "leaks {secret}");
    }
}

#[tokio::test]
async fn receipt_exists_only_for_paid_payments_and_the_right_id() {
    let h = harness().await;
    let t = tenant(&h).await;
    let (_, pending) = post(&h, &t, body("rcpt-2")).await;
    let pid = pending["id"].as_str().unwrap();
    let get = |id: String| {
        let app = h.app.clone();
        async move {
            send(
                &app,
                "GET",
                &format!("/receipt/{id}"),
                None,
                &[],
                Value::Null,
            )
            .await
        }
    };
    assert_eq!(get(pid.to_string()).await.status, StatusCode::NOT_FOUND);
    assert_eq!(get("00".repeat(32)).await.status, StatusCode::NOT_FOUND);
    assert_eq!(get("nothex".into()).await.status, StatusCode::NOT_FOUND);
    let id = paid_payment(&h, &t, body("rcpt-3")).await;
    assert_eq!(get(id).await.status, StatusCode::OK);
}

#[tokio::test]
async fn plain_payment_receipt_works_without_lines() {
    let h = harness().await;
    let t = tenant(&h).await;
    let id = paid_payment(&h, &t, eur("rcpt-4", 2500)).await;
    let r = send(
        &h.app,
        "GET",
        &format!("/receipt/{id}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.text.contains("25.00 EUR"));
}

#[tokio::test]
async fn console_settings_validate_seller_fields() {
    let h = harness().await;
    let t = tenant(&h).await;
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/settings", t.id),
        Some(ADMIN),
        json!({"support_email": "not an email"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

fn request(lines: Vec<payment_gateway::invoice::ChargeLine>, amount: u64) -> AttemptRequest {
    AttemptRequest {
        payment_id: [7u8; 32],
        amount,
        currency: "EUR".into(),
        description: "Order".into(),
        expires: chrono::Utc::now() + chrono::Duration::hours(1),
        success_url: Some("https://shop.example/ok".into()),
        cancel_url: Some("https://shop.example/no".into()),
        customer_ref: None,
        customer_email: Some("a@b.c".into()),
        option: None,
        save_method: false,
        receive: None,
        lines,
    }
}

fn charge_lines() -> Vec<payment_gateway::invoice::ChargeLine> {
    use payment_gateway::invoice::ChargeLine;
    vec![
        ChargeLine {
            name: "Widget".into(),
            description: Some("Blue".into()),
            quantity: 3,
            unit_amount: 360,
        },
        ChargeLine {
            name: "Shipping".into(),
            description: None,
            quantity: 1,
            unit_amount: 500,
        },
    ]
}

fn fixture(name: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap())
        .unwrap()
}

#[tokio::test]
async fn stripe_checkout_receives_every_line() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fixture("stripe/checkout_session.json")),
        )
        .mount(&server)
        .await;
    let p = StripeProvider::new(&json!({"secret_key": "sk_test_x", "url": server.uri()})).unwrap();
    p.create(request(charge_lines(), 1580)).await.unwrap();
    let got = String::from_utf8(server.received_requests().await.unwrap()[0].body.clone()).unwrap();
    let form: Vec<(String, String)> = url::form_urlencoded::parse(got.as_bytes())
        .into_owned()
        .collect();
    let find = |k: &str| form.iter().find(|(a, _)| a == k).map(|(_, v)| v.clone());
    assert_eq!(find("line_items[0][quantity]").as_deref(), Some("3"));
    assert_eq!(
        find("line_items[0][price_data][unit_amount]").as_deref(),
        Some("360")
    );
    assert_eq!(
        find("line_items[0][price_data][product_data][name]").as_deref(),
        Some("Widget")
    );
    assert_eq!(
        find("line_items[0][price_data][product_data][description]").as_deref(),
        Some("Blue")
    );
    assert_eq!(
        find("line_items[1][price_data][product_data][name]").as_deref(),
        Some("Shipping")
    );
    assert_eq!(
        find("line_items[1][price_data][unit_amount]").as_deref(),
        Some("500")
    );
    assert_eq!(find("line_items[2][quantity]"), None);
}

#[tokio::test]
async fn stripe_falls_back_to_one_line_without_items() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fixture("stripe/checkout_session.json")),
        )
        .mount(&server)
        .await;
    let p = StripeProvider::new(&json!({"secret_key": "sk_test_x", "url": server.uri()})).unwrap();
    p.create(request(vec![], 2500)).await.unwrap();
    let got = String::from_utf8(server.received_requests().await.unwrap()[0].body.clone()).unwrap();
    assert!(got.contains("line_items%5B0%5D%5Bprice_data%5D%5Bunit_amount%5D=2500"));
    assert!(!got.contains("line_items%5B1%5D"));
}

#[tokio::test]
async fn revolut_order_receives_every_line() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/orders"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fixture("revolut/order_created.json")),
        )
        .mount(&server)
        .await;
    let p = RevolutProvider::new(&json!({
        "url": server.uri(), "token": "tok", "api_version": "2024-09-01", "public_key": "pk"
    }))
    .unwrap();
    p.create(request(charge_lines(), 1580)).await.unwrap();
    let sent: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    let items = sent["line_items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["name"], "Widget");
    assert_eq!(items[0]["quantity"]["value"], 3);
    assert_eq!(items[0]["unit_price_amount"], 360);
    assert_eq!(items[0]["total_amount"], 1080);
    assert_eq!(items[1]["name"], "Shipping");
    assert_eq!(sent["amount"], 1580);

    server.reset().await;
    Mock::given(method("POST"))
        .and(path("/api/orders"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(fixture("revolut/order_created.json")),
        )
        .mount(&server)
        .await;
    p.create(request(vec![], 2500)).await.unwrap();
    let sent: Value =
        serde_json::from_slice(&server.received_requests().await.unwrap()[0].body).unwrap();
    assert!(sent.get("line_items").is_none_or(Value::is_null));
}

#[tokio::test]
async fn card_attempt_passes_the_stored_lines_to_the_provider() {
    let h = harness().await;
    let t = tenant(&h).await;
    let sp = provider(&h, &t, "stripe", json!({}), None).await;
    let (_, p) = post(&h, &t, body("li-card")).await;
    let id = p["id"].as_str().unwrap();
    let (s, r) = attempt(&h, &t, id, json!({"method": "stripe"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let sent = mock(&h, sp)
        .created
        .lock()
        .unwrap()
        .last()
        .cloned()
        .unwrap();
    assert_eq!(sent.lines.len(), 2);
    let sum: u64 = sent.lines.iter().map(|l| l.quantity * l.unit_amount).sum();
    assert_eq!(sum, p["amount"].as_u64().unwrap());
}

#[tokio::test]
async fn validation_errors_name_the_field() {
    let h = harness().await;
    let t = tenant(&h).await;
    let (_, p) = post(&h, &t, body("fld-0")).await;
    let mut bad = body("fld-1");
    bad["amount"] = json!(p["amount"].as_u64().unwrap() + 1);
    let (s, r) = post(&h, &t, bad).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    assert_eq!(r["error_detail"]["field"], "amount");
    assert_eq!(r["error_detail"]["code"], "invalid_request");
    let mut bad_tax = body("fld-2");
    bad_tax["tax"] = json!(1);
    assert_eq!(
        post(&h, &t, bad_tax).await.1["error_detail"]["field"],
        "tax"
    );
    let many: Vec<Value> = (0..101)
        .map(|_| json!({"name": "x", "unit_amount": 1}))
        .collect();
    let (_, r) = post(&h, &t, json!({"currency": "EUR", "line_items": many})).await;
    assert_eq!(r["error_detail"]["field"], "line_items");
    let (_, r) = post(
        &h,
        &t,
        json!({"currency": "EUR", "line_items": [{"name": "x", "unit_amount": 100}], "discount": {"percent": 150}}),
    )
    .await;
    assert_eq!(r["error_detail"]["field"], "discount");
    let (_, r) = post(
        &h,
        &t,
        json!({"currency": "EUR", "amount": 5, "discount": {"amount": 1}}),
    )
    .await;
    assert_eq!(r["error_detail"]["field"], "discount");
}

#[tokio::test]
async fn endpoint_deliveries_carry_the_lines() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let extra = Receiver::start().await;
    let sealed = h.service.sealer.seal(b"whsec_extra").unwrap();
    payment_gateway::db::insert_endpoint(&h.pool, t.id, &extra.url, &sealed, None, None)
        .await
        .unwrap();
    let (_, p) = post(&h, &t, body("li-ep")).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(&h, &t, &id, json!({"method": "lightning"})).await;
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: h.service.clone(),
        provider_id: ln,
    }
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
    h.service.deliver_due().await.unwrap();
    for rx in [&t.receiver, &extra] {
        let events = rx.events();
        let payment = &events.last().unwrap()["data"]["payment"];
        assert_eq!(payment["line_items"].as_array().unwrap().len(), 2);
        assert_eq!(payment["subtotal"], 1499);
        assert_eq!(payment["discount"]["code"], "SAVE10");
        assert_eq!(payment["amount"], p["amount"]);
        assert!(
            payment["receipt_url"]
                .as_str()
                .unwrap()
                .contains("/receipt/")
        );
    }
}

#[tokio::test]
async fn openapi_documents_line_items_and_the_payment_view() {
    let h = harness().await;
    let r = send(&h.app, "GET", "/v1/openapi.json", None, &[], Value::Null).await;
    let schemas = &r.json["components"]["schemas"];
    let create = &schemas["CreatePayment"]["properties"];
    for k in ["line_items", "discount", "tax", "amount"] {
        assert!(create[k].is_object(), "CreatePayment.{k}");
    }
    assert_eq!(create["line_items"]["maxItems"], 100);
    assert_eq!(schemas["CreatePayment"]["required"], json!(["currency"]));
    let view = &schemas["Payment"]["properties"];
    for k in [
        "line_items",
        "subtotal",
        "discount",
        "tax",
        "amount",
        "receipt_url",
    ] {
        assert!(view[k].is_object(), "Payment.{k}");
    }
    assert!(r.json["paths"]["/payments"]["post"]["responses"]["200"]["content"]["application/json"]["schema"]["$ref"]
        .as_str()
        .unwrap()
        .ends_with("/Payment"));
    assert!(r.json["paths"].get("/receipt/{id}").is_none());
}

fn cents(text: &str) -> i64 {
    let n = text.trim().trim_end_matches(" EUR").replace(',', "");
    let (sign, n) = match n.strip_prefix('-') {
        Some(rest) => (-1, rest.to_string()),
        None => (1, n),
    };
    let (whole, frac) = n.split_once('.').unwrap();
    sign * (whole.parse::<i64>().unwrap() * 100 + frac.parse::<i64>().unwrap())
}

fn last_cells(html: &str, from: &str, to: &str) -> Vec<String> {
    let start = html.find(from).unwrap();
    let section = &html[start..start + html[start..].find(to).unwrap()];
    section
        .split("<tr>")
        .skip(1)
        .map(|row| {
            let cell = row.rsplit("<td class=\"num money\">").next().unwrap();
            cell.split("</td>").next().unwrap().to_string()
        })
        .collect()
}

#[tokio::test]
async fn receipt_line_totals_sum_to_the_subtotal() {
    let h = harness().await;
    let t = tenant(&h).await;
    let fixture = json!({
        "reference": "rcpt-sum",
        "currency": "EUR",
        "line_items": [
            {"name": "Widget", "quantity": 3, "unit_amount": 333, "tax_rate": 8.25},
            {"name": "Shipping", "unit_amount": 500, "tax_rate": 23},
            {"name": "Gift wrap", "unit_amount": 250}
        ],
        "discount": {"percent": 10, "code": "SAVE10"}
    });
    let id = paid_payment(&h, &t, fixture).await;
    let r = send(
        &h.app,
        "GET",
        &format!("/receipt/{id}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    let lines = last_cells(&r.text, "<tbody>", "</tbody>");
    assert_eq!(lines.len(), 3);
    let sum: i64 = lines.iter().map(|c| cents(c)).sum();
    let foot = last_cells(&r.text, "<tfoot>", "</tfoot>");
    assert_eq!(cents(&foot[0]), 1749, "{foot:?}");
    assert_eq!(sum, cents(&foot[0]), "{lines:?} {foot:?}");
    assert_eq!(cents(&foot[1]), -175);
    assert_eq!(cents(&foot[2]), 178);
    assert_eq!(cents(&foot[0]) + cents(&foot[1]) + cents(&foot[2]), 1752);
}

#[tokio::test]
async fn receipt_names_the_seller_once_and_fills_the_card_width() {
    let h = harness().await;
    let t = tenant(&h).await;
    set_seller(&h, &t).await;
    let id = paid_payment(&h, &t, body("rcpt-once")).await;
    let r = send(
        &h.app,
        "GET",
        &format!("/receipt/{id}"),
        None,
        &[],
        Value::Null,
    )
    .await;
    assert_eq!(r.text.matches("Acme &lt;b&gt;Ltd&lt;/b&gt;").count(), 1);
    let css_href = r
        .text
        .split('"')
        .find(|s| s.starts_with("/static/assets/receipt-") && s.ends_with(".css"))
        .unwrap()
        .to_string();
    let css = send(&h.app, "GET", &css_href, None, &[], Value::Null).await;
    assert!(css.text.contains(".lines{width:100%"));
    assert!(!css.text.contains("nth-child"));
}
