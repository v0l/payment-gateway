mod common;
use axum::http::StatusCode;
use common::*;
use payment_gateway::accounts;
use payment_gateway::ledger::{
    self, AccountKind, AccountRef, Entry, LedgerError, Posting, TxKind, post,
};
use rand::{Rng, SeedableRng, rngs::StdRng};
use serde_json::{Value, json};

fn wallet() -> AccountRef {
    AccountRef::platform(AccountKind::PlatformWallet, "BTC")
}

fn owed(tenant: u64) -> AccountRef {
    AccountRef::tenant(tenant, AccountKind::TenantBalance, "BTC")
}

fn fees() -> AccountRef {
    AccountRef::platform(AccountKind::PlatformFees, "BTC")
}

fn transfer(from: AccountRef, to: AccountRef, amount: i64) -> Posting {
    Posting::new(
        TxKind::Adjustment,
        vec![Entry::new(from, -amount), Entry::new(to, amount)],
    )
}

async fn post_one(h: &Harness, p: &Posting, key: &str) -> Result<ledger::Posted, LedgerError> {
    let mut tx = h.pool.begin().await.unwrap();
    let r = post(&mut tx, p, key).await;
    match r {
        Ok(v) => {
            tx.commit().await.unwrap();
            Ok(v)
        }
        Err(e) => {
            tx.rollback().await.unwrap();
            Err(e)
        }
    }
}

async fn fund(h: &Harness, tenant: u64, amount: i64, key: &str) {
    post_one(h, &transfer(wallet(), owed(tenant), amount), key)
        .await
        .unwrap();
}

async fn total_value(h: &Harness) -> i64 {
    let (v,): (i64,) =
        sqlx::query_as("select cast(coalesce(sum(balance), 0) as signed) from ledger_account")
            .fetch_one(&h.pool)
            .await
            .unwrap();
    v
}

#[test]
fn validate_refuses_unbalanced_empty_and_zero() {
    let e = |a: AccountRef, n: i64| Entry::new(a, n);
    assert!(matches!(ledger::validate(&[]), Err(LedgerError::Empty)));
    assert!(matches!(
        ledger::validate(&[e(wallet(), 5)]),
        Err(LedgerError::Empty)
    ));
    assert!(matches!(
        ledger::validate(&[e(wallet(), -5), e(owed(1), 4)]),
        Err(LedgerError::Unbalanced { .. })
    ));
    assert!(matches!(
        ledger::validate(&[e(wallet(), 0), e(owed(1), 0)]),
        Err(LedgerError::ZeroAmount)
    ));
    let eur = AccountRef::tenant(1, AccountKind::TenantBalance, "EUR");
    assert!(matches!(
        ledger::validate(&[e(wallet(), -5), e(owed(1), 5), e(eur, 3), e(fees(), -3)]),
        Err(LedgerError::Unbalanced { .. })
    ));
    assert!(matches!(
        ledger::validate(&[
            e(AccountRef::platform(AccountKind::TenantBalance, "BTC"), -1),
            e(wallet(), 1)
        ]),
        Err(LedgerError::BadAccount(_))
    ));
    assert!(ledger::validate(&[e(wallet(), -5), e(owed(1), 5)]).is_ok());
}

#[tokio::test]
async fn unbalanced_posting_writes_nothing() {
    let h = harness().await;
    let t = tenant(&h).await;
    let bad = Posting::new(
        TxKind::Adjustment,
        vec![Entry::new(wallet(), -10), Entry::new(owed(t.id), 9)],
    );
    assert!(matches!(
        post_one(&h, &bad, "k1").await,
        Err(LedgerError::Unbalanced { .. })
    ));
    let (n,): (i64,) = sqlx::query_as("select count(*) from ledger_transaction")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn overdraw_refused_unless_account_allows_it() {
    let h = harness().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 1_000, "fund").await;
    let over = transfer(owed(t.id), fees(), 1_001);
    assert!(matches!(
        post_one(&h, &over, "over").await,
        Err(LedgerError::Overdraw { .. })
    ));
    assert_eq!(
        ledger::tenant_balance(&h.pool, t.id, "BTC").await.unwrap(),
        1_000
    );
    post_one(&h, &transfer(owed(t.id), fees(), 1_000), "exact")
        .await
        .unwrap();
    assert_eq!(
        ledger::tenant_balance(&h.pool, t.id, "BTC").await.unwrap(),
        0
    );

    let mut tx = h.pool.begin().await.unwrap();
    ledger::set_allow_negative(&mut tx, &owed(t.id), true)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    post_one(&h, &over, "over2").await.unwrap();
    assert_eq!(
        ledger::tenant_balance(&h.pool, t.id, "BTC").await.unwrap(),
        -1_001
    );
    assert!(ledger::reconcile(&h.pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn replaying_a_key_changes_nothing() {
    let h = harness().await;
    let t = tenant(&h).await;
    let p = transfer(wallet(), owed(t.id), 500);
    let first = post_one(&h, &p, "same").await.unwrap();
    assert!(!first.replayed);
    let again = post_one(&h, &transfer(wallet(), owed(t.id), 999), "same")
        .await
        .unwrap();
    assert!(again.replayed);
    assert_eq!(again.transaction_id, first.transaction_id);
    assert_eq!(
        ledger::tenant_balance(&h.pool, t.id, "BTC").await.unwrap(),
        500
    );
    let (n,): (i64,) = sqlx::query_as("select count(*) from ledger_entry")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(n, 2);
}

#[tokio::test]
async fn random_postings_keep_every_invariant() {
    let h = harness().await;
    let a = tenant(&h).await;
    let b = tenant(&h).await;
    let mut rng = StdRng::seed_from_u64(738);
    let currencies = ["BTC", "EUR"];
    let mut accepted = 0;
    let mut keys: Vec<(String, Posting)> = Vec::new();
    for i in 0..150 {
        let cur = currencies[rng.gen_range(0..2)];
        let pool = [
            AccountRef::tenant(a.id, AccountKind::TenantBalance, cur),
            AccountRef::tenant(b.id, AccountKind::TenantBalance, cur),
            AccountRef::platform(AccountKind::PlatformFees, cur),
            AccountRef::platform(AccountKind::PlatformWallet, cur),
            AccountRef::tenant(a.id, AccountKind::PendingPayout, cur),
            AccountRef::tenant(b.id, AccountKind::RefundReserve, cur),
        ];
        let legs = rng.gen_range(2..5);
        let mut entries: Vec<Entry> = Vec::new();
        let mut sum = 0i64;
        for _ in 0..legs - 1 {
            let amt = rng.gen_range(-5_000i64..5_000);
            sum += amt;
            entries.push(Entry::new(pool[rng.gen_range(0..pool.len())].clone(), amt));
        }
        let last = if rng.gen_bool(0.1) { -sum + 1 } else { -sum };
        entries.push(Entry::new(pool[rng.gen_range(0..pool.len())].clone(), last));
        let p = Posting::new(TxKind::Adjustment, entries);
        let key = format!("rand-{i}");
        let balanced = ledger::validate(&p.entries).is_ok();
        let r = post_one(&h, &p, &key).await;
        match r {
            Ok(v) => {
                assert!(balanced && !v.replayed);
                accepted += 1;
                keys.push((key, p));
            }
            Err(LedgerError::Overdraw { .. }) => assert!(balanced),
            Err(_) => assert!(!balanced || p.entries.iter().any(|e| e.amount == 0)),
        }
        assert_eq!(total_value(&h).await, 0);
    }
    assert!(accepted > 10);
    let before = ledger::reconcile(&h.pool).await.unwrap();
    assert!(before.is_empty(), "{before:?}");
    for (key, p) in &keys {
        assert!(post_one(&h, p, key).await.unwrap().replayed);
    }
    assert!(ledger::reconcile(&h.pool).await.unwrap().is_empty());
    assert_eq!(total_value(&h).await, 0);
    let (neg,): (i64,) = sqlx::query_as(
        "select count(*) from ledger_account where balance < 0 and allow_negative = false",
    )
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert_eq!(neg, 0);
}

#[tokio::test]
async fn fifty_parallel_postings_on_one_account_stay_consistent() {
    let h = harness().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 1_000, "seed").await;
    let mut set = tokio::task::JoinSet::new();
    for i in 0..50 {
        let pool = h.pool.clone();
        let id = t.id;
        set.spawn(async move {
            let (from, to) = if i % 2 == 0 {
                (wallet(), owed(id))
            } else {
                (owed(id), fees())
            };
            let p = transfer(from, to, 10 + i as i64);
            for attempt in 0..20 {
                let mut tx = pool.begin().await.unwrap();
                match post(&mut tx, &p, &format!("par-{i}")).await {
                    Ok(_) => {
                        tx.commit().await.unwrap();
                        return;
                    }
                    Err(LedgerError::Db(_)) => {
                        let _ = tx.rollback().await;
                        tokio::time::sleep(std::time::Duration::from_millis(10 * attempt)).await;
                    }
                    Err(e) => panic!("{e}"),
                }
            }
            panic!("posting {i} never committed");
        });
    }
    while let Some(r) = set.join_next().await {
        r.unwrap();
    }
    let credits: i64 = (0..50).filter(|i| i % 2 == 0).map(|i| 10 + i).sum();
    let debits: i64 = (0..50).filter(|i| i % 2 == 1).map(|i| 10 + i).sum();
    assert_eq!(
        ledger::tenant_balance(&h.pool, t.id, "BTC").await.unwrap(),
        1_000 + credits - debits
    );
    assert!(ledger::reconcile(&h.pool).await.unwrap().is_empty());
    assert_eq!(total_value(&h).await, 0);
}

#[tokio::test]
async fn reconcile_detects_a_tampered_cache() {
    let h = harness().await;
    let t = tenant(&h).await;
    fund(&h, t.id, 700, "f").await;
    sqlx::query("update ledger_account set balance = balance + 1 where tenant_id = ?")
        .bind(t.id)
        .execute(&h.pool)
        .await
        .unwrap();
    let bad = ledger::reconcile_tenant(&h.pool, t.id).await.unwrap();
    assert_eq!(bad.len(), 1);
    assert_eq!((bad[0].cached, bad[0].computed), (701, 700));
}

#[tokio::test]
async fn admin_endpoints_show_balance_and_history() {
    let h = harness().await;
    let t = tenant(&h).await;
    let other = tenant(&h).await;
    let p = transfer(wallet(), owed(t.id), 2_500).reference("payment", "abc");
    post_one(&h, &p, "pay-abc").await.unwrap();
    fund(&h, other.id, 77, "other").await;

    let (s, b) = call(
        &h.app,
        "GET",
        &format!("/admin/tenants/{}/balance", t.id),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(
        b["balances"],
        json!([{"kind": "tenant_balance", "currency": "BTC", "balance": 2500}])
    );
    let (s, l) = call(
        &h.app,
        "GET",
        &format!("/admin/tenants/{}/ledger", t.id),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let rows = l.as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["amount"], 2500);
    assert_eq!(rows[0]["reference_kind"], "payment");
    assert_eq!(rows[0]["reference_id"], "abc");
    let (s, _) = call(
        &h.app,
        "GET",
        &format!("/admin/tenants/{}/balance", t.id),
        Some("nope"),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, r) = call(
        &h.app,
        "GET",
        "/admin/ledger/reconcile",
        Some(ADMIN),
        Value::Null,
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(r, json!([]));
    let events = payment_gateway::audit::list(
        &h.pool,
        &payment_gateway::audit::Filter {
            action: Some("ledger.reconciled".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].details["mismatches"], 0);
    assert_eq!(events[0].actor_kind, "admin_token");
}

#[tokio::test]
async fn history_pages_newest_first() {
    let h = harness().await;
    let t = tenant(&h).await;
    for i in 0..5 {
        fund(&h, t.id, 10, &format!("p{i}")).await;
    }
    let (_, first) = call(
        &h.app,
        "GET",
        &format!("/admin/tenants/{}/ledger?limit=2", t.id),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    let first = first.as_array().unwrap().clone();
    assert_eq!(first.len(), 2);
    let cursor = first[1]["id"].as_u64().unwrap();
    let (_, next) = call(
        &h.app,
        "GET",
        &format!("/admin/tenants/{}/ledger?limit=10&before={cursor}", t.id),
        Some(ADMIN),
        Value::Null,
    )
    .await;
    let ids: Vec<u64> = first
        .iter()
        .chain(next.as_array().unwrap())
        .map(|r| r["id"].as_u64().unwrap())
        .collect();
    assert_eq!(ids.len(), 5);
    assert!(ids.windows(2).all(|w| w[0] > w[1]));
}

struct Console {
    cookie: String,
    csrf: String,
}

async fn console_login(h: &Harness, email: &str) -> Console {
    let hash = accounts::hash_password("correct horse battery").unwrap();
    accounts::insert_account(&h.pool, email, &hash, false)
        .await
        .unwrap();
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
    let cookie = r.headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .trim_start_matches("pgw_session=")
        .to_string();
    Console {
        cookie,
        csrf: r.json["csrf"].as_str().unwrap().to_string(),
    }
}

async fn console_get(h: &Harness, c: &Console, path: &str) -> (StatusCode, Value) {
    let cookie = format!("pgw_session={}", c.cookie);
    let r = send(
        &h.app,
        "GET",
        &format!("/console/api{path}"),
        None,
        &[("cookie", &cookie), ("x-csrf-token", &c.csrf)],
        Value::Null,
    )
    .await;
    (r.status, r.json)
}

#[tokio::test]
async fn console_balance_is_readable_by_members_only() {
    let h = harness().await;
    let owner = console_login(&h, "owner@example.com").await;
    let stranger = console_login(&h, "stranger@example.com").await;
    let cookie = format!("pgw_session={}", owner.cookie);
    let r = send(
        &h.app,
        "POST",
        "/console/api/projects",
        None,
        &[("cookie", &cookie), ("x-csrf-token", &owner.csrf)],
        json!({"name": "shop"}),
    )
    .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text);
    let pid = r.json["id"].as_u64().unwrap();
    let other = tenant(&h).await;
    fund(&h, pid, 4_000, "mine").await;
    fund(&h, other.id, 9_000, "theirs").await;

    let (s, b) = console_get(&h, &owner, &format!("/projects/{pid}/balance")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(b["balances"][0]["balance"], 4000);
    let (s, l) = console_get(&h, &owner, &format!("/projects/{pid}/ledger")).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(l.as_array().unwrap().len(), 1);

    let (s, _) = console_get(&h, &stranger, &format!("/projects/{pid}/balance")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = console_get(&h, &stranger, &format!("/projects/{pid}/ledger")).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = console_get(&h, &owner, &format!("/projects/{}/balance", other.id)).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
