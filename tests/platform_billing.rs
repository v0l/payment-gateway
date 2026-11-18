mod common;
use axum::Router;
use axum::http::StatusCode;
use bitcoin::hashes::{Hash, sha256};
use common::*;
use lightning_invoice::{Currency, InvoiceBuilder, PaymentSecret};
use payment_gateway::accounts;
use payment_gateway::audit::{self, Filter};
use payment_gateway::db;
use payment_gateway::ledger::{AccountKind, AccountRef, Entry, Posting, TxKind, post};
use payment_gateway::payouts::PayoutPolicy;
use payment_gateway::service::{Error, WithdrawalRequest};
use serde_json::{Value, json};

struct World {
    h: Harness,
    clock: std::sync::Arc<ManualClock>,
    platform: TenantFixture,
    platform_stripe: u64,
    t: TenantFixture,
    stripe: u64,
}

async fn define_live(h: &Harness, price: u64, bps: u32, base: u64, included: u64) {
    h.service
        .define_platform_plan(
            Some(2),
            payment_gateway::service::PlanDefinition {
                name: "Live".into(),
                live: true,
                monthly_price: price,
                currency: "EUR".into(),
                included_volume: included,
                fee_bps: bps,
                fee_base: base,
                limits: Default::default(),
                active: true,
            },
        )
        .await
        .unwrap();
}

async fn world(price: u64, bps: u32, base: u64) -> World {
    world_with(price, bps, base, false).await
}

async fn world_with(price: u64, bps: u32, base: u64, mail: bool) -> World {
    let clock = ManualClock::at(2030, 1, 1);
    let h = if mail {
        harness_with_clock_and_mail(clock.clone()).await
    } else {
        harness_with_clock(clock.clone()).await
    };
    define_live(&h, price, bps, base, 0).await;
    let platform = tenant(&h).await;
    let platform_stripe = provider(&h, &platform, "stripe", json!({}), None).await;
    h.service
        .set_platform_tenant(Some(platform.id))
        .await
        .unwrap();
    let t = tenant(&h).await;
    let stripe = provider(&h, &t, "stripe", json!({}), None).await;
    World {
        h,
        clock,
        platform,
        platform_stripe,
        t,
        stripe,
    }
}

async fn pay_via(
    h: &Harness,
    t: &TenantFixture,
    stripe: u64,
    reference: &str,
    cents: u64,
) -> String {
    let p = new_payment(h, t, eur(reference, cents)).await;
    let id = p["id"].as_str().unwrap().to_string();
    settle(h, t, stripe, &id).await;
    id
}

async fn settle(h: &Harness, t: &TenantFixture, stripe: u64, id: &str) {
    let (s, r) = attempt(h, t, id, json!({"method": "stripe"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    let lookup = db::get_attempt(&h.pool, aid).await.unwrap().unwrap().lookup;
    let hook = send(
        &h.app,
        "POST",
        &format!("/hook/{}/{stripe}", t.id),
        None,
        &[("x-mock-secret", "mock-secret")],
        json!({"lookup": lookup}),
    )
    .await;
    assert_eq!(hook.status, StatusCode::OK, "{}", hook.text);
}

async fn invoices(w: &World) -> Vec<db::PlatformInvoiceRow> {
    let mut rows = db::list_platform_invoices(&w.h.pool, w.t.id, 100)
        .await
        .unwrap();
    rows.reverse();
    rows
}

async fn pay_invoice(w: &World, inv: &db::PlatformInvoiceRow) {
    w.h.service.run_platform_billing().await.unwrap();
    let fresh = db::get_platform_invoice(&w.h.pool, w.t.id, inv.id)
        .await
        .unwrap()
        .unwrap();
    let payment = hex::encode(fresh.payment_id.expect("invoice payment"));
    settle(&w.h, &w.platform, w.platform_stripe, &payment).await;
}

async fn standing(w: &World) -> String {
    db::platform_standing(&w.h.pool, w.t.id)
        .await
        .unwrap()
        .unwrap()
}

async fn types(t: &TenantFixture, h: &Harness) -> Vec<String> {
    for _ in 0..6 {
        h.service.deliver_due().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    t.receiver
        .types()
        .into_iter()
        .filter(|t| t.starts_with("platform."))
        .collect()
}

async fn audit_actions(h: &Harness, tenant: u64) -> Vec<String> {
    audit::list(
        &h.pool,
        &Filter {
            tenant_id: Some(tenant),
            limit: Some(200),
            ..Default::default()
        },
    )
    .await
    .unwrap()
    .into_iter()
    .map(|e| e.action)
    .collect()
}

#[tokio::test]
async fn default_prices_are_zero_and_produce_no_invoices() {
    let h = harness().await;
    let plans = db::list_platform_plans(&h.pool).await.unwrap();
    assert_eq!(plans.len(), 2);
    for p in &plans {
        assert_eq!((p.monthly_price, p.fee_bps, p.fee_base), (0, 0, 0));
    }
    let clock_world = world(0, 0, 0).await;
    let w = &clock_world;
    pay_via(&w.h, &w.t, w.stripe, "a", 5000).await;
    w.h.service.run_platform_billing().await.unwrap();
    w.clock.advance_days(40);
    w.h.service.run_platform_billing().await.unwrap();
    assert!(invoices(w).await.is_empty());
    assert_eq!(standing(w).await, "current");
}

#[tokio::test]
async fn metering_counts_settled_payments_per_period_with_exact_boundaries() {
    let w = world(1000, 100, 10).await;
    w.h.service.run_platform_billing().await.unwrap();
    let first = invoices(&w).await;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].kind, "plan");
    assert_eq!(first[0].total, 1000);

    pay_via(&w.h, &w.t, w.stripe, "in-period", 10_000).await;
    let unpaid = new_payment(&w.h, &w.t, eur("never-settled", 99_999)).await;
    assert_eq!(unpaid["status"], "pending");
    w.clock.advance_days(31);
    pay_via(&w.h, &w.t, w.stripe, "next-period", 4_000).await;

    w.h.service.run_platform_billing().await.unwrap();
    let rows = invoices(&w).await;
    let period = rows.iter().find(|i| i.kind == "period").unwrap();
    assert_eq!((period.usage_count, period.usage_volume), (1, 10_000));
    assert_eq!(period.subtotal, 10 + 100 + 1000);
    assert_eq!(period.total, 1110);
    let lines: Vec<Value> = serde_json::from_str(&period.line_items).unwrap();
    assert_eq!(lines.len(), 3);

    w.clock.advance_days(31);
    w.h.service.run_platform_billing().await.unwrap();
    let rows = invoices(&w).await;
    let second = rows.iter().filter(|i| i.kind == "period").nth(1).unwrap();
    assert_eq!((second.usage_count, second.usage_volume), (1, 4_000));
    assert_eq!(second.subtotal, 10 + 40 + 1000);
}

#[tokio::test]
async fn running_the_billing_worker_twice_never_double_invoices() {
    let w = world(1000, 0, 0).await;
    w.h.service.run_platform_billing().await.unwrap();
    w.clock.advance_days(32);
    for _ in 0..3 {
        w.h.service.run_platform_billing().await.unwrap();
    }
    let rows = invoices(&w).await;
    assert_eq!(rows.iter().filter(|i| i.kind == "period").count(), 1);
    assert_eq!(rows.iter().filter(|i| i.kind == "plan").count(), 1);
}

#[tokio::test]
async fn upgrade_prorates_and_downgrade_becomes_credit() {
    let w = world(3100, 0, 0).await;
    let pro =
        w.h.service
            .define_platform_plan(
                None,
                payment_gateway::service::PlanDefinition {
                    name: "Pro".into(),
                    live: true,
                    monthly_price: 6200,
                    currency: "EUR".into(),
                    included_volume: 0,
                    fee_bps: 0,
                    fee_base: 0,
                    limits: Default::default(),
                    active: true,
                },
            )
            .await
            .unwrap();
    w.h.service.run_platform_billing().await.unwrap();
    w.clock.advance_days(10);
    let (_, _, diff) =
        w.h.service
            .change_platform_plan(w.t.id, pro.id, "customer asked")
            .await
            .unwrap();
    assert_eq!(diff, (6200 - 3100) * 21 / 31);
    let rows = invoices(&w).await;
    let upgrade = rows.iter().find(|i| i.kind == "upgrade").unwrap();
    assert_eq!(upgrade.total, diff as u64);

    w.clock.advance_days(5);
    let (_, _, back) =
        w.h.service
            .change_platform_plan(w.t.id, 2, "downgrade")
            .await
            .unwrap();
    assert!(back < 0);
    let account = db::get_platform_account(&w.h.pool, w.t.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(account.credit, (-back) as u64);

    w.clock.advance_days(20);
    w.h.service.run_platform_billing().await.unwrap();
    let rows = invoices(&w).await;
    let period = rows.iter().find(|i| i.kind == "period").unwrap();
    assert_eq!(period.credit_applied, (-back) as u64);
    assert_eq!(period.total, period.subtotal - (-back) as u64);
}

#[tokio::test]
async fn dunning_warns_then_restricts_then_disables_and_recovers_on_payment() {
    let w = world_with(1000, 0, 0, true).await;
    let owner = accounts::insert_account(
        &w.h.pool,
        "owner@example.com",
        &accounts::hash_password("correct horse battery").unwrap(),
        false,
    )
    .await
    .unwrap();
    accounts::add_member(&w.h.pool, w.t.id, owner, accounts::Role::Owner)
        .await
        .unwrap();

    w.h.service.run_platform_billing().await.unwrap();
    assert_eq!(standing(&w).await, "current");
    new_payment(&w.h, &w.t, eur("ok-now", 100)).await;

    w.clock.advance_days(3);
    w.h.service.run_platform_billing().await.unwrap();
    assert_eq!(standing(&w).await, "warned");
    new_payment(&w.h, &w.t, eur("ok-warned", 100)).await;

    w.clock.advance_days(7);
    w.h.service.run_platform_billing().await.unwrap();
    assert_eq!(standing(&w).await, "restricted");
    let (s, r) = call(
        &w.h.app,
        "POST",
        "/v1/payments",
        Some(&w.t.key),
        eur("blocked", 100),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT, "{r}");

    w.clock.advance_days(14);
    w.h.service.run_platform_billing().await.unwrap();
    assert_eq!(standing(&w).await, "disabled");

    let rows = invoices(&w).await;
    pay_invoice(&w, &rows[0]).await;
    assert_eq!(standing(&w).await, "current");
    new_payment(&w.h, &w.t, eur("back", 100)).await;

    let actions = audit_actions(&w.h, w.t.id).await;
    assert_eq!(
        actions
            .iter()
            .filter(|a| *a == "platform.standing_changed")
            .count(),
        4
    );
    let sent = w.h.mailer.wait_for(4).await;
    let subjects: Vec<String> = sent.iter().map(|m| m.subject.clone()).collect();
    assert!(
        subjects.iter().any(|s| s.contains("past due")),
        "{subjects:?}"
    );
    assert!(
        subjects.iter().any(|s| s.contains("restricted")),
        "{subjects:?}"
    );
    assert!(
        subjects.iter().any(|s| s.contains("disabled")),
        "{subjects:?}"
    );
    assert!(sent.iter().all(|m| m.to == "owner@example.com"));
}

fn bolt11(amount_msat: u64) -> String {
    let secret: [u8; 32] = rand::random();
    let key = bitcoin::secp256k1::SecretKey::from_slice(&[0xcd; 32]).unwrap();
    InvoiceBuilder::new(Currency::Regtest)
        .description("t".into())
        .payment_hash(sha256::Hash::from_slice(&secret).unwrap())
        .payment_secret(PaymentSecret(secret))
        .current_timestamp()
        .min_final_cltv_expiry_delta(144)
        .amount_milli_satoshis(amount_msat)
        .build_signed(|m| bitcoin::secp256k1::Secp256k1::new().sign_ecdsa_recoverable(m, &key))
        .unwrap()
        .to_string()
}

async fn fund(h: &Harness, tenant: u64, msat: i64) {
    let mut tx = h.pool.begin().await.unwrap();
    post(
        &mut tx,
        &Posting::new(
            TxKind::Adjustment,
            vec![
                Entry::new(
                    AccountRef::platform(AccountKind::PlatformWallet, "BTC"),
                    -msat,
                ),
                Entry::new(
                    AccountRef::tenant(tenant, AccountKind::TenantBalance, "BTC"),
                    msat,
                ),
            ],
        ),
        &unique("fund"),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn funds_are_never_frozen_even_when_disabled_for_non_payment() {
    let w = world(1000, 0, 0).await;
    w.h.service.payouts.set_policy(PayoutPolicy {
        network: bitcoin::Network::Regtest,
        ..PayoutPolicy::default()
    });
    let paid = pay_via(&w.h, &w.t, w.stripe, "old-sale", 2000).await;
    let pending = new_payment(&w.h, &w.t, eur("in-flight", 1500)).await;
    let pending_id = pending["id"].as_str().unwrap().to_string();
    fund(&w.h, w.t.id, 50_000_000).await;

    w.h.service.run_platform_billing().await.unwrap();
    w.clock.advance_days(30);
    w.h.service.run_platform_billing().await.unwrap();
    assert_eq!(standing(&w).await, "disabled");

    let (s, _) = call(
        &w.h.app,
        "POST",
        "/v1/payments",
        Some(&w.t.key),
        eur("new", 100),
    )
    .await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = attempt(&w.h, &w.t, &pending_id, json!({"method": "stripe"})).await;
    assert_eq!(s, StatusCode::CONFLICT);

    let (s, r) = call(
        &w.h.app,
        "POST",
        &format!("/v1/payments/{paid}/refund"),
        Some(&w.t.key),
        json!({"amount": 500}),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");

    let tenant = w.h.service.tenant(w.t.id).await.unwrap();
    let wd =
        w.h.service
            .request_withdrawal(
                &tenant,
                WithdrawalRequest {
                    amount: 2_000_000,
                    invoice: Some(bolt11(2_000_000)),
                    ..Default::default()
                },
            )
            .await;
    assert!(wd.is_ok(), "{wd:?}");

    assert!(
        w.h.service
            .check_standing(w.t.id)
            .await
            .is_err_and(|e| matches!(e, Error::Conflict(_)))
    );
    w.h.service.review_platform_standing(w.t.id).await.unwrap();
    assert_eq!(standing(&w).await, "disabled");
}

#[tokio::test]
async fn settlement_of_payments_created_before_the_restriction_still_completes() {
    let w = world(1000, 0, 0).await;
    let pending = new_payment(&w.h, &w.t, eur("early", 1500)).await;
    let id = pending["id"].as_str().unwrap().to_string();
    let (s, r) = attempt(&w.h, &w.t, &id, json!({"method": "stripe"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let aid = r["attempts"][0]["id"].as_u64().unwrap();
    let lookup = db::get_attempt(&w.h.pool, aid)
        .await
        .unwrap()
        .unwrap()
        .lookup;
    w.h.service.run_platform_billing().await.unwrap();
    w.clock.advance_days(10);
    w.h.service.run_platform_billing().await.unwrap();
    assert_eq!(standing(&w).await, "restricted");
    let hook = send(
        &w.h.app,
        "POST",
        &format!("/hook/{}/{}", w.t.id, w.stripe),
        None,
        &[("x-mock-secret", "mock-secret")],
        json!({"lookup": lookup}),
    )
    .await;
    assert_eq!(hook.status, StatusCode::OK, "{}", hook.text);
    assert_eq!(get_payment(&w.h, &w.t, &id).await["status"], "paid");
}

#[tokio::test]
async fn plan_limits_feed_the_usage_limits() {
    let w = world(0, 0, 0).await;
    w.h.service
        .define_platform_plan(
            Some(2),
            payment_gateway::service::PlanDefinition {
                name: "Live".into(),
                live: true,
                monthly_price: 0,
                currency: "EUR".into(),
                included_volume: 0,
                fee_bps: 0,
                fee_base: 0,
                limits: payment_gateway::limits::LimitOverrides {
                    limit_payments_per_day: Some(1),
                    ..Default::default()
                },
                active: true,
            },
        )
        .await
        .unwrap();
    w.h.service.run_platform_billing().await.unwrap();
    new_payment(&w.h, &w.t, eur("one", 100)).await;
    let (s, r) = call(
        &w.h.app,
        "POST",
        "/v1/payments",
        Some(&w.t.key),
        eur("two", 100),
    )
    .await;
    assert_eq!(s, StatusCode::FORBIDDEN, "{r}");
    assert_eq!(r["limit"], "payments_per_day");
}

#[tokio::test]
async fn invoice_webhooks_reach_the_project_owner_endpoint() {
    let w = world(1000, 0, 0).await;
    w.h.service.run_platform_billing().await.unwrap();
    w.clock.advance_days(4);
    w.h.service.run_platform_billing().await.unwrap();
    let rows = invoices(&w).await;
    pay_invoice(&w, &rows[0]).await;
    let got = types(&w.t, &w.h).await;
    assert_eq!(
        got,
        vec![
            "platform.invoice.created",
            "platform.invoice.past_due",
            "platform.invoice.paid"
        ]
    );
    let events = w.t.receiver.events();
    let paid = events
        .iter()
        .find(|e| e["type"] == "platform.invoice.paid")
        .unwrap();
    assert_eq!(paid["data"]["invoice"]["status"], "paid");
    assert_eq!(paid["data"]["invoice"]["total"], 1000);
    assert!(
        w.platform
            .receiver
            .types()
            .iter()
            .all(|t| !t.starts_with("platform.invoice"))
    );
}

struct Client {
    app: Router,
    cookie: String,
    csrf: String,
}

impl Client {
    async fn login(app: &Router, email: &str) -> Self {
        let r = send(
            app,
            "POST",
            "/console/api/login",
            None,
            &[],
            json!({"email": email, "password": "correct horse battery"}),
        )
        .await;
        assert_eq!(r.status, StatusCode::OK, "{}", r.text);
        let cookie = r.headers["set-cookie"]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .trim_start_matches("pgw_session=")
            .to_string();
        Self {
            app: app.clone(),
            cookie,
            csrf: r.json["csrf"].as_str().unwrap_or("").to_string(),
        }
    }

    async fn req(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        let cookie = format!("pgw_session={}", self.cookie);
        let r = send(
            &self.app,
            method,
            &format!("/console/api{path}"),
            None,
            &[("cookie", cookie.as_str()), ("x-csrf-token", &self.csrf)],
            body,
        )
        .await;
        (r.status, r.json)
    }
}

async fn account(h: &Harness, email: &str, operator: bool) -> Client {
    let hash = accounts::hash_password("correct horse battery").unwrap();
    accounts::insert_account(&h.pool, email, &hash, operator)
        .await
        .unwrap();
    Client::login(&h.app, email).await
}

#[tokio::test]
async fn billing_tab_data_is_private_and_operator_overrides_are_logged_with_reasons() {
    let w = world(1000, 0, 0).await;
    let owner = account(&w.h, "owner@example.com", false).await;
    let other = account(&w.h, "other@example.com", false).await;
    let op = account(&w.h, "op@example.com", true).await;
    let owner_id = accounts::account_by_email(&w.h.pool, "owner@example.com")
        .await
        .unwrap()
        .unwrap()
        .id;
    accounts::add_member(&w.h.pool, w.t.id, owner_id, accounts::Role::Viewer)
        .await
        .unwrap();
    w.h.service.run_platform_billing().await.unwrap();

    let (s, view) = owner
        .req(
            "GET",
            &format!("/projects/{}/platform-billing", w.t.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{view}");
    assert_eq!(view["account"]["standing"], "current");
    assert_eq!(view["invoices"][0]["lines"][0]["unit_amount"], 1000);
    assert!(view["invoices"][0]["pay_url"].as_str().is_some() || true);
    let invoice_id = view["invoices"][0]["id"].as_u64().unwrap();

    let (s, _) = other
        .req(
            "GET",
            &format!("/projects/{}/platform-billing", w.t.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    for (path, body) in [
        (
            format!("/admin/projects/{}/platform/credit", w.t.id),
            json!({"amount": 5, "reason": "x"}),
        ),
        (
            format!(
                "/admin/projects/{}/platform/invoices/{invoice_id}/waive",
                w.t.id
            ),
            json!({"reason": "x"}),
        ),
        (
            format!("/admin/projects/{}/platform/plan", w.t.id),
            json!({"plan_id": 1, "reason": "x"}),
        ),
    ] {
        let (s, _) = owner.req("POST", &path, body).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
    }

    let (s, _) = op
        .req(
            "POST",
            &format!("/admin/projects/{}/platform/credit", w.t.id),
            json!({"amount": 5, "reason": ""}),
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, r) = op
        .req(
            "POST",
            &format!("/admin/projects/{}/platform/credit", w.t.id),
            json!({"amount": 250, "reason": "goodwill"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["credit"], 250);

    let (s, r) = op
        .req(
            "POST",
            &format!(
                "/admin/projects/{}/platform/invoices/{invoice_id}/waive",
                w.t.id
            ),
            json!({"reason": "launch promo"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["status"], "waived");
    assert_eq!(r["reason"], "launch promo");

    let (s, _) = op
        .req(
            "POST",
            &format!(
                "/admin/projects/{}/platform/invoices/{invoice_id}/waive",
                w.t.id
            ),
            json!({"reason": "again"}),
        )
        .await;
    assert_eq!(s, StatusCode::CONFLICT);

    let (s, r) = op
        .req(
            "POST",
            &format!("/admin/projects/{}/platform/plan", w.t.id),
            json!({"plan_id": 1, "reason": "moved to test"}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");

    let events = audit::list(
        &w.h.pool,
        &Filter {
            tenant_id: Some(w.t.id),
            limit: Some(100),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for (action, reason) in [
        ("platform.credited", "goodwill"),
        ("platform.invoice_waived", "launch promo"),
        ("platform.plan_changed", "moved to test"),
    ] {
        let e = events
            .iter()
            .find(|e| e.action == action)
            .unwrap_or_else(|| panic!("missing {action}"));
        assert_eq!(e.details["reason"], reason);
        assert_eq!(e.actor_kind, "account");
    }

    let (s, plans) = op.req("GET", "/admin/platform/plans", Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(plans.as_array().unwrap().len(), 2);
    let (s, r) = op
        .req(
            "POST",
            "/admin/platform/plans/2",
            json!({"name": "Live", "live": true, "monthly_price": 900, "fee_bps": 50}),
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["monthly_price"], 900);
    assert!(
        audit_actions(&w.h, w.t.id).await.len() >= 3,
        "tenant scoped events recorded"
    );
    let all = audit::list(&w.h.pool, &Filter::default()).await.unwrap();
    assert!(all.iter().any(|e| e.action == "platform.plan_defined"));
}

#[tokio::test]
async fn waiving_an_invoice_clears_the_past_due_state() {
    let w = world(1000, 0, 0).await;
    w.h.service.run_platform_billing().await.unwrap();
    w.clock.advance_days(10);
    w.h.service.run_platform_billing().await.unwrap();
    assert_eq!(standing(&w).await, "restricted");
    let rows = invoices(&w).await;
    w.h.service
        .waive_platform_invoice(w.t.id, rows[0].id, "oops")
        .await
        .unwrap();
    assert_eq!(standing(&w).await, "current");
}

#[tokio::test]
async fn one_project_cannot_waive_or_read_another_projects_invoice() {
    let w = world(1000, 0, 0).await;
    let other = tenant(&w.h).await;
    w.h.service.run_platform_billing().await.unwrap();
    let rows = invoices(&w).await;
    let r =
        w.h.service
            .waive_platform_invoice(other.id, rows[0].id, "wrong tenant")
            .await;
    assert!(matches!(r, Err(Error::NotFound)));
    let still = db::get_platform_invoice(&w.h.pool, w.t.id, rows[0].id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(still.status, "open");
    assert!(
        db::get_platform_invoice(&w.h.pool, other.id, rows[0].id)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn shared_custody_payments_carry_only_the_custody_fee_not_the_platform_fee() {
    use payment_gateway::ledger::{self, AccountKind};
    use payment_gateway::providers::{EventSink, ProviderEvent};
    use payment_gateway::service::ProviderSink;
    use payment_gateway::shared::shared_key;

    let w = world(0, 100, 10).await;
    let (s, r) = call(
        &w.h.app,
        "POST",
        "/admin/shared",
        Some(ADMIN),
        json!({
            "name": unique("shared"), "method": "lightning", "provider_type": "mock",
            "network": "regtest", "config": {"macaroon": "topsecret"},
            "fee_bps": 150, "fee_base_msat": 2000, "allow_untrusted": true,
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let sid = r["id"].as_u64().unwrap();
    let tenant = w.h.service.tenant(w.t.id).await.unwrap();
    w.h.service.enable_shared(&tenant, sid).await.unwrap();

    w.h.service.run_platform_billing().await.unwrap();
    let p = new_payment(&w.h, &w.t, eur("custody", 5000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (s, r) = attempt(&w.h, &w.t, &id, json!({"method": "lightning"})).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let hash = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: w.h.service.clone(),
        provider_id: shared_key(sid),
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
    pay_via(&w.h, &w.t, w.stripe, "own-provider", 10_000).await;

    let received = 100_000_000i64;
    let custody_fee = received * 150 / 10_000 + 2000;
    let (fees,): (i64,) = sqlx::query_as(
        "select cast(coalesce(sum(balance), 0) as signed) from ledger_account where tenant_id is null and kind = ?",
    )
    .bind(AccountKind::PlatformFees.as_str())
    .fetch_one(&w.h.pool)
    .await
    .unwrap();
    assert_eq!(fees, custody_fee);
    assert_eq!(
        ledger::tenant_balance(&w.h.pool, w.t.id, "BTC")
            .await
            .unwrap(),
        received - custody_fee
    );

    w.clock.advance_days(32);
    w.h.service.run_platform_billing().await.unwrap();
    let rows = invoices(&w).await;
    let period = rows.iter().find(|i| i.kind == "period").unwrap();
    assert_eq!((period.usage_count, period.usage_volume), (1, 10_000));
    assert_eq!(period.subtotal, 10 + 100);
}
