use async_trait::async_trait;
use futures::Stream;
use hmac::{Hmac, Mac};
use payment_gateway::db::{AttemptRow, SavedMethodRow};
use payment_gateway::providers::lightning::LightningProvider;
use payment_gateway::providers::onchain::OnChainProviderImpl;
use payment_gateway::providers::revolut::RevolutProvider;
use payment_gateway::providers::stripe::StripeProvider;
use payment_gateway::providers::{
    AttemptRequest, EventSink, Provider, ProviderEvent, RefundRequest,
};
use payments_rs::lightning::{
    AddInvoiceRequest, AddInvoiceResponse, InvoiceUpdate, LightningNode, PayInvoiceRequest,
    PayInvoiceResponse,
};
use payments_rs::onchain::{ChainPaymentUpdate, MockOnChainProvider};
use serde_json::{Value, json};
use sha2::Sha256;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn request(amount: u64, currency: &str) -> AttemptRequest {
    AttemptRequest {
        payment_id: [7u8; 32],
        amount,
        currency: currency.into(),
        description: "Pro plan".into(),
        expires: chrono::Utc::now() + chrono::Duration::hours(1),
        success_url: Some("https://shop.example/ok".into()),
        cancel_url: Some("https://shop.example/no".into()),
        customer_ref: Some("cust".into()),
        customer_email: Some("a@b.c".into()),
        option: None,
        save_method: false,
        receive: None,
    }
}

fn attempt_row(lookup: &str) -> AttemptRow {
    AttemptRow {
        id: 1,
        payment_id: vec![7; 32],
        provider_config_id: 1,
        watch_provider_id: 1,
        method: "x".into(),
        option_key: String::new(),
        base_amount: 1000,
        processing_fee: 0,
        currency: "EUR".into(),
        charge_amount: 1000,
        charge_currency: "EUR".into(),
        rate: 1.0,
        lookup: lookup.into(),
        external_id: None,
        details: "{}".into(),
        status: "paid".into(),
        created: chrono::Utc::now(),
        expires: chrono::Utc::now(),
        paid_at: None,
        save_method: false,
        shared_provider_id: None,
        platform_fee_bps: None,
        platform_fee_base: None,
        platform_fee: None,
        platform_net: None,
    }
}

fn bolt11(amount_msat: Option<u64>) -> String {
    use bitcoin::hashes::{Hash, sha256};
    use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
    let secret: [u8; 32] = rand::random();
    let mut b = InvoiceBuilder::new(Currency::Regtest)
        .description("t".into())
        .payment_hash(sha256::Hash::from_slice(&secret).unwrap())
        .payment_secret(PaymentSecret(secret))
        .current_timestamp()
        .min_final_cltv_expiry_delta(144);
    if let Some(a) = amount_msat {
        b = b.amount_milli_satoshis(a);
    }
    let key = bitcoin::secp256k1::SecretKey::from_slice(&[0xcd; 32]).unwrap();
    b.build_signed(|m| bitcoin::secp256k1::Secp256k1::new().sign_ecdsa_recoverable(m, &key))
        .unwrap()
        .to_string()
}

#[derive(Default)]
struct FakeNode {
    added: Mutex<Vec<AddInvoiceRequest>>,
    paid: Mutex<Vec<String>>,
    cancelled: Mutex<Vec<Vec<u8>>>,
    updates: Mutex<Vec<InvoiceUpdate>>,
    from: Mutex<Vec<Option<Vec<u8>>>>,
}

#[async_trait]
impl LightningNode for FakeNode {
    async fn add_invoice(&self, req: AddInvoiceRequest) -> anyhow::Result<AddInvoiceResponse> {
        let pr = bolt11(Some(req.amount));
        self.added.lock().unwrap().push(req);
        AddInvoiceResponse::from_invoice(&pr, None)
    }
    async fn cancel_invoice(&self, id: &[u8]) -> anyhow::Result<()> {
        self.cancelled.lock().unwrap().push(id.to_vec());
        Ok(())
    }
    async fn pay_invoice(&self, req: PayInvoiceRequest) -> anyhow::Result<PayInvoiceResponse> {
        self.paid.lock().unwrap().push(req.invoice);
        Ok(PayInvoiceResponse {
            payment_hash: "abcd".into(),
            payment_preimage: None,
            amount_msat: 0,
            fee_msat: 0,
        })
    }
    async fn subscribe_invoices(
        &self,
        from: Option<Vec<u8>>,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = InvoiceUpdate> + Send>>> {
        self.from.lock().unwrap().push(from);
        let updates: Vec<_> = self.updates.lock().unwrap().drain(..).collect();
        Ok(Box::pin(futures::stream::iter(updates)))
    }
}

struct Collect(Mutex<Vec<(ProviderEvent, Option<String>)>>);

#[async_trait]
impl EventSink for Collect {
    async fn event(&self, e: ProviderEvent, c: Option<String>) -> anyhow::Result<()> {
        self.0.lock().unwrap().push((e, c));
        Ok(())
    }
}

#[tokio::test]
async fn lightning_creates_a_real_invoice_for_the_amount_and_hash_is_the_lookup() {
    let node = Arc::new(FakeNode::default());
    let p = LightningProvider::new(node.clone());
    let created = p.create(request(150_000_000, "BTC")).await.unwrap();
    assert_eq!(created.lookup.len(), 64);
    assert_eq!(created.details["payment_hash"], created.lookup.as_str());
    let pr = created.details["bolt11"].as_str().unwrap();
    assert_eq!(
        payment_gateway::providers::lightning::invoice_amount_msat(pr).unwrap(),
        Some(150_000_000)
    );
    assert_eq!(node.added.lock().unwrap()[0].amount, 150_000_000);
}

#[tokio::test]
async fn lightning_watcher_forwards_settled_invoices_with_resume_cursor_and_errors_out_when_the_stream_ends()
 {
    let node = Arc::new(FakeNode::default());
    node.updates.lock().unwrap().extend([
        InvoiceUpdate::Created {
            payment_hash: "aa".into(),
            payment_request: "x".into(),
        },
        InvoiceUpdate::Settled {
            payment_hash: "ab12".into(),
            preimage: None,
            external_id: None,
        },
    ]);
    let p = LightningProvider::new(node.clone());
    let sink = Arc::new(Collect(Default::default()));
    let result = p.watch(sink.clone(), Some("ab12".into())).await;
    assert!(
        result.is_err(),
        "ended streams must surface so the worker reconnects"
    );
    let seen = sink.0.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].1.as_deref(), Some("ab12"));
    assert!(
        matches!(&seen[0].0, ProviderEvent::Paid { lookup, confirmed: true, .. } if lookup == "ab12")
    );
    assert_eq!(node.from.lock().unwrap()[0], Some(vec![0xab, 0x12]));
}

#[tokio::test]
async fn lightning_watcher_fails_on_stream_error() {
    let node = Arc::new(FakeNode::default());
    node.updates
        .lock()
        .unwrap()
        .push(InvoiceUpdate::Error("boom".into()));
    let p = LightningProvider::new(node);
    let err = p
        .watch(Arc::new(Collect(Default::default())), None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("boom"));
}

#[tokio::test]
async fn lightning_refund_pays_an_invoice_within_the_refundable_amount_only() {
    let node = Arc::new(FakeNode::default());
    let p = LightningProvider::new(node.clone());
    let row = attempt_row("x");
    let ok = bolt11(Some(50_000_000));
    let out = p
        .refund(
            &row,
            RefundRequest {
                amount: 100_000_000,
                currency: "BTC".into(),
                destination: Some(format!("lightning:{ok}")),
            },
        )
        .await
        .unwrap();
    assert_eq!(out.external_id.as_deref(), Some("abcd"));
    assert_eq!(node.paid.lock().unwrap()[0], ok);

    let too_big = bolt11(Some(200_000_000));
    assert!(
        p.refund(
            &row,
            RefundRequest {
                amount: 100_000_000,
                currency: "BTC".into(),
                destination: Some(too_big)
            }
        )
        .await
        .is_err()
    );
    let no_amount = bolt11(None);
    assert!(
        p.refund(
            &row,
            RefundRequest {
                amount: 100_000_000,
                currency: "BTC".into(),
                destination: Some(no_amount)
            }
        )
        .await
        .is_err()
    );
    assert!(
        p.refund(
            &row,
            RefundRequest {
                amount: 1,
                currency: "BTC".into(),
                destination: None
            }
        )
        .await
        .is_err()
    );
    assert!(
        p.refund(
            &row,
            RefundRequest {
                amount: 1,
                currency: "BTC".into(),
                destination: Some("garbage".into())
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        node.paid.lock().unwrap().len(),
        1,
        "rejected refunds never reach the node"
    );
}

#[tokio::test]
async fn lightning_cancel_decodes_the_hash() {
    let node = Arc::new(FakeNode::default());
    let p = LightningProvider::new(node.clone());
    p.cancel(&attempt_row("0a0b")).await.unwrap();
    assert_eq!(node.cancelled.lock().unwrap()[0], vec![0x0a, 0x0b]);
}

fn chain(addresses: &[&str], updates: Vec<(u64, ChainPaymentUpdate)>) -> OnChainProviderImpl {
    OnChainProviderImpl::new(Arc::new(MockOnChainProvider::new(
        addresses.iter().map(|a| a.to_string()).collect(),
        updates,
    )))
}

#[tokio::test]
async fn onchain_hands_out_addresses_with_exact_sat_amounts_and_a_bip21_uri() {
    let p = chain(&["bc1qaaa"], vec![]);
    let c = p.create(request(150_000_000, "BTC")).await.unwrap();
    assert_eq!(c.lookup, "bc1qaaa");
    assert_eq!(c.details["amount_sats"], 150_000);
    assert_eq!(c.details["uri"], "bitcoin:bc1qaaa?amount=0.0015");
    let c = p.create(request(1_001, "BTC")).await.unwrap();
    assert_eq!(c.details["amount_sats"], 2, "never under-asks");
}

#[tokio::test]
async fn onchain_watcher_maps_detected_and_confirmed_with_txid_vout_references() {
    let p = chain(
        &[],
        vec![
            (
                1,
                ChainPaymentUpdate::Detected {
                    address: "a".into(),
                    txid: "t1".into(),
                    vout: 0,
                    amount_msat: 5_000,
                    confirmations: 0,
                    label: None,
                },
            ),
            (
                2,
                ChainPaymentUpdate::Confirmed {
                    address: "a".into(),
                    txid: "t1".into(),
                    vout: 0,
                    amount_msat: 5_000,
                    confirmations: 3,
                    label: None,
                },
            ),
        ],
    );
    let sink = Arc::new(Collect(Default::default()));
    assert!(p.watch(sink.clone(), None).await.is_err());
    let seen: Vec<_> = sink
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|(e, _)| e.clone())
        .collect();
    assert_eq!(seen.len(), 2);
    assert!(
        matches!(&seen[0], ProviderEvent::Paid { reference, confirmed: false, amount: Some(5_000), .. } if reference == "t1:0")
    );
    assert!(
        matches!(&seen[1], ProviderEvent::Paid { reference, confirmed: true, .. } if reference == "t1:0")
    );
}

#[tokio::test]
async fn onchain_watcher_fails_on_chain_errors_and_refund_sends_coins() {
    let p = chain(&[], vec![(1, ChainPaymentUpdate::Error("lnd down".into()))]);
    let err = p
        .watch(Arc::new(Collect(Default::default())), None)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("lnd down"));

    let mock = Arc::new(MockOnChainProvider::new(vec![], vec![]));
    let p = OnChainProviderImpl::new(mock.clone());
    let out = p
        .refund(
            &attempt_row("x"),
            RefundRequest {
                amount: 2_000_000,
                currency: "BTC".into(),
                destination: Some("bc1qdest".into()),
            },
        )
        .await
        .unwrap();
    assert!(out.external_id.is_some());
    let sends = mock.sends();
    assert_eq!(sends[0].outputs[0].address, "bc1qdest");
    assert_eq!(sends[0].outputs[0].amount.value(), 2_000_000);
    assert!(
        p.refund(
            &attempt_row("x"),
            RefundRequest {
                amount: 1,
                currency: "BTC".into(),
                destination: None
            }
        )
        .await
        .is_err()
    );
}

fn stripe_for(url: &str) -> StripeProvider {
    StripeProvider::new(&json!({"secret_key": "sk_test", "url": url, "webhook_secret": "whsec_x"}))
        .unwrap()
}

fn stripe_sig(secret: &str, body: &str) -> String {
    let ts = chrono::Utc::now().timestamp();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("{ts}.{body}").as_bytes());
    format!("t={ts},v1={}", hex::encode(mac.finalize().into_bytes()))
}

#[tokio::test]
async fn stripe_checkout_session_carries_amount_currency_urls_and_save_flags() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/checkout/sessions"))
        .and(header("authorization", "Bearer sk_test"))
        .and(body_string_contains("unit_amount%5D=1045"))
        .and(body_string_contains("currency%5D=eur"))
        .and(body_string_contains(
            "success_url=https%3A%2F%2Fshop.example%2Fok",
        ))
        .and(body_string_contains("setup_future_usage%5D=off_session"))
        .and(body_string_contains("client_reference_id="))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"id": "cs_123", "url": "https://checkout.stripe.com/c/pay/cs_123"}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let mut req = request(1045, "EUR");
    req.save_method = true;
    let c = stripe_for(&server.uri()).create(req).await.unwrap();
    assert_eq!(c.lookup, "cs_123");
    assert_eq!(
        c.details["checkout_url"],
        "https://checkout.stripe.com/c/pay/cs_123"
    );
}

#[tokio::test]
async fn stripe_api_errors_surface_the_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(402)
                .set_body_json(json!({"error": {"message": "Your card was declined"}})),
        )
        .mount(&server)
        .await;
    let err = stripe_for(&server.uri())
        .create(request(100, "EUR"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Your card was declined"), "{err}");
}

#[tokio::test]
async fn stripe_webhooks_need_a_valid_signature_and_only_paid_sessions_settle() {
    let p = stripe_for("http://127.0.0.1:1");
    let event = |kind: &str, status: &str| {
        json!({"id": "evt_1", "type": kind, "data": {"object": {"id": "cs_9", "payment_status": status, "amount_total": 1045, "metadata": {}}}}).to_string()
    };
    let headers = |sig: String| HashMap::from([("stripe-signature".to_string(), sig)]);

    let body = event("checkout.session.completed", "paid");
    let events = p
        .handle_webhook(&headers(stripe_sig("whsec_x", &body)), body.as_bytes())
        .await
        .unwrap();
    assert_eq!(
        events,
        vec![ProviderEvent::Paid {
            lookup: "cs_9".into(),
            reference: "cs_9".into(),
            amount: Some(1045),
            confirmed: true,
            saved: None
        }]
    );

    assert!(
        p.handle_webhook(&headers(stripe_sig("wrong", &body)), body.as_bytes())
            .await
            .is_err()
    );
    assert!(
        p.handle_webhook(&HashMap::new(), body.as_bytes())
            .await
            .is_err()
    );

    let unpaid = event("checkout.session.completed", "unpaid");
    assert!(
        p.handle_webhook(&headers(stripe_sig("whsec_x", &unpaid)), unpaid.as_bytes())
            .await
            .unwrap()
            .is_empty()
    );
    let failed = event("checkout.session.async_payment_failed", "unpaid");
    let evs = p
        .handle_webhook(&headers(stripe_sig("whsec_x", &failed)), failed.as_bytes())
        .await
        .unwrap();
    assert!(matches!(&evs[0], ProviderEvent::Failed { lookup, .. } if lookup == "cs_9"));
    let other = event("charge.refunded", "paid");
    assert!(
        p.handle_webhook(&headers(stripe_sig("whsec_x", &other)), other.as_bytes())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn stripe_reads_the_saved_card_from_the_session_when_asked() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/payment_intents/pi_1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "pi_1", "payment_method": "pm_1"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/v1/payment_methods/pm_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "pm_1", "card": {"brand": "visa", "last4": "4242", "exp_month": 4, "exp_year": 2030}})))
        .mount(&server).await;
    let p = stripe_for(&server.uri());
    let body = json!({"id": "evt", "type": "checkout.session.completed", "data": {"object": {
        "id": "cs_1", "payment_status": "paid", "amount_total": 500, "customer": "cus_1",
        "payment_intent": "pi_1", "metadata": {"save_method": "1"}}}})
    .to_string();
    let h = HashMap::from([("stripe-signature".to_string(), stripe_sig("whsec_x", &body))]);
    let evs = p.handle_webhook(&h, body.as_bytes()).await.unwrap();
    let ProviderEvent::Paid { saved: Some(s), .. } = &evs[0] else {
        panic!("{evs:?}")
    };
    assert_eq!(
        (
            s.customer_id.as_str(),
            s.method_id.as_str(),
            s.last4.as_deref()
        ),
        ("cus_1", "pm_1", Some("4242"))
    );
    assert_eq!((s.exp_month, s.exp_year), (Some(4), Some(2030)));
}

#[tokio::test]
async fn stripe_refund_resolves_the_payment_intent_and_sends_the_amount() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/checkout/sessions/cs_1"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "cs_1", "payment_intent": "pi_77"})),
        )
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/v1/refunds"))
        .and(body_string_contains("payment_intent=pi_77"))
        .and(body_string_contains("amount=300"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "re_1"})))
        .expect(1)
        .mount(&server)
        .await;
    let out = stripe_for(&server.uri())
        .refund(
            &attempt_row("cs_1"),
            RefundRequest {
                amount: 300,
                currency: "EUR".into(),
                destination: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(out.external_id.as_deref(), Some("re_1"));
}

#[tokio::test]
async fn stripe_off_session_charge_requires_success() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .and(body_string_contains("off_session=true"))
        .and(body_string_contains("payment_method=pm_9"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"id": "pi_5", "status": "succeeded"})),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    let saved = SavedMethodRow {
        id: 1,
        tenant_id: 1,
        customer_ref: "c".into(),
        provider_config_id: 1,
        external_customer_id: "cus_9".into(),
        external_id: "pm_9".into(),
        brand: None,
        last4: None,
        exp_month: None,
        exp_year: None,
        enabled: true,
        created: chrono::Utc::now(),
    };
    let p = stripe_for(&server.uri());
    let c = p.charge_saved(&saved, request(700, "EUR")).await.unwrap();
    assert!(c.completed);
    assert_eq!(c.lookup, "pi_5");

    Mock::given(method("POST"))
        .and(path("/v1/payment_intents"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"id": "pi_6", "status": "requires_action"})),
        )
        .mount(&server)
        .await;
    assert!(p.charge_saved(&saved, request(700, "EUR")).await.is_err());
}

fn revolut_for(url: &str, secret: Option<&str>) -> RevolutProvider {
    let mut cfg =
        json!({"url": url, "token": "tok", "api_version": "2024-09-01", "public_key": "pk"});
    if let Some(s) = secret {
        cfg["webhook_secret"] = json!(s);
    }
    RevolutProvider::new(&cfg).unwrap()
}

fn order_json(id: &str, state: &str) -> Value {
    json!({
        "id": id, "token": "t", "state": state,
        "created_at": "2025-01-01T00:00:00Z", "updated_at": "2025-01-01T00:00:00Z",
        "amount": 1000, "currency": "EUR", "outstanding_amount": 0,
        "checkout_url": format!("https://checkout.revolut.com/payment-link/{id}")
    })
}

fn revolut_sig(secret: &str, body: &str) -> HashMap<String, String> {
    let ts = chrono::Utc::now().timestamp_millis().to_string();
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(format!("v1.{ts}.{body}").as_bytes());
    HashMap::from([
        (
            "revolut-signature".to_string(),
            format!("v1={}", hex::encode(mac.finalize().into_bytes())),
        ),
        ("revolut-request-timestamp".to_string(), ts),
    ])
}

#[tokio::test]
async fn revolut_order_uses_minor_units_and_returns_the_checkout_link() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/orders"))
        .and(header("authorization", "Bearer tok"))
        .and(header("revolut-api-version", "2024-09-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(order_json("ord_1", "pending")))
        .expect(1)
        .mount(&server)
        .await;
    let c = revolut_for(&server.uri(), None)
        .create(request(1000, "EUR"))
        .await
        .unwrap();
    assert_eq!(c.lookup, "ord_1");
    assert_eq!(
        c.details["checkout_url"],
        "https://checkout.revolut.com/payment-link/ord_1"
    );
    let received = server.received_requests().await.unwrap();
    let body: Value = serde_json::from_slice(&received[0].body).unwrap();
    assert_eq!(body["amount"], 1000);
    assert_eq!(body["currency"], "EUR");
}

#[tokio::test]
async fn revolut_saving_a_card_needs_an_email() {
    let mut req = request(1000, "EUR");
    req.save_method = true;
    req.customer_email = None;
    assert!(
        revolut_for("http://127.0.0.1:1", None)
            .create(req)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn revolut_webhook_verifies_signature_then_confirms_the_order_state_with_the_api() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/orders/ord_1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(order_json("ord_1", "completed")))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/orders/ord_2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(order_json("ord_2", "pending")))
        .mount(&server)
        .await;
    let p = revolut_for(&server.uri(), Some("wsk_x"));
    let body =
        json!({"event": "ORDER_COMPLETED", "order_id": "ord_1", "merchant_order_ext_ref": null})
            .to_string();

    let evs = p
        .handle_webhook(&revolut_sig("wsk_x", &body), body.as_bytes())
        .await
        .unwrap();
    assert_eq!(
        evs,
        vec![ProviderEvent::Paid {
            lookup: "ord_1".into(),
            reference: "ord_1".into(),
            amount: Some(1000),
            confirmed: true,
            saved: None
        }]
    );

    assert!(
        p.handle_webhook(&revolut_sig("other", &body), body.as_bytes())
            .await
            .is_err()
    );

    let spoof =
        json!({"event": "ORDER_COMPLETED", "order_id": "ord_2", "merchant_order_ext_ref": null})
            .to_string();
    let evs = p
        .handle_webhook(&revolut_sig("wsk_x", &spoof), spoof.as_bytes())
        .await
        .unwrap();
    assert!(
        evs.is_empty(),
        "a signed event for an order that is not completed must not settle"
    );

    let failed =
        json!({"event": "ORDER_FAILED", "order_id": "ord_3", "merchant_order_ext_ref": null})
            .to_string();
    let evs = p
        .handle_webhook(&revolut_sig("wsk_x", &failed), failed.as_bytes())
        .await
        .unwrap();
    assert!(matches!(&evs[0], ProviderEvent::Failed { lookup, .. } if lookup == "ord_3"));
}

#[tokio::test]
async fn revolut_setup_registers_a_webhook_once_and_returns_the_signing_secret() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/1.0/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    Mock::given(method("POST")).and(path("/api/1.0/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "wh_1", "url": "https://gw/hook/1/2", "events": ["ORDER_COMPLETED"], "signing_secret": "wsk_new"})))
        .expect(1).mount(&server).await;
    let patch = revolut_for(&server.uri(), None)
        .setup("https://gw/hook/1/2")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(patch["webhook_secret"], "wsk_new");

    let server2 = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/1.0/webhooks"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!([{"id": "wh_1", "url": "https://gw/hook/1/2", "events": ["ORDER_COMPLETED"]}]),
        ))
        .mount(&server2)
        .await;
    assert!(
        revolut_for(&server2.uri(), Some("wsk_new"))
            .setup("https://gw/hook/1/2")
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn revolut_refund_posts_the_amount() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/orders/ord_1/refund"))
        .and(body_string_contains("\"amount\":250"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "ref_1"})))
        .expect(1)
        .mount(&server)
        .await;
    let out = revolut_for(&server.uri(), None)
        .refund(
            &attempt_row("ord_1"),
            RefundRequest {
                amount: 250,
                currency: "EUR".into(),
                destination: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(out.external_id.as_deref(), Some("ref_1"));

    let bad = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(422).set_body_json(json!({"message": "too much"})))
        .mount(&bad)
        .await;
    assert!(
        revolut_for(&bad.uri(), None)
            .refund(
                &attempt_row("ord_1"),
                RefundRequest {
                    amount: 250,
                    currency: "EUR".into(),
                    destination: None
                }
            )
            .await
            .is_err()
    );
}
