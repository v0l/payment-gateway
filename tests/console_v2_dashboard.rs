mod common;
use axum::http::StatusCode;
use common::console::*;
use common::*;
use payment_gateway::db;
use serde_json::{Value, json};

async fn with_method(
    h: &Harness,
    p: &Project,
    reference: &str,
    cents: u64,
    customer: &str,
    method: &str,
    minutes: i64,
) -> String {
    let mut body = eur(reference, cents);
    body["customer_ref"] = json!(customer);
    let id = payment(h, p, body).await;
    force_paid(h, &id, minutes).await;
    sqlx::query(
        "insert into payment_attempt (payment_id, provider_config_id, watch_provider_id, method, base_amount, currency, charge_amount, charge_currency, lookup, details, status, created, expires) select id, (select min(id) from provider_config where tenant_id = ?), (select min(id) from provider_config where tenant_id = ?), ?, amount, currency, amount, currency, ?, '{}', 'paid', created, expires from payment where id = ?",
    )
    .bind(p.id)
    .bind(p.id)
    .bind(method)
    .bind(format!("lk-{reference}"))
    .bind(&pid(&id)[..])
    .execute(&h.pool)
    .await
    .unwrap();
    id
}

async fn fixture(h: &Harness, c: &Client) -> Project {
    let p = project(c, "shop").await;
    let t = TenantFixture {
        id: p.id,
        key: p.key.clone(),
        webhook_secret: p.secret.clone(),
        receiver: Receiver::start().await,
    };
    provider(h, &t, "lightning", json!({}), None).await;
    with_method(h, &p, "a", 1000, "cus_1", "lightning", 2).await;
    with_method(h, &p, "b", 3000, "cus_1", "onchain", 10).await;
    with_method(h, &p, "c", 500, "cus_2", "lightning", 6).await;
    let e = payment(h, &p, eur("expired", 200)).await;
    sqlx::query("update payment set status = 'expired' where id = ?")
        .bind(&pid(&e)[..])
        .execute(&h.pool)
        .await
        .unwrap();
    payment(h, &p, eur("pend", 777)).await;
    let old = with_method(h, &p, "old", 99999, "cus_9", "lightning", 1).await;
    sqlx::query("update payment set created = created - interval 20 day where id = ?")
        .bind(&pid(&old)[..])
        .execute(&h.pool)
        .await
        .unwrap();
    p
}

#[tokio::test]
async fn dashboard_numbers_match_the_fixture() {
    let h = harness().await;
    let c = signed_in(&h, "a@example.com", false).await;
    let p = fixture(&h, &c).await;
    let (s, r) = c
        .req(
            "GET",
            &format!("/projects/{}/dashboard?days=7", p.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["paid"], 3);
    assert_eq!(r["failed"], 1);
    assert_eq!(r["success_rate"], 0.75);
    assert_eq!(r["avg_settlement_seconds"], 360.0);
    assert_eq!(r["pending"][0]["value"], 777);
    let vol = r["volume"].as_array().unwrap();
    let ln = vol.iter().find(|v| v["method"] == "lightning").unwrap();
    assert_eq!(ln["volume"], 1500);
    assert_eq!(ln["payments"], 2);
    assert_eq!(r["top_customers"][0]["customer_ref"], "cus_1");
    assert_eq!(r["top_customers"][0]["volume"], 4000);
    let daily = r["daily"].as_array().unwrap();
    assert_eq!(
        daily
            .iter()
            .map(|d| d["volume"].as_i64().unwrap())
            .sum::<i64>(),
        4500
    );
    assert_eq!(
        daily
            .iter()
            .map(|d| d["payments"].as_i64().unwrap())
            .sum::<i64>(),
        3
    );
    assert!(
        daily
            .iter()
            .all(|d| d["day"].as_str().unwrap().len() == 10 && d["currency"] == "EUR")
    );
    let (_, r30) = c
        .req(
            "GET",
            &format!("/projects/{}/dashboard?days=30", p.id),
            Value::Null,
        )
        .await;
    assert_eq!(r30["paid"], 4);
    assert_eq!(r30["days"], 30);
}

#[tokio::test]
async fn dashboard_rejects_other_windows_and_is_tenant_scoped() {
    let h = harness().await;
    let a = signed_in(&h, "a@example.com", false).await;
    let b = signed_in(&h, "b@example.com", false).await;
    let pa = fixture(&h, &a).await;
    let pb = project(&b, "b").await;
    let (s, _) = a
        .req(
            "GET",
            &format!("/projects/{}/dashboard?days=5", pa.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = b
        .req(
            "GET",
            &format!("/projects/{}/dashboard", pa.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, r) = b
        .req(
            "GET",
            &format!("/projects/{}/dashboard", pb.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r["paid"], 0);
    assert!(r["success_rate"].is_null());
    assert!(r["volume"].as_array().unwrap().is_empty());
    assert!(r["daily"].as_array().unwrap().is_empty());
    assert!(r["top_customers"].as_array().unwrap().is_empty());
    assert!(r["pending"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn operator_tools_are_operator_only_and_audit_support_views() {
    let h = harness().await;
    let op = signed_in(&h, "op@example.com", true).await;
    let user = signed_in(&h, "u@example.com", false).await;
    let p = fixture(&h, &user).await;
    for path in [
        "/operator/tenants",
        "/operator/search?q=abcdef",
        "/operator/health",
        &format!("/operator/tenants/{}/support", p.id),
    ] {
        let (s, _) = user.req("GET", path, Value::Null).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{path}");
    }
    let (s, r) = op.req("GET", "/operator/tenants", Value::Null).await;
    assert_eq!(s, StatusCode::OK, "{r}");
    let row = r["tenants"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == p.id)
        .unwrap();
    assert_eq!(row["enabled"], true);
    assert!(row["last_payment"].is_string());
    let (s, r) = op
        .req(
            "GET",
            &format!("/operator/tenants/{}/support", p.id),
            Value::Null,
        )
        .await;
    assert_eq!(s, StatusCode::OK, "{r}");
    assert_eq!(r["read_only"], true);
    assert!(r["recent_payments"].as_array().unwrap().len() >= 4);
    assert!(!r.to_string().contains(&p.key));
    assert!(!r.to_string().contains(&p.secret));
    let n: i64 = sqlx::query_scalar(
        "select count(*) from audit_event where action = 'support.viewed' and tenant_id = ?",
    )
    .bind(p.id)
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!(n, 1);
    let (s, _) = op
        .req("GET", "/operator/tenants/999999/support", Value::Null)
        .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, r) = op.req("GET", "/operator/health", Value::Null).await;
    assert_eq!(s, StatusCode::OK);
    assert!(r["pending_payments"].as_i64().unwrap() >= 1);
    let _ = db::tenant_has_pending;
}

#[tokio::test]
async fn global_search_finds_by_id_reference_invoice_and_txid() {
    let h = harness().await;
    let op = signed_in(&h, "op@example.com", true).await;
    let user = signed_in(&h, "u@example.com", false).await;
    let p = fixture(&h, &user).await;
    let id: String = sqlx::query_scalar("select lower(hex(id)) from payment where reference = 'a'")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    sqlx::query("insert into attempt_receipt (attempt_id, ref, amount, confirmed, created) select a.id, 'tx_deadbeef_0', 1, 1, now() from payment_attempt a where a.lookup = 'lk-a'")
        .execute(&h.pool).await.unwrap();
    for (q, matched) in [
        (&id[..12], "id"),
        ("lk-b", "invoice"),
        ("tx_deadbeef_0", "txid"),
        ("expired", "reference"),
    ] {
        let (s, r) = op
            .req("GET", &format!("/operator/search?q={q}"), Value::Null)
            .await;
        assert_eq!(s, StatusCode::OK, "{r}");
        let hits = r["hits"].as_array().unwrap();
        assert_eq!(hits.len(), 1, "{q}: {r}");
        assert_eq!(hits[0]["matched"], matched);
        assert_eq!(hits[0]["tenant_id"], p.id);
    }
    let (s, _) = op.req("GET", "/operator/search?q=ab", Value::Null).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}
