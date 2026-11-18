mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::accounts;
use payment_gateway::ledger::{self, AccountKind};
use payment_gateway::providers::{EventSink, ProviderEvent, WalletBalance};
use payment_gateway::service::ProviderSink;
use payment_gateway::shared::shared_key;
use payment_gateway::treasury::{self, StuckKind};
use serde_json::{Value, json};

const RECEIVED: i64 = 100_000_000;

struct Setup {
    h: Harness,
    clock: std::sync::Arc<ManualClock>,
    t: TenantFixture,
    sid: u64,
}

async fn setup(mail: bool) -> Setup {
    let clock = ManualClock::at(2030, 1, 1);
    let h = if mail {
        harness_with_clock_and_mail(clock.clone()).await
    } else {
        harness_with_clock(clock.clone()).await
    };
    let t = tenant(&h).await;
    let (s, r) = call(
        &h.app,
        "POST",
        "/admin/shared",
        Some(ADMIN),
        json!({
            "name": unique("shared"), "method": "lightning", "provider_type": "mock",
            "network": "regtest", "config": {}, "fee_bps": 100, "fee_base_msat": 0,
            "allow_untrusted": true,
        }),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let sid = r["id"].as_u64().unwrap();
    let tenant = payment_gateway::db::get_tenant(&h.pool, t.id)
        .await
        .unwrap()
        .unwrap();
    h.service.enable_shared(&tenant, sid).await.unwrap();
    Setup { h, clock, t, sid }
}

async fn open_attempt(s: &Setup) -> (String, String) {
    let p = new_payment(&s.h, &s.t, eur("o", 5000)).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (st, r) = attempt(&s.h, &s.t, &id, json!({"method": "lightning"})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    (
        id,
        r["attempts"][0]["details"]["payment_hash"]
            .as_str()
            .unwrap()
            .to_string(),
    )
}

async fn settle(s: &Setup) -> String {
    let (id, hash) = open_attempt(s).await;
    ProviderSink {
        service: s.h.service.clone(),
        provider_id: shared_key(s.sid),
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

fn fund(s: &Setup, msat: i64) {
    *mock(&s.h, shared_key(s.sid)).wallet.lock().unwrap() = vec![WalletBalance::new(
        "lnd-test#channels",
        "lightning",
        msat as u64,
    )];
}

async fn operator(h: &Harness, email: &str) {
    let hash = accounts::hash_password("correct horse battery").unwrap();
    accounts::insert_account(&h.pool, email, &hash, true)
        .await
        .unwrap();
}

#[tokio::test]
async fn balanced_books_raise_no_alert_and_record_the_run() {
    let s = setup(false).await;
    settle(&s).await;
    fund(&s, RECEIVED);
    let run = s.h.service.run_reconciliation("manual").await.unwrap();
    assert_eq!(run.liabilities, RECEIVED);
    assert_eq!(run.assets, RECEIVED);
    assert_eq!(run.difference, 0);
    assert_eq!(run.status, "ok");
    assert!(run.alert_list().is_empty());
    assert_eq!(run.platform_fees, RECEIVED / 100);
    assert_eq!(run.tenant_balances, RECEIVED - RECEIVED / 100);
    let runs = treasury::list_runs(&s.h.pool, 10).await.unwrap();
    assert_eq!(runs.len(), 1);
}

#[tokio::test]
async fn deficit_beyond_tolerance_alerts_by_banner_and_email() {
    let s = setup(true).await;
    operator(&s.h, "ops@example.com").await;
    settle(&s).await;
    fund(&s, RECEIVED - 5_000_000);
    let run = s.h.service.run_reconciliation("scheduled").await.unwrap();
    assert_eq!(run.difference, -5_000_000);
    assert_eq!(run.status, "alert");
    assert!(run.alert_list()[0].contains("liabilities exceed assets by 5000 sats"));
    let mails = s.h.mailer.wait_for(1).await;
    assert_eq!(mails[0].to, "ops@example.com");
    assert!(mails[0].subject.contains("Treasury alert"));

    s.h.service.treasury.set_tolerance_msat(5_000_000);
    let run = s.h.service.run_reconciliation("manual").await.unwrap();
    assert_eq!(run.status, "ok");
}

#[tokio::test]
async fn unreachable_wallet_is_an_alert_not_a_pass() {
    let s = setup(false).await;
    settle(&s).await;
    fund(&s, RECEIVED);
    *mock(&s.h, shared_key(s.sid)).fail_wallet.lock().unwrap() = true;
    let run = s.h.service.run_reconciliation("manual").await.unwrap();
    assert_eq!(run.wallet_errors, 1);
    assert_eq!(run.status, "alert");
}

#[tokio::test]
async fn drift_and_unbalanced_entries_are_reported_and_never_fixed() {
    let s = setup(false).await;
    settle(&s).await;
    fund(&s, RECEIVED);
    sqlx::query("update ledger_account set balance = balance + 7 where kind = 'platform_fees'")
        .execute(&s.h.pool)
        .await
        .unwrap();
    sqlx::query("insert into ledger_entry (transaction_id, account_id, amount, currency, created) select transaction_id, account_id, 5, currency, created from ledger_entry limit 1")
        .execute(&s.h.pool)
        .await
        .unwrap();
    let before = platform_fees(&s.h).await;
    let run = s.h.service.run_reconciliation("manual").await.unwrap();
    assert!(run.drift_accounts >= 1);
    assert!(run.unbalanced_transactions >= 1);
    assert_eq!(run.status, "alert");
    assert!(run.alert_list().iter().any(|a| a.contains("zero-sum")));
    assert!(
        run.alert_list()
            .iter()
            .any(|a| a.contains("cached balance"))
    );
    assert_eq!(platform_fees(&s.h).await, before);
    let rec = ledger::reconcile(&s.h.pool).await.unwrap();
    assert!(!rec.is_empty());
}

async fn platform_fees(h: &Harness) -> i64 {
    let (v,): (i64,) =
        sqlx::query_as("select balance from ledger_account where tenant_id is null and kind = ?")
            .bind(AccountKind::PlatformFees.as_str())
            .fetch_one(&h.pool)
            .await
            .unwrap();
    v
}

#[tokio::test]
async fn stuck_report_finds_each_case() {
    let s = setup(false).await;
    let (credit_id, hash) = open_attempt(&s).await;
    sqlx::query("update payment_attempt set status = 'paid', paid_at = ? where lookup = ?")
        .bind(s.clock_now())
        .bind(&hash)
        .execute(&s.h.pool)
        .await
        .unwrap();

    sqlx::query("insert into withdrawal (tenant_id, amount, currency, destination_kind, destination, destination_label, status, created, sending_since) values (?, 1000, 'BTC', 'lightning_address', 'x', 'x', 'sending', ?, ?)")
        .bind(s.t.id)
        .bind(s.clock_now())
        .bind(s.clock_now())
        .execute(&s.h.pool)
        .await
        .unwrap();

    let ob = provider(&s.h, &s.t, "onchain", json!({}), None).await;
    let p = new_payment(&s.h, &s.t, eur("c", 3000)).await;
    let chain_id = p["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(&s.h, &s.t, &chain_id, json!({"method": "onchain"})).await;
    let lookup = r["attempts"][0]["details"]["address"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: s.h.service.clone(),
        provider_id: ob,
    }
    .event(
        ProviderEvent::Paid {
            lookup: lookup.clone(),
            reference: "tx1".into(),
            amount: None,
            confirmed: false,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();

    let ln = provider(&s.h, &s.t, "lightning", json!({}), None).await;
    let q = new_payment(&s.h, &s.t, eur("w", 2000)).await;
    let wid = q["id"].as_str().unwrap().to_string();
    let (_, r) = attempt(
        &s.h,
        &s.t,
        &wid,
        json!({"method": "lightning", "provider_id": ln}),
    )
    .await;
    let wh = r["attempts"][0]["details"]["payment_hash"]
        .as_str()
        .unwrap()
        .to_string();
    ProviderSink {
        service: s.h.service.clone(),
        provider_id: ln,
    }
    .event(
        ProviderEvent::Paid {
            lookup: wh.clone(),
            reference: wh,
            amount: None,
            confirmed: true,
            saved: None,
        },
        None,
    )
    .await
    .unwrap();

    let fresh = treasury::find_stuck(&s.h.pool, s.clock_now(), 24 * 3600)
        .await
        .unwrap();
    assert!(
        fresh
            .iter()
            .all(|i| i.kind == StuckKind::SettledWithoutCredit)
    );

    s.clock.advance_secs(25 * 3600);
    let found = treasury::find_stuck(&s.h.pool, s.clock_now(), 24 * 3600)
        .await
        .unwrap();
    let kinds = |k: StuckKind| found.iter().filter(|i| i.kind == k).collect::<Vec<_>>();
    let credit = kinds(StuckKind::SettledWithoutCredit);
    assert_eq!(credit.len(), 1);
    assert_eq!(credit[0].reference, credit_id);
    assert_eq!(
        credit[0].link,
        format!("#/p/{}/payments/{credit_id}", s.t.id)
    );
    assert_eq!(kinds(StuckKind::WithdrawalSending).len(), 1);
    let receipts = kinds(StuckKind::UnconfirmedReceipt);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].reference, chain_id);
    let hooks = kinds(StuckKind::PaidWithoutWebhook);
    assert!(hooks.iter().any(|i| i.reference == wid));

    s.h.service.deliver_due().await.unwrap();
    let after = treasury::find_stuck(&s.h.pool, s.clock_now(), 24 * 3600)
        .await
        .unwrap();
    assert!(
        !after
            .iter()
            .any(|i| i.kind == StuckKind::PaidWithoutWebhook && i.reference == wid)
    );
}

impl Setup {
    fn clock_now(&self) -> chrono::DateTime<chrono::Utc> {
        use payment_gateway::lease::Clock;
        self.clock.now()
    }
}

#[tokio::test]
async fn scheduled_run_happens_hourly_only() {
    let s = setup(false).await;
    assert!(s.h.service.reconcile_if_due().await.unwrap().is_some());
    assert!(s.h.service.reconcile_if_due().await.unwrap().is_none());
    s.clock.advance_secs(3601);
    assert!(s.h.service.reconcile_if_due().await.unwrap().is_some());
}

fn parse_metrics(text: &str) -> std::collections::HashMap<String, f64> {
    let mut out = std::collections::HashMap::new();
    for line in text.lines() {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, value) = line.rsplit_once(' ').expect(line);
        out.insert(name.to_string(), value.parse::<f64>().expect(line));
    }
    out
}

#[tokio::test]
async fn metrics_expose_the_treasury_gauges_and_need_the_admin_token() {
    let s = setup(false).await;
    settle(&s).await;
    fund(&s, RECEIVED);
    s.h.service.run_reconciliation("manual").await.unwrap();
    let anon = send(&s.h.app, "GET", "/metrics", None, &[], Value::Null).await;
    assert_eq!(anon.status, StatusCode::UNAUTHORIZED);
    let r = send(&s.h.app, "GET", "/metrics", Some(ADMIN), &[], Value::Null).await;
    assert_eq!(r.status, StatusCode::OK);
    let m = parse_metrics(&r.text);
    assert_eq!(m["gateway_liabilities_msat"], RECEIVED as f64);
    assert_eq!(m["gateway_assets_msat"], RECEIVED as f64);
    assert_eq!(m["gateway_solvency_difference_msat"], 0.0);
    assert_eq!(m["gateway_ledger_drift_accounts"], 0.0);
    assert_eq!(m["gateway_withdrawals_pending"], 0.0);
    assert!(m.contains_key("gateway_webhook_oldest_pending_age_seconds"));
    assert!(r.text.contains("# TYPE gateway_liabilities_msat gauge"));
}

#[tokio::test]
async fn treasury_page_api_is_operator_only_and_lists_runs_and_tenants() {
    let s = setup(false).await;
    settle(&s).await;
    fund(&s, RECEIVED);
    let hash = accounts::hash_password("correct horse battery").unwrap();
    accounts::insert_account(&s.h.pool, "boss@example.com", &hash, true)
        .await
        .unwrap();
    accounts::insert_account(&s.h.pool, "cust@example.com", &hash, false)
        .await
        .unwrap();
    let boss = console_login(&s.h, "boss@example.com").await;
    let cust = console_login(&s.h, "cust@example.com").await;
    let r = console(&s.h, &cust, "GET", "/admin/treasury", Value::Null).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = console(&s.h, &cust, "POST", "/admin/treasury/run", json!({})).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
    let r = console(&s.h, &boss, "POST", "/admin/treasury/run", json!({})).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["status"], "ok");
    let r = console(&s.h, &boss, "GET", "/admin/treasury", Value::Null).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    assert_eq!(r.json["liabilities"], RECEIVED);
    assert_eq!(r.json["fees_earned"], RECEIVED / 100);
    assert_eq!(r.json["runs"].as_array().unwrap().len(), 1);
    assert_eq!(r.json["tenants"][0]["tenant_id"], s.t.id);
    assert_eq!(r.json["wallets"][0]["identity"], "lnd-test#channels");
    let r = console(&s.h, &boss, "GET", "/admin/treasury/banner", Value::Null).await;
    assert_eq!(r.json["alerts"], json!([]));
}

async fn console_login(h: &Harness, email: &str) -> (String, String) {
    let r = send(
        &h.app,
        "POST",
        "/console/api/login",
        None,
        &[],
        json!({"email": email, "password": "correct horse battery"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let cookie = r.headers["set-cookie"].to_str().unwrap();
    let cookie = cookie.split(';').next().unwrap().to_string();
    (cookie, r.json["csrf"].as_str().unwrap().to_string())
}

async fn console(
    h: &Harness,
    who: &(String, String),
    method: &str,
    path: &str,
    body: Value,
) -> Reply {
    send(
        &h.app,
        method,
        &format!("/console/api{path}"),
        None,
        &[("cookie", who.0.as_str()), ("x-csrf-token", who.1.as_str())],
        body,
    )
    .await
}
