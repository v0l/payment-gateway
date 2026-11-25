mod common;
use axum::http::StatusCode;
use common::console::*;
use common::*;
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::ProviderSink;
use serde_json::{Value, json};

struct Shop {
    p: Project,
    t: TenantFixture,
    ln: u64,
    oc: u64,
}

async fn shop(h: &Harness, c: &Client, name: &str) -> Shop {
    let p = project(c, name).await;
    let t = TenantFixture {
        id: p.id,
        key: p.key.clone(),
        webhook_secret: p.secret.clone(),
        receiver: Receiver::start().await,
    };
    let ln = provider(h, &t, "lightning", json!({}), None).await;
    let oc = provider(h, &t, "onchain", json!({}), None).await;
    payment_gateway::db::set_tenant_trusted(&h.pool, p.id, true)
        .await
        .unwrap();
    let (st, r) = c
        .req(
            "POST",
            &format!("/projects/{}/webhook-endpoints", p.id),
            json!({"url": t.receiver.url, "events": []}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    Shop { p, t, ln, oc }
}

async fn open(
    h: &Harness,
    s: &Shop,
    cents: u64,
    method: &str,
    desc: &str,
) -> (String, String, u64) {
    let mut body = eur(desc, cents);
    body["description"] = json!(desc);
    body["customer_ref"] = json!(format!("cus_{desc}"));
    body["customer_email"] = json!(format!("{desc}@example.com"));
    let p = new_payment(h, &s.t, body).await;
    let id = p["id"].as_str().unwrap().to_string();
    let (st, r) = attempt(h, &s.t, &id, json!({"method": method})).await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let a = r["attempts"].as_array().unwrap().last().unwrap().clone();
    let lookup = if method == "onchain" {
        a["details"]["address"].as_str().unwrap().to_string()
    } else {
        a["details"]["payment_hash"].as_str().unwrap().to_string()
    };
    (id, lookup, a["amount"].as_u64().unwrap())
}

async fn event(h: &Harness, provider_id: u64, ev: ProviderEvent) {
    let sink = ProviderSink {
        service: h.service.clone(),
        provider_id,
    };
    sink.event(ev, None).await.unwrap();
}

fn paid(lookup: &str, amount: Option<u64>, confirmed: bool) -> ProviderEvent {
    ProviderEvent::Paid {
        lookup: lookup.into(),
        reference: format!("tx-{lookup}"),
        amount,
        confirmed,
        saved: None,
    }
}

async fn shift(h: &Harness, id: &str, hours: i64) {
    for sql in [
        "update payment set created = created - interval ? hour, paid_at = paid_at - interval ? hour where id = ?",
    ] {
        sqlx::query(sql)
            .bind(hours)
            .bind(hours)
            .bind(&pid(id)[..])
            .execute(&h.pool)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn home_compares_today_with_the_same_time_last_week() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let s = shop(&h, &c, "shop").await;
    let (today, lk, _) = open(&h, &s, 1800, "lightning", "today").await;
    event(&h, s.ln, paid(&lk, None, true)).await;
    let (old, lk, _) = open(&h, &s, 5000, "lightning", "lastweek").await;
    event(&h, s.ln, paid(&lk, None, true)).await;
    shift(&h, &old, 24 * 7).await;
    let (older, lk, _) = open(&h, &s, 700, "lightning", "lastweeklater").await;
    event(&h, s.ln, paid(&lk, None, true)).await;
    shift(&h, &older, 24 * 7 - 1).await;
    let _ = today;

    let (st, r) = c
        .req(
            "GET",
            &format!("/projects/{}/home?tz=0", s.p.id),
            Value::Null,
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let series = r["series"].as_array().unwrap();
    assert_eq!(series.len(), 1, "{r}");
    let eur = &series[0];
    assert_eq!(eur["currency"], "EUR");
    assert_eq!(eur["today_total"], 1800);
    assert_eq!(eur["today_payments"], 1);
    assert_eq!(eur["last_week"].as_array().unwrap().len(), 24);
    assert_eq!(eur["last_week_total"], 5700);
    let hours = r["hours_elapsed"].as_u64().unwrap() as usize;
    assert_eq!(eur["today"].as_array().unwrap().len(), hours);
    let same_time = eur["last_week_same_time"].as_i64().unwrap();
    assert!(same_time == 5000 || same_time == 5700, "{same_time}");

    let figs = r["month"]["figures"].as_array().unwrap();
    let f = figs.iter().find(|f| f["currency"] == "EUR").unwrap();
    assert!(f["paid"].as_i64().unwrap() >= 1800);
    assert!(f["success_rate"].as_f64().unwrap() > 0.99);
    assert_eq!(r["health"].as_array().unwrap().len(), 2);
    assert!(
        r["health"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["state"] == "ok")
    );
    assert_eq!(r["funds"]["available"], 0);
    let wallets = r["funds"]["wallets"].as_array().unwrap();
    assert_eq!(wallets.len(), 2, "{r}");
    assert!(wallets.iter().all(|w| w["available"].is_null()));
}

#[tokio::test]
async fn home_and_feed_reject_bad_parameters_and_other_tenants() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let s = shop(&h, &a, "a").await;
    let (_, lk, _) = open(&h, &s, 999, "lightning", "secretdesc").await;
    event(&h, s.ln, paid(&lk, None, true)).await;
    for path in [
        format!("/projects/{}/home?tz=9999", s.p.id),
        format!("/projects/{}/home?tz=abc", s.p.id),
        format!("/projects/{}/feed?kind=nope", s.p.id),
        format!("/projects/{}/feed?before=yesterday", s.p.id),
        format!("/projects/{}/statement?limit=0", s.p.id),
        format!("/projects/{}/statement?before=x", s.p.id),
    ] {
        let (st, _) = a.req("GET", &path, Value::Null).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{path}");
    }
    for path in [
        "home",
        "feed",
        "attention",
        "payment-view-counts",
        "statement",
    ] {
        let (st, r) = b
            .req("GET", &format!("/projects/{}/{path}", s.p.id), Value::Null)
            .await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{path}: {r}");
        assert!(!r.to_string().contains("secretdesc"));
    }
    let (st, _) = b
        .req(
            "POST",
            &format!("/projects/{}/attention/bulk", s.p.id),
            json!({"action": "cancel", "payments": ["00"]}),
        )
        .await;
    assert_eq!(st, StatusCode::NOT_FOUND);
    let other = shop(&h, &b, "b").await;
    let (_, r) = b
        .req(
            "GET",
            &format!("/projects/{}/feed", other.p.id),
            Value::Null,
        )
        .await;
    assert!(r["items"].as_array().unwrap().is_empty(), "{r}");
    let (_, r) = b
        .req(
            "GET",
            &format!("/projects/{}/home", other.p.id),
            Value::Null,
        )
        .await;
    assert!(r["series"].as_array().unwrap().is_empty(), "{r}");
}

#[tokio::test]
async fn attention_groups_disputes_late_short_confirming_and_declined() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let s = shop(&h, &c, "shop").await;

    let (disputed, lk, _) = open(&h, &s, 14900, "lightning", "host").await;
    event(&h, s.ln, paid(&lk, None, true)).await;
    event(
        &h,
        s.ln,
        ProviderEvent::Dispute {
            lookup: lk.clone(),
            state: payment_gateway::providers::DisputeState::Open,
            reason: Some("not received".into()),
        },
    )
    .await;

    let (late, lk, _) = open(&h, &s, 1800, "lightning", "late").await;
    let (st, _) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{late}/cancel"),
        Some(&s.t.key),
        Value::Null,
    )
    .await;
    assert_eq!(st, StatusCode::OK);
    event(&h, s.ln, paid(&lk, None, true)).await;

    let (short, addr, need) = open(&h, &s, 5400, "onchain", "short").await;
    event(&h, s.oc, paid(&addr, Some(need / 2), true)).await;

    let (confirming, addr, _) = open(&h, &s, 3000, "onchain", "confirming").await;
    event(&h, s.oc, paid(&addr, None, false)).await;

    let (declined, lk, _) = open(&h, &s, 2500, "lightning", "declined").await;
    event(
        &h,
        s.ln,
        ProviderEvent::Failed {
            lookup: lk,
            reason: "insufficient funds".into(),
        },
    )
    .await;
    let (waiting, _, _) = open(&h, &s, 100, "lightning", "waiting").await;

    let (st, r) = c
        .req(
            "GET",
            &format!("/projects/{}/attention", s.p.id),
            Value::Null,
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let groups = r["groups"].as_array().unwrap();
    let ids = |kind: &str| -> Vec<String> {
        groups
            .iter()
            .find(|g| g["kind"] == kind)
            .map(|g| {
                g["rows"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|r| r["payment_id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };
    assert_eq!(ids("disputes"), vec![disputed.clone()]);
    let wrong = ids("wrong_amount");
    assert!(wrong.contains(&late) && wrong.contains(&short), "{r}");
    assert_eq!(ids("confirming"), vec![confirming.clone()]);
    assert_eq!(ids("declined"), vec![declined.clone()]);
    assert_eq!(r["open"], 5);
    let all: Vec<String> = groups
        .iter()
        .flat_map(|g| g["rows"].as_array().unwrap().iter())
        .filter_map(|r| r["payment_id"].as_str().map(str::to_string))
        .collect();
    assert!(!all.contains(&waiting));
    let w = r["waiting"].as_array().unwrap();
    assert_eq!(w[0]["payments"], 1, "{r}");
    assert_eq!(w[0]["volume"], 100);
    let short_row = groups.iter().find(|g| g["kind"] == "wrong_amount").unwrap()["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["payment_id"] == short)
        .unwrap()
        .clone();
    assert!(
        short_row["why"].as_str().unwrap().contains("short"),
        "{short_row}"
    );

    let (st, r) = c
        .req(
            "GET",
            &format!("/projects/{}/payment-view-counts", s.p.id),
            Value::Null,
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["all"], 6);
    assert_eq!(r["disputed"], 1);
    assert_eq!(r["pending"], 4);
    assert_eq!(r["last_7_days"], 6);
}

#[tokio::test]
async fn bulk_cancel_and_accept_respect_role_and_the_manual_paid_switch() {
    let h = harness().await;
    let owner = signed_in(&h, "o@example.com", false).await;
    let s = shop(&h, &owner, "shop").await;
    let (a, _, _) = open(&h, &s, 500, "lightning", "a").await;
    let (b, _, _) = open(&h, &s, 600, "lightning", "b").await;
    let path = format!("/projects/{}/attention/bulk", s.p.id);

    let (st, _) = owner
        .req("POST", &path, json!({"action": "explode", "payments": [a]}))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, _) = owner
        .req("POST", &path, json!({"action": "cancel", "payments": []}))
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, r) = owner
        .req(
            "POST",
            &path,
            json!({"action": "mark_paid", "payments": [a], "reason": "seen in bank"}),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN, "{r}");

    let (st, _) = owner
        .req(
            "POST",
            &format!("/projects/{}/manual-paid", s.p.id),
            json!({"enabled": true}),
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let (st, _) = owner
        .req(
            "POST",
            &path,
            json!({"action": "mark_paid", "payments": [a], "reason": "x"}),
        )
        .await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, r) = owner
        .req(
            "POST",
            &path,
            json!({"action": "mark_paid", "payments": [a], "reason": "seen in bank"}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["done"], json!([a]));

    let other = signed_in(&h, "x@example.com", false).await;
    let theirs = shop(&h, &other, "theirs").await;
    let (foreign, _, _) = open(&h, &theirs, 700, "lightning", "foreign").await;
    let (st, r) = owner
        .req(
            "POST",
            &path,
            json!({"action": "cancel", "payments": [b, foreign, "zz"]}),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["done"], json!([b]));
    assert_eq!(r["failed"].as_array().unwrap().len(), 2);
    let status = |id: &str| {
        let id = id.to_string();
        let h = &h;
        async move {
            let (s,): (String,) = sqlx::query_as("select status from payment where id = ?")
                .bind(&pid(&id)[..])
                .fetch_one(&h.pool)
                .await
                .unwrap();
            s
        }
    };
    assert_eq!(status(&a).await, "paid");
    assert_eq!(status(&b).await, "cancelled");
    assert_eq!(status(&foreign).await, "pending");
}

#[tokio::test]
async fn feed_lists_paid_declined_and_failed_webhooks_newest_first() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let s = shop(&h, &c, "shop").await;
    let (p1, lk, _) = open(&h, &s, 1800, "lightning", "vps").await;
    event(&h, s.ln, paid(&lk, None, true)).await;
    let (_, lk, _) = open(&h, &s, 2500, "lightning", "topup").await;
    event(
        &h,
        s.ln,
        ProviderEvent::Failed {
            lookup: lk,
            reason: "insufficient funds".into(),
        },
    )
    .await;
    sqlx::query("update webhook_delivery set last_error = 'connection refused', last_status = 503, last_attempt_at = created where tenant_id = ? and event_type = 'payment.paid'")
        .bind(s.p.id)
        .execute(&h.pool)
        .await
        .unwrap();

    let (st, r) = c
        .req("GET", &format!("/projects/{}/feed", s.p.id), Value::Null)
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let items = r["items"].as_array().unwrap();
    let titles: Vec<&str> = items.iter().map(|i| i["title"].as_str().unwrap()).collect();
    assert!(titles.contains(&"Paid"), "{titles:?}");
    assert!(titles.contains(&"Declined"), "{titles:?}");
    assert!(
        titles.iter().any(|t| t.starts_with("Webhook payment.paid")),
        "{titles:?}"
    );
    let paid_item = items.iter().find(|i| i["title"] == "Paid").unwrap();
    assert_eq!(paid_item["payment_id"], p1);
    assert_eq!(paid_item["method"], "lightning");
    assert_eq!(paid_item["reference"], "vps");
    let times: Vec<&str> = items.iter().map(|i| i["at"].as_str().unwrap()).collect();
    let mut sorted = times.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(times, sorted);

    let (_, r) = c
        .req(
            "GET",
            &format!("/projects/{}/feed?kind=webhooks", s.p.id),
            Value::Null,
        )
        .await;
    let items = r["items"].as_array().unwrap();
    assert!(!items.is_empty());
    assert!(items.iter().all(|i| i["kind"] == "webhook"), "{r}");
    assert!(items[0]["detail"].as_str().unwrap().contains("503"));
    let (_, r) = c
        .req(
            "GET",
            &format!("/projects/{}/feed?kind=payouts", s.p.id),
            Value::Null,
        )
        .await;
    assert!(r["items"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn feed_shows_an_unconfirmed_deposit_until_it_confirms() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let s = shop(&h, &c, "shop").await;
    let (id, lk, amount) = open(&h, &s, 1800, "onchain", "vps").await;
    event(&h, s.oc, paid(&lk, Some(amount), false)).await;

    let (st, r) = c
        .req("GET", &format!("/projects/{}/feed", s.p.id), Value::Null)
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let items = r["items"].as_array().unwrap();
    let seen = items
        .iter()
        .find(|i| i["title"] == "Deposit seen, waiting for confirmation")
        .unwrap_or_else(|| panic!("{items:?}"));
    assert_eq!(seen["payment_id"], id);
    assert_eq!(seen["method"], "onchain");

    event(&h, s.oc, paid(&lk, Some(amount), true)).await;
    let (_, r) = c
        .req(
            "GET",
            &format!("/projects/{}/payments/{id}", s.p.id),
            Value::Null,
        )
        .await;
    let events: Vec<&str> = r["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert_eq!(events, vec!["attempt.received", "payment.paid"]);
}

#[tokio::test]
async fn payment_detail_has_events_deliveries_and_a_masked_email() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let s = shop(&h, &c, "shop").await;
    let (id, lk, _) = open(&h, &s, 1800, "lightning", "amelia").await;
    event(&h, s.ln, paid(&lk, None, true)).await;
    let (st, r) = c
        .req(
            "GET",
            &format!("/projects/{}/payments/{id}", s.p.id),
            Value::Null,
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    assert_eq!(r["customer_email"], "am***@example.com");
    assert!(!r.to_string().contains("amelia@example.com"));
    let events: Vec<&str> = r["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["type"].as_str().unwrap())
        .collect();
    assert!(events.contains(&"payment.paid"), "{events:?}");
    let d = &r["deliveries"][0];
    assert_eq!(d["event_type"], "payment.paid");
    assert!(d.get("url").is_some() && d.get("last_status").is_some());
}

#[tokio::test]
async fn statement_runs_the_balance_back_from_the_latest_entry() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let s = shop(&h, &c, "shop").await;
    let tenant = payment_gateway::db::get_tenant(&h.pool, s.p.id)
        .await
        .unwrap()
        .unwrap();
    let mut tx = h.pool.begin().await.unwrap();
    use payment_gateway::ledger::{self, AccountKind, AccountRef, Entry, Posting, TxKind};
    for (i, amount) in [100_000i64, 250_000, -50_000].iter().enumerate() {
        let posting = Posting::new(
            TxKind::Adjustment,
            vec![
                Entry::new(
                    AccountRef::tenant(tenant.id, AccountKind::TenantBalance, "BTC"),
                    *amount,
                ),
                Entry::new(
                    AccountRef::platform(AccountKind::PlatformWallet, "BTC"),
                    -amount,
                ),
            ],
        );
        ledger::post(&mut tx, &posting, &format!("test:{}:{i}", tenant.id))
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    let (st, r) = c
        .req(
            "GET",
            &format!("/projects/{}/statement", s.p.id),
            Value::Null,
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{r}");
    let lines = r.as_array().unwrap();
    let after: Vec<i64> = lines
        .iter()
        .map(|l| l["balance_after"].as_i64().unwrap())
        .collect();
    assert_eq!(after, vec![300_000, 350_000, 100_000]);
    let first = lines[2]["id"].as_u64().unwrap();
    let (_, r) = c
        .req(
            "GET",
            &format!("/projects/{}/statement?before={}", s.p.id, lines[1]["id"]),
            Value::Null,
        )
        .await;
    assert_eq!(r.as_array().unwrap().len(), 1);
    assert_eq!(r[0]["id"].as_u64().unwrap(), first);
}
