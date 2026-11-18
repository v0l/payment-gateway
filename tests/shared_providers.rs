mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::ledger::{self, AccountKind, AccountRef};
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::ProviderSink;
use payment_gateway::shared::shared_key;
use serde_json::{Value, json};

async fn shared(h: &Harness, method: &str, network: &str, bps: u32, base: u64) -> u64 {
    let (s, r) = call(
        &h.app,
        "POST",
        "/admin/shared",
        Some(ADMIN),
        json!({
            "name": unique("shared"), "method": method, "provider_type": "mock",
            "network": network, "config": {"macaroon": "topsecret"},
            "fee_bps": bps, "fee_base_msat": base, "allow_untrusted": network != "mainnet",
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    r["id"].as_u64().unwrap()
}

async fn enable(h: &Harness, t: &TenantFixture, sid: u64) {
    let tenant = payment_gateway::db::get_tenant(&h.pool, t.id)
        .await
        .unwrap()
        .unwrap();
    h.service.enable_shared(&tenant, sid).await.unwrap();
}

async fn pay(h: &Harness, t: &TenantFixture, sid: u64, cents: u64) -> (String, String, u64) {
    let p = new_payment(h, t, eur("o", cents)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let a = &r["attempts"][0];
    assert_eq!(a["platform_managed"], true);
    let _ = sid;
    (
        id,
        a["details"]["payment_hash"].as_str().unwrap().to_string(),
        a["id"].as_u64().unwrap(),
    )
}

fn paid(lookup: &str, amount: Option<u64>) -> ProviderEvent {
    ProviderEvent::Paid {
        lookup: lookup.into(),
        reference: lookup.into(),
        amount,
        confirmed: true,
        saved: None,
    }
}

fn sink(h: &Harness, sid: u64) -> ProviderSink {
    ProviderSink {
        service: h.service.clone(),
        provider_id: shared_key(sid),
    }
}

async fn balance(h: &Harness, t: &TenantFixture) -> i64 {
    ledger::tenant_balance(&h.pool, t.id, "BTC").await.unwrap()
}

async fn platform(h: &Harness, kind: AccountKind) -> i64 {
    let (v,): (i64,) = sqlx::query_as(
        "select cast(coalesce(sum(balance), 0) as signed) from ledger_account where tenant_id is null and kind = ?",
    )
    .bind(kind.as_str())
    .fetch_one(&h.pool)
    .await
    .unwrap();
    v
}

#[tokio::test]
async fn settlement_credits_net_and_fee_exactly_and_shows_them() {
    let h = harness().await;
    let t = tenant(&h).await;
    let sid = shared(&h, "lightning", "regtest", 150, 2000).await;
    enable(&h, &t, sid).await;
    let (id, hash, _) = pay(&h, &t, sid, 5000).await;
    sink(&h, sid).event(paid(&hash, None), None).await.unwrap();

    let received = 100_000_000i64;
    let fee = received * 150 / 10_000 + 2000;
    assert_eq!(balance(&h, &t).await, received - fee);
    assert_eq!(platform(&h, AccountKind::PlatformFees).await, fee);
    assert_eq!(platform(&h, AccountKind::PlatformWallet).await, -received);
    let got = get_payment(&h, &t, &id).await;
    assert_eq!(got["attempts"][0]["platform_fee"], fee);
    assert_eq!(got["attempts"][0]["net_credited"], received - fee);
    h.service.deliver_due().await.unwrap();
    let ev = &t.receiver.events()[0];
    assert_eq!(ev["type"], "payment.paid");
    assert_eq!(ev["data"]["detail"]["platform_fee"], fee);
    assert_eq!(ev["data"]["detail"]["net_credited"], received - fee);
    let rec = ledger::reconcile(&h.pool).await.unwrap();
    assert!(rec.is_empty(), "{rec:?}");
}

#[tokio::test]
async fn replayed_and_concurrent_settlement_credit_once() {
    let h = harness().await;
    let t = tenant(&h).await;
    let sid = shared(&h, "lightning", "regtest", 100, 0).await;
    enable(&h, &t, sid).await;
    let (_, hash, _) = pay(&h, &t, sid, 5000).await;
    let mut jobs = vec![];
    for _ in 0..6 {
        let s = sink(&h, sid);
        let hash = hash.clone();
        jobs.push(tokio::spawn(async move {
            s.event(paid(&hash, None), None).await.unwrap();
        }));
    }
    for j in jobs {
        j.await.unwrap();
    }
    sink(&h, sid).event(paid(&hash, None), None).await.unwrap();
    assert_eq!(balance(&h, &t).await, 99_000_000);
    assert_eq!(platform(&h, AccountKind::PlatformFees).await, 1_000_000);
}

#[tokio::test]
async fn platform_fee_rounds_up_and_tenant_never_gets_more_than_received() {
    let h = harness().await;
    let t = tenant(&h).await;
    let sid = shared(&h, "lightning", "regtest", 1, 0).await;
    enable(&h, &t, sid).await;
    let (_, hash, _) = pay(&h, &t, sid, 5001).await;
    sink(&h, sid).event(paid(&hash, None), None).await.unwrap();
    let received = 100_020_000i64;
    let fee = (received + 9_999) / 10_000;
    assert_eq!(platform(&h, AccountKind::PlatformFees).await, fee);
    assert_eq!(balance(&h, &t).await + fee, received);
}

#[tokio::test]
async fn refund_debits_the_balance_and_pays_from_the_platform_wallet() {
    let h = harness().await;
    let t = tenant(&h).await;
    let sid = shared(&h, "lightning", "regtest", 100, 0).await;
    enable(&h, &t, sid).await;
    let (id, hash, _) = pay(&h, &t, sid, 5000).await;
    sink(&h, sid).event(paid(&hash, None), None).await.unwrap();
    let before = balance(&h, &t).await;
    let wallet_before = platform(&h, AccountKind::PlatformWallet).await;
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{id}/refund"),
        Some(&t.key),
        json!({"destination": "lnbc1x", "amount": 2500}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(balance(&h, &t).await, before - 50_000_000);
    assert_eq!(
        platform(&h, AccountKind::PlatformWallet).await,
        wallet_before + 50_000_000
    );
    let sent = mock(&h, shared_key(sid)).refunds.lock().unwrap().clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].amount, 50_000_000);
}

#[tokio::test]
async fn refund_is_refused_when_the_balance_is_too_low_unless_negative_is_allowed() {
    let h = harness().await;
    let t = tenant(&h).await;
    let sid = shared(&h, "lightning", "regtest", 0, 0).await;
    enable(&h, &t, sid).await;
    let (id, hash, _) = pay(&h, &t, sid, 5000).await;
    sink(&h, sid).event(paid(&hash, None), None).await.unwrap();
    let mut tx = h.pool.begin().await.unwrap();
    let drained = AccountRef::tenant(t.id, AccountKind::TenantBalance, "BTC");
    let wallet = AccountRef::platform(AccountKind::PlatformWallet, "BTC");
    ledger::post(
        &mut tx,
        &ledger::Posting::new(
            ledger::TxKind::Adjustment,
            vec![
                ledger::Entry::new(drained, -100_000_000),
                ledger::Entry::new(wallet, 100_000_000),
            ],
        ),
        "drain",
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    let uri = format!("/v1/payments/{id}/refund");
    let (s, _) = call(
        &h.app,
        "POST",
        &uri,
        Some(&t.key),
        json!({"destination": "lnbc1x"}),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    assert!(mock(&h, shared_key(sid)).refunds.lock().unwrap().is_empty());
    assert_eq!(get_payment(&h, &t, &id).await["status"], "paid");

    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/admin/tenants/{}/allow-negative", t.id),
        Some(ADMIN),
        json!({"allow": true}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, r) = call(
        &h.app,
        "POST",
        &uri,
        Some(&t.key),
        json!({"destination": "lnbc1x"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(balance(&h, &t).await, -100_000_000);
}

#[tokio::test]
async fn failed_shared_refund_restores_the_balance() {
    let h = harness().await;
    let t = tenant(&h).await;
    let sid = shared(&h, "lightning", "regtest", 0, 0).await;
    enable(&h, &t, sid).await;
    let (id, hash, _) = pay(&h, &t, sid, 5000).await;
    sink(&h, sid).event(paid(&hash, None), None).await.unwrap();
    let before = balance(&h, &t).await;
    *mock(&h, shared_key(sid)).fail_refund.lock().unwrap() = true;
    let uri = format!("/v1/payments/{id}/refund");
    let (_, r) = call(
        &h.app,
        "POST",
        &uri,
        Some(&t.key),
        json!({"destination": "x"}),
    )
    .await;
    assert_eq!(r["status"], "failed");
    assert_eq!(balance(&h, &t).await, before);
    *mock(&h, shared_key(sid)).fail_refund.lock().unwrap() = false;
    let (s, _) = call(
        &h.app,
        "POST",
        &uri,
        Some(&t.key),
        json!({"destination": "x"}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(balance(&h, &t).await, 0);
}

#[tokio::test]
async fn per_project_fee_override_applies_to_new_attempts() {
    let h = harness().await;
    let t = tenant(&h).await;
    let sid = shared(&h, "lightning", "regtest", 500, 0).await;
    enable(&h, &t, sid).await;
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/admin/shared/{sid}/tenants/{}/fee", t.id),
        Some(ADMIN),
        json!({"fee_bps": 50}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (_, hash, _) = pay(&h, &t, sid, 5000).await;
    sink(&h, sid).event(paid(&hash, None), None).await.unwrap();
    assert_eq!(platform(&h, AccountKind::PlatformFees).await, 500_000);
}

#[tokio::test]
async fn secrets_are_not_echoed_and_tenants_are_isolated() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let sid = shared(&h, "lightning", "regtest", 100, 0).await;
    let (s, r) = call(&h.app, "GET", "/admin/shared", Some(ADMIN), Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    assert!(!r.to_string().contains("topsecret"), "{r}");
    enable(&h, &a, sid).await;
    let (_, hash, _) = pay(&h, &a, sid, 5000).await;
    sink(&h, sid).event(paid(&hash, None), None).await.unwrap();
    assert_eq!(balance(&h, &b).await, 0);
    let (_, methods) = call(&h.app, "GET", "/v1/methods", Some(&b.key), Value::Null).await;
    assert!(methods.as_array().unwrap().is_empty(), "{methods}");
    let (_, methods) = call(&h.app, "GET", "/v1/methods", Some(&a.key), Value::Null).await;
    assert_eq!(methods[0]["platform_managed"], true);
    let p = new_payment(&h, &b, eur("o", 1000)).await;
    let (s, _) = attempt(
        &h,
        &b,
        p["id"].as_str().unwrap(),
        json!({"method": "lightning"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn disabled_or_disallowed_shared_providers_cannot_take_payments() {
    let h = harness().await;
    let t = tenant(&h).await;
    let sid = shared(&h, "lightning", "regtest", 100, 0).await;
    enable(&h, &t, sid).await;
    let (s, _) = call(
        &h.app,
        "POST",
        &format!("/admin/shared/{sid}"),
        Some(ADMIN),
        json!({"enabled": false}),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let p = new_payment(&h, &t, eur("o", 1000)).await;
    let (s, _) = attempt(
        &h,
        &t,
        p["id"].as_str().unwrap(),
        json!({"method": "lightning"}),
    )
    .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
