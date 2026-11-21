mod common;
use chrono::{Duration, Utc};
use common::*;
use payment_gateway::audit::Actor;
use payment_gateway::crypto::Sealer;
use payment_gateway::ops::integrity;
use payment_gateway::ops::privacy::{self, Subject};
use payment_gateway::ops::retention::{self, RetentionPolicy};
use payment_gateway::ops::rotate::{
    NOT_SEALED_BINARY_COLUMNS, RotateOptions, SEALED_COLUMNS, rotate_keys, sealed_kids,
};
use payment_gateway::providers::{EventSink, ProviderEvent};
use payment_gateway::service::ProviderSink;
use serde_json::json;
use sqlx::MySqlPool;

fn key(n: u8) -> [u8; 32] {
    [n; 32]
}

fn sealer(kids: &[(&str, u8)]) -> Sealer {
    let keys: Vec<(String, [u8; 32])> =
        kids.iter().map(|(k, n)| (k.to_string(), key(*n))).collect();
    Sealer::with_keys(&keys).unwrap()
}

async fn seed_secrets(h: &Harness, sealer: &Sealer, tenants: usize) {
    for _ in 0..tenants {
        let t = tenant(h).await;
        provider(h, &t, "lightning", json!({"macaroon": "m"}), None).await;
    }
    sqlx::query("update tenant set webhook_secret = ?")
        .bind(sealer.seal(b"whsec").unwrap())
        .execute(&h.pool)
        .await
        .unwrap();
    sqlx::query("update webhook_endpoint set secret = ?")
        .bind(sealer.seal(b"whsec").unwrap())
        .execute(&h.pool)
        .await
        .unwrap();
    sqlx::query("update provider_config set config = ?")
        .bind(sealer.seal(b"{\"macaroon\":\"m\"}").unwrap())
        .execute(&h.pool)
        .await
        .unwrap();
}

async fn seed_totp_secret(h: &Harness, sealer: &Sealer) {
    let id = payment_gateway::accounts::insert_account(&h.pool, "totp@example.com", "hash", false)
        .await
        .unwrap();
    sqlx::query("update account set totp_secret = ?, totp_enabled = 1 where id = ?")
        .bind(sealer.seal(b"totp-secret-bytes").unwrap())
        .bind(id)
        .execute(&h.pool)
        .await
        .unwrap();
}

async fn kid_counts(pool: &MySqlPool) -> Vec<(String, String, u64)> {
    sealed_kids(pool).await.unwrap()
}

#[tokio::test]
async fn rotation_moves_every_sealed_value_to_the_newest_key() {
    let h = harness().await;
    let k1 = sealer(&[("k1", 1)]);
    seed_secrets(&h, &k1, 5).await;
    seed_totp_secret(&h, &k1).await;

    let k2 = sealer(&[("k2", 2), ("k1", 1)]);
    let report = rotate_keys(
        &h.pool,
        &k2,
        &RotateOptions {
            batch: 2,
            max_rows: None,
        },
        Actor::System,
    )
    .await
    .unwrap();
    assert_eq!(report.rotated(), 16);
    assert!(
        kid_counts(&h.pool)
            .await
            .iter()
            .all(|(_, kid, _)| kid == "k2")
    );

    let k3 = sealer(&[("k3", 3), ("k2", 2)]);
    let report = rotate_keys(&h.pool, &k3, &RotateOptions::default(), Actor::System)
        .await
        .unwrap();
    assert_eq!(report.rotated(), 16);
    assert!(
        kid_counts(&h.pool)
            .await
            .iter()
            .all(|(_, kid, _)| kid == "k3")
    );

    let only_k3 = sealer(&[("k3", 3)]);
    let (secret,): (Vec<u8>,) = sqlx::query_as("select webhook_secret from tenant limit 1")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(only_k3.open(&secret).unwrap(), b"whsec");
    let (cfg,): (Vec<u8>,) = sqlx::query_as("select config from provider_config limit 1")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(only_k3.open(&cfg).unwrap(), b"{\"macaroon\":\"m\"}");
    let (totp,): (Vec<u8>,) =
        sqlx::query_as("select totp_secret from account where totp_enabled = 1")
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(only_k3.open(&totp).unwrap(), b"totp-secret-bytes");
    assert_eq!(
        integrity::unreadable_sealed_values(&h.pool, &only_k3)
            .await
            .unwrap(),
        0
    );
    assert!(
        kid_counts(&h.pool)
            .await
            .iter()
            .any(|(col, kid, n)| col == "account.totp_secret" && kid == "k3" && *n == 1)
    );

    let again = rotate_keys(&h.pool, &k3, &RotateOptions::default(), Actor::System)
        .await
        .unwrap();
    assert_eq!(again.rotated(), 0);
}

#[tokio::test]
async fn rotation_resumes_after_a_crash_halfway() {
    let h = harness().await;
    let k1 = sealer(&[("k1", 1)]);
    seed_secrets(&h, &k1, 6).await;
    let k2 = sealer(&[("k2", 2), ("k1", 1)]);

    let first = rotate_keys(
        &h.pool,
        &k2,
        &RotateOptions {
            batch: 2,
            max_rows: Some(4),
        },
        Actor::System,
    )
    .await
    .unwrap();
    assert!(first.stopped_early);
    assert_eq!(first.rotated(), 4);
    let mixed = kid_counts(&h.pool).await;
    assert!(mixed.iter().any(|(_, kid, _)| kid == "k1"));
    assert!(mixed.iter().any(|(_, kid, _)| kid == "k2"));

    let rows: Vec<(Vec<u8>,)> = sqlx::query_as("select webhook_secret from tenant")
        .fetch_all(&h.pool)
        .await
        .unwrap();
    for (v,) in rows {
        assert_eq!(k2.open(&v).unwrap(), b"whsec");
    }

    let second = rotate_keys(&h.pool, &k2, &RotateOptions::default(), Actor::System)
        .await
        .unwrap();
    assert!(!second.stopped_early);
    assert_eq!(second.rotated(), 18 - 4);
    assert!(
        kid_counts(&h.pool)
            .await
            .iter()
            .all(|(_, kid, _)| kid == "k2")
    );
}

#[tokio::test]
async fn rotation_stops_on_a_value_no_key_opens_and_changes_nothing_else() {
    let h = harness().await;
    seed_secrets(&h, &sealer(&[("k1", 1)]), 2).await;
    sqlx::query("update tenant set webhook_secret = ? where id = (select min(id) from (select id from tenant) x)")
        .bind(sealer(&[("lost", 9)]).seal(b"x").unwrap())
        .execute(&h.pool)
        .await
        .unwrap();
    let k2 = sealer(&[("k2", 2), ("k1", 1)]);
    assert!(
        rotate_keys(&h.pool, &k2, &RotateOptions::default(), Actor::System)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn legacy_unprefixed_values_are_rotated() {
    let h = harness().await;
    let t = tenant(&h).await;
    let _ = t;
    use aes_gcm::aead::{Aead, KeyInit};
    let cipher = aes_gcm::Aes256Gcm::new((&key(1)).into());
    let nonce = [7u8; 12];
    let mut legacy = nonce.to_vec();
    legacy.extend(
        cipher
            .encrypt(aes_gcm::Nonce::from_slice(&nonce), &b"whsec"[..])
            .unwrap(),
    );
    sqlx::query("update tenant set webhook_secret = ?")
        .bind(&legacy)
        .execute(&h.pool)
        .await
        .unwrap();
    sqlx::query("update webhook_endpoint set secret = ?")
        .bind(&legacy)
        .execute(&h.pool)
        .await
        .unwrap();
    let k2 = sealer(&[("k2", 2), ("default", 1)]);
    let r = rotate_keys(&h.pool, &k2, &RotateOptions::default(), Actor::System)
        .await
        .unwrap();
    assert_eq!(r.rotated(), 2);
    let (v,): (Vec<u8>,) = sqlx::query_as("select webhook_secret from tenant")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(Sealer::kid_of(&v), Some("k2"));
}

#[tokio::test]
async fn every_binary_column_is_registered_as_sealed_or_known_plain() {
    let h = harness().await;
    let cols: Vec<(String, String)> = sqlx::query_as(
        "select table_name, column_name from information_schema.columns where table_schema = database() and data_type in ('varbinary', 'blob', 'mediumblob', 'longblob', 'tinyblob') and table_name <> '_sqlx_migrations'",
    )
    .fetch_all(&h.pool)
    .await
    .unwrap();
    assert!(!cols.is_empty());
    for (table, column) in cols {
        let sealed = SEALED_COLUMNS
            .iter()
            .any(|c| c.table == table && c.column == column);
        let plain = NOT_SEALED_BINARY_COLUMNS
            .iter()
            .any(|(t, c)| *t == table && *c == column);
        assert!(
            sealed || plain,
            "{table}.{column} is binary: add it to SEALED_COLUMNS or NOT_SEALED_BINARY_COLUMNS in src/ops/rotate.rs"
        );
    }
}

struct Seeded {
    paid: String,
    unpaid: String,
    open: String,
    refunded: String,
}

async fn seed_payments(h: &Harness, t: &TenantFixture, ln: u64) -> Seeded {
    let mut ids = vec![];
    for (i, who) in [
        "a@example.com",
        "a@example.com",
        "b@example.com",
        "c@example.com",
    ]
    .iter()
    .enumerate()
    {
        let mut body = eur(&format!("r{i}"), 1000 + i as u64);
        body["customer_email"] = json!(who);
        body["customer_ref"] = json!(format!("cust{}", who.chars().next().unwrap()));
        body["metadata"] = json!({"name": format!("name{i}")});
        let p = new_payment(h, t, body).await;
        let id = p["id"].as_str().unwrap().to_string();
        let (_, r) = attempt(h, t, &id, json!({"method": "lightning"})).await;
        let hash = r["attempts"][0]["details"]["payment_hash"]
            .as_str()
            .unwrap()
            .to_string();
        ids.push((id, hash));
    }
    let sink = ProviderSink {
        service: h.service.clone(),
        provider_id: ln,
    };
    for i in [0usize, 3] {
        let hash = ids[i].1.clone();
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
    }
    let (s, r) = call(
        &h.app,
        "POST",
        &format!("/v1/payments/{}/refund", ids[3].0),
        Some(&t.key),
        json!({"destination": "lnbc1refund"}),
    )
    .await;
    assert_eq!(s, axum::http::StatusCode::OK, "{r}");
    sqlx::query("update payment set expires = ? where id = ? or id = ?")
        .bind(Utc::now() - Duration::days(40))
        .bind(&pid(&ids[1].0)[..])
        .bind(&pid(&ids[2].0)[..])
        .execute(&h.pool)
        .await
        .unwrap();
    h.service.expire_due().await.unwrap();
    sqlx::query("update payment set expires = ? where id = ?")
        .bind(Utc::now() + Duration::days(1))
        .bind(&pid(&ids[2].0)[..])
        .execute(&h.pool)
        .await
        .unwrap();
    sqlx::query("update payment set status = 'pending' where id = ?")
        .bind(&pid(&ids[2].0)[..])
        .execute(&h.pool)
        .await
        .unwrap();
    h.service.deliver_due().await.unwrap();
    Seeded {
        paid: ids[0].0.clone(),
        unpaid: ids[1].0.clone(),
        open: ids[2].0.clone(),
        refunded: ids[3].0.clone(),
    }
}

async fn count(pool: &MySqlPool, sql: &str) -> i64 {
    sqlx::query_scalar(sql).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn retention_purges_old_unpaid_but_never_paid_refunded_or_open() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let s = seed_payments(&h, &t, ln).await;
    sqlx::query("update webhook_delivery set created = ?")
        .bind(Utc::now() - Duration::days(100))
        .execute(&h.pool)
        .await
        .unwrap();
    let payments_before = count(&h.pool, "select count(*) from payment").await;
    let refunds_before = count(&h.pool, "select count(*) from refund").await;
    let paid_attempts = count(
        &h.pool,
        "select count(*) from payment_attempt where status = 'paid'",
    )
    .await;

    let policy = RetentionPolicy {
        delivery_body_days: Some(30),
        unpaid_payment_days: Some(30),
        session_days: Some(7),
    };
    let report = retention::purge(&h.pool, &policy, Utc::now(), Actor::System)
        .await
        .unwrap();
    assert_eq!(report.unpaid_payments_deleted, 1);
    assert!(report.delivery_bodies_cleared >= 2);

    assert_eq!(
        count(&h.pool, "select count(*) from payment").await,
        payments_before - 1
    );
    assert_eq!(
        count(&h.pool, "select count(*) from refund").await,
        refunds_before
    );
    assert_eq!(
        count(
            &h.pool,
            "select count(*) from payment_attempt where status = 'paid'"
        )
        .await,
        paid_attempts
    );
    for kept in [&s.paid, &s.open, &s.refunded] {
        let n: i64 = sqlx::query_scalar("select count(*) from payment where id = ?")
            .bind(&pid(kept)[..])
            .fetch_one(&h.pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }
    let gone: i64 = sqlx::query_scalar("select count(*) from payment where id = ?")
        .bind(&pid(&s.unpaid)[..])
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(gone, 0);
    assert_eq!(
        count(&h.pool, "select count(*) from webhook_delivery where body <> '' and (delivered_at is not null or abandoned_at is not null)").await,
        0
    );
    assert!(integrity::check(&h.pool).await.unwrap().is_empty());

    let again = retention::purge(&h.pool, &policy, Utc::now(), Actor::System)
        .await
        .unwrap();
    assert_eq!(again, retention::PurgeReport::default());
}

#[tokio::test]
async fn retention_keeps_recent_rows_and_undelivered_bodies() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let _ = seed_payments(&h, &t, ln).await;
    sqlx::query("update webhook_delivery set delivered_at = null")
        .execute(&h.pool)
        .await
        .unwrap();
    sqlx::query("update webhook_delivery set created = ?")
        .bind(Utc::now() - Duration::days(100))
        .execute(&h.pool)
        .await
        .unwrap();
    let policy = RetentionPolicy {
        delivery_body_days: Some(30),
        unpaid_payment_days: Some(3650),
        session_days: None,
    };
    let report = retention::purge(&h.pool, &policy, Utc::now(), Actor::System)
        .await
        .unwrap();
    assert_eq!(report, retention::PurgeReport::default());
}

#[tokio::test]
async fn retention_deletes_only_long_expired_sessions() {
    let h = harness().await;
    let id = payment_gateway::accounts::insert_account(&h.pool, "x@example.com", "hash", false)
        .await
        .unwrap();
    for (token, expires) in [("old", -40), ("recent", -1), ("live", 5)] {
        sqlx::query("insert into account_session (account_id, token_hash, csrf, created, expires) values (?, ?, 'c', ?, ?)")
            .bind(id)
            .bind(token)
            .bind(Utc::now() - Duration::days(50))
            .bind(Utc::now() + Duration::days(expires))
            .execute(&h.pool)
            .await
            .unwrap();
    }
    let policy = RetentionPolicy {
        session_days: Some(7),
        ..Default::default()
    };
    let r = retention::purge(&h.pool, &policy, Utc::now(), Actor::System)
        .await
        .unwrap();
    assert_eq!(r.sessions_deleted, 1);
    assert_eq!(
        count(&h.pool, "select count(*) from account_session").await,
        2
    );
}

#[tokio::test]
async fn privacy_export_then_delete_anonymises_financial_records() {
    let h = harness().await;
    let t = tenant(&h).await;
    let other = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    provider(&h, &other, "lightning", json!({}), None).await;
    let s = seed_payments(&h, &t, ln).await;
    let mut theirs = eur("other", 500);
    theirs["customer_email"] = json!("a@example.com");
    theirs["metadata"] = json!({"keep": true});
    let theirs = new_payment(&h, &other, theirs).await;

    let subject = Subject::Email("a@example.com".into());
    let export = privacy::export(&h.pool, t.id, &subject, Actor::System)
        .await
        .unwrap();
    assert_eq!(export.payments.len(), 2);
    assert!(
        export
            .payments
            .iter()
            .all(|p| p.customer_email.as_deref() == Some("a@example.com"))
    );
    assert!(
        export
            .payments
            .iter()
            .any(|p| p.metadata.as_deref().unwrap().contains("name0"))
    );

    let before_paid = get_payment(&h, &t, &s.paid).await;
    let report = privacy::delete(&h.pool, t.id, &subject, Actor::System)
        .await
        .unwrap();
    assert_eq!(report.payments_anonymised, 2);
    assert!(report.delivery_bodies_cleared >= 1);

    let after_paid = get_payment(&h, &t, &s.paid).await;
    assert_eq!(after_paid["status"], "paid");
    assert_eq!(after_paid["amount"], before_paid["amount"]);
    assert_eq!(after_paid["attempts"], before_paid["attempts"]);
    let row: (Option<String>, Option<String>, Option<String>) =
        sqlx::query_as("select customer_email, customer_ref, metadata from payment where id = ?")
            .bind(&pid(&s.paid)[..])
            .fetch_one(&h.pool)
            .await
            .unwrap();
    assert_eq!(row, (None, None, None));

    let leaked: i64 = count(
        &h.pool,
        "select count(*) from webhook_delivery d join payment p on p.id = d.payment_id where d.body like '%a@example.com%' and p.tenant_id = (select min(id) from tenant)",
    )
    .await;
    assert_eq!(leaked, 0);

    let untouched = privacy::export(&h.pool, other.id, &subject, Actor::System)
        .await
        .unwrap();
    assert_eq!(untouched.payments.len(), 1);
    assert_eq!(untouched.payments[0].id, theirs["id"].as_str().unwrap());
    let b = privacy::export(
        &h.pool,
        t.id,
        &Subject::Email("b@example.com".into()),
        Actor::System,
    )
    .await
    .unwrap();
    assert_eq!(b.payments.len(), 1);
    assert!(
        privacy::export(&h.pool, t.id, &subject, Actor::System)
            .await
            .unwrap()
            .payments
            .is_empty()
    );
    assert!(integrity::check(&h.pool).await.unwrap().is_empty());
}

#[tokio::test]
async fn privacy_delete_by_customer_ref_removes_saved_methods_in_that_project_only() {
    let h = harness().await;
    let t = tenant(&h).await;
    let other = tenant(&h).await;
    let pa = provider(&h, &t, "lightning", json!({}), None).await;
    let pb = provider(&h, &other, "lightning", json!({}), None).await;
    for (tenant_id, pc) in [(t.id, pa), (other.id, pb)] {
        sqlx::query("insert into saved_method (tenant_id, customer_ref, provider_config_id, external_customer_id, external_id, created) values (?, 'cust1', ?, 'cus', 'pm', ?)")
            .bind(tenant_id)
            .bind(pc)
            .bind(Utc::now())
            .execute(&h.pool)
            .await
            .unwrap();
    }
    let subject = Subject::CustomerRef("cust1".into());
    assert_eq!(
        privacy::export(&h.pool, t.id, &subject, Actor::System)
            .await
            .unwrap()
            .saved_methods
            .len(),
        1
    );
    let r = privacy::delete(&h.pool, t.id, &subject, Actor::System)
        .await
        .unwrap();
    assert_eq!(r.saved_methods_deleted, 1);
    assert_eq!(
        privacy::export(&h.pool, other.id, &subject, Actor::System)
            .await
            .unwrap()
            .saved_methods
            .len(),
        1
    );
}

#[tokio::test]
async fn privacy_delete_refuses_an_empty_subject() {
    let h = harness().await;
    let t = tenant(&h).await;
    assert!(
        privacy::delete(&h.pool, t.id, &Subject::Email(" ".into()), Actor::System)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn integrity_check_flags_orphans_and_unreadable_secrets() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let s = seed_payments(&h, &t, ln).await;
    assert!(integrity::check(&h.pool).await.unwrap().is_empty());

    sqlx::query("update payment_attempt set status = 'expired' where payment_id = ?")
        .bind(&pid(&s.paid)[..])
        .execute(&h.pool)
        .await
        .unwrap();
    let findings = integrity::check(&h.pool).await.unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.check == "paid_payment_without_paid_attempt")
    );

    let wrong = sealer(&[("other", 5)]);
    assert!(
        integrity::unreadable_sealed_values(&h.pool, &wrong)
            .await
            .unwrap()
            > 0
    );
}

fn db_parts(url: &str) -> (String, String, String, String) {
    let u = url::Url::parse(url).unwrap();
    (
        u.host_str().unwrap().to_string(),
        u.port().unwrap_or(3306).to_string(),
        u.username().to_string(),
        u.password().unwrap_or("").to_string(),
    )
}

fn have(bin: &str) -> bool {
    std::process::Command::new("sh")
        .args(["-c", &format!("command -v {bin}")])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[tokio::test]
async fn restore_drill_backs_up_restores_into_a_scratch_database_and_passes_integrity() {
    if !(have("mariadb-dump") || have("mysqldump")) || !(have("mariadb") || have("mysql")) {
        assert!(
            std::env::var_os("CI").is_none(),
            "restore drill needs mysql client tools in CI"
        );
        eprintln!("skipping restore drill: no mysql client tools");
        return;
    }
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    let s = seed_payments(&h, &t, ln).await;
    let sealer_k = sealer(&[("k1", 1)]);
    seed_secrets(&h, &sealer_k, 1).await;
    seed_ledger(&h, t.id).await;

    let base = std::env::var("TEST_DATABASE_URL").unwrap();
    let (host, port, user, pass) = db_parts(&base);
    let source_db: String = sqlx::query_scalar("select database()")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    let dir = std::env::temp_dir().join(unique("pgwbackup"));
    let root = env!("CARGO_MANIFEST_DIR");

    let out = std::process::Command::new(format!("{root}/scripts/backup.sh"))
        .env("DB_NAME", &source_db)
        .env("DB_HOST", &host)
        .env("DB_PORT", &port)
        .env("DB_USER", &user)
        .env("DB_PASSWORD", &pass)
        .env("BACKUP_DIR", &dir)
        .env("BACKUP_KEEP", "3")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let dump = String::from_utf8(out.stdout).unwrap().trim().to_string();
    assert!(std::path::Path::new(&dump).exists());

    let scratch = unique("restore_");
    let run_restore = |overwrite: bool| {
        let mut c = std::process::Command::new(format!("{root}/scripts/restore.sh"));
        c.arg(&dump)
            .arg(&scratch)
            .env("DB_HOST", &host)
            .env("DB_PORT", &port)
            .env("DB_USER", &user)
            .env("DB_PASSWORD", &pass)
            .env("GATEWAY_BIN", env!("CARGO_BIN_EXE_payment-gateway"))
            .env(
                "GATEWAY_ENCRYPTION_KEYS",
                format!("k1:{}", hex::encode(key(1))),
            );
        if overwrite {
            c.env("RESTORE_OVERWRITE", "1");
        }
        c.output().unwrap()
    };
    let out = run_restore(false);
    assert!(
        out.status.success(),
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!run_restore(false).status.success());
    assert!(run_restore(true).status.success());

    let (prefix, _) = base.rsplit_once('/').unwrap();
    let restored = MySqlPool::connect(&format!("{prefix}/{scratch}"))
        .await
        .unwrap();
    for table in [
        "tenant",
        "provider_config",
        "payment",
        "payment_attempt",
        "refund",
        "webhook_delivery",
        "ledger_account",
        "ledger_transaction",
        "ledger_entry",
    ] {
        let sql = format!("select count(*) from {table}");
        assert_eq!(
            count(&restored, &sql).await,
            count(&h.pool, &sql).await,
            "{table}"
        );
    }
    let id: i64 =
        sqlx::query_scalar("select count(*) from payment where id = ? and status = 'paid'")
            .bind(&pid(&s.paid)[..])
            .fetch_one(&restored)
            .await
            .unwrap();
    assert_eq!(id, 1);
    assert!(integrity::check(&restored).await.unwrap().is_empty());
    assert_eq!(
        payment_gateway::ledger::tenant_balance(&restored, t.id, "BTC")
            .await
            .unwrap(),
        5_000
    );
    assert_eq!(
        integrity::unreadable_sealed_values(&restored, &sealer_k)
            .await
            .unwrap(),
        0
    );
    let _ = std::fs::remove_dir_all(&dir);
    sqlx::query(&format!("drop database {scratch}"))
        .execute(&h.pool)
        .await
        .unwrap();
}

async fn audit_rows(pool: &MySqlPool, action: &str) -> Vec<(String, Option<u64>, Option<String>)> {
    sqlx::query_as(
        "select actor_kind, tenant_id, details from audit_event where action = ? order by id",
    )
    .bind(action)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn operator_tools_write_audit_events_without_subject_values() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    seed_payments(&h, &t, ln).await;
    seed_secrets(&h, &sealer(&[("k1", 1)]), 0).await;

    let k2 = sealer(&[("k2", 2), ("k1", 1)]);
    rotate_keys(&h.pool, &k2, &RotateOptions::default(), Actor::System)
        .await
        .unwrap();
    let rows = audit_rows(&h.pool, "keys.rotated").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "system");
    let d: serde_json::Value = serde_json::from_str(rows[0].2.as_deref().unwrap()).unwrap();
    assert_eq!(d["kid"], "k2");

    sqlx::query("update payment set expires = ?, status = 'expired' where status = 'pending'")
        .bind(Utc::now() - Duration::days(90))
        .execute(&h.pool)
        .await
        .unwrap();
    let policy = RetentionPolicy {
        unpaid_payment_days: Some(30),
        ..Default::default()
    };
    let r = retention::purge(&h.pool, &policy, Utc::now(), Actor::AdminToken)
        .await
        .unwrap();
    let rows = audit_rows(&h.pool, "retention.purged").await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].0, "admin_token");
    let d: serde_json::Value = serde_json::from_str(rows[0].2.as_deref().unwrap()).unwrap();
    assert_eq!(d["unpaid_payments_deleted"], r.unpaid_payments_deleted);

    let subject = Subject::Email("c@example.com".into());
    privacy::export(&h.pool, t.id, &subject, Actor::System)
        .await
        .unwrap();
    privacy::delete(&h.pool, t.id, &subject, Actor::System)
        .await
        .unwrap();
    for action in ["privacy.exported", "privacy.deleted"] {
        let rows = audit_rows(&h.pool, action).await;
        assert_eq!(rows.len(), 1, "{action}");
        assert_eq!(rows[0].1, Some(t.id));
        let details = rows[0].2.as_deref().unwrap();
        assert!(details.contains("\"subject_kind\":\"email\""));
        assert!(!details.contains("c@example.com"));
    }
}

async fn seed_ledger(h: &Harness, tenant_id: u64) {
    use payment_gateway::ledger::{AccountKind, AccountRef, Entry, Posting, TxKind, post};
    let mut tx = h.pool.begin().await.unwrap();
    let p = Posting::new(
        TxKind::PaymentReceived,
        vec![
            Entry::new(
                AccountRef::platform(AccountKind::PlatformWallet, "BTC"),
                -5_000,
            ),
            Entry::new(
                AccountRef::tenant(tenant_id, AccountKind::TenantBalance, "BTC"),
                5_000,
            ),
        ],
    );
    post(&mut tx, &p, "seed-ledger").await.unwrap();
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn retention_and_privacy_never_touch_ledger_tables() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    seed_payments(&h, &t, ln).await;
    seed_ledger(&h, t.id).await;
    sqlx::query("update ledger_transaction set created = ?")
        .bind(Utc::now() - Duration::days(4000))
        .execute(&h.pool)
        .await
        .unwrap();
    let tables = ["ledger_account", "ledger_transaction", "ledger_entry"];
    let mut before = Vec::new();
    for table in tables {
        before.push(count(&h.pool, &format!("select count(*) from {table}")).await);
    }
    let policy = RetentionPolicy {
        delivery_body_days: Some(0),
        unpaid_payment_days: Some(0),
        session_days: Some(0),
    };
    retention::purge(
        &h.pool,
        &policy,
        Utc::now() + Duration::days(5000),
        Actor::System,
    )
    .await
    .unwrap();
    let mut after = Vec::new();
    for table in tables {
        after.push(count(&h.pool, &format!("select count(*) from {table}")).await);
    }
    assert_eq!(before, after);
    assert!(before.iter().all(|n| *n > 0));
}

#[tokio::test]
async fn integrity_reports_a_tampered_ledger_cache_and_an_unbalanced_transaction() {
    let h = harness().await;
    let t = tenant(&h).await;
    seed_ledger(&h, t.id).await;
    assert!(integrity::check(&h.pool).await.unwrap().is_empty());

    sqlx::query("update ledger_account set balance = balance + 1 where tenant_id = ?")
        .bind(t.id)
        .execute(&h.pool)
        .await
        .unwrap();
    let findings = integrity::check(&h.pool).await.unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.check == "ledger_cached_balance_mismatch" && f.count == 1)
    );
    assert!(
        !findings
            .iter()
            .any(|f| f.check == "ledger_unbalanced_transaction")
    );

    sqlx::query("update ledger_account set balance = balance - 1 where tenant_id = ?")
        .bind(t.id)
        .execute(&h.pool)
        .await
        .unwrap();
    sqlx::query("insert into ledger_transaction (kind, idempotency_key, created) values ('adjustment', 'hand', ?)")
        .bind(Utc::now())
        .execute(&h.pool)
        .await
        .unwrap();
    sqlx::query("insert into ledger_entry (transaction_id, account_id, amount, currency, created) select (select id from ledger_transaction where idempotency_key = 'hand'), id, 7, currency, ? from ledger_account where tenant_id = ?")
        .bind(Utc::now())
        .bind(t.id)
        .execute(&h.pool)
        .await
        .unwrap();
    let findings = integrity::check(&h.pool).await.unwrap();
    assert!(
        findings
            .iter()
            .any(|f| f.check == "ledger_unbalanced_transaction" && f.count == 1)
    );
}

#[tokio::test]
async fn retention_never_touches_audit_events() {
    let h = harness().await;
    let t = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    seed_payments(&h, &t, ln).await;
    sqlx::query("insert into audit_event (tenant_id, actor_kind, action, details, created) values (?, 'system', 'login.ok', '{}', ?)")
        .bind(t.id)
        .bind(Utc::now() - Duration::days(4000))
        .execute(&h.pool)
        .await
        .unwrap();
    let before = count(&h.pool, "select count(*) from audit_event").await;
    let policy = RetentionPolicy {
        delivery_body_days: Some(0),
        unpaid_payment_days: Some(0),
        session_days: Some(0),
    };
    retention::purge(
        &h.pool,
        &policy,
        Utc::now() + Duration::days(5000),
        Actor::System,
    )
    .await
    .unwrap();
    let after = count(
        &h.pool,
        "select count(*) from audit_event where action <> 'retention.purged'",
    )
    .await;
    assert_eq!(after, before);
}

#[tokio::test]
async fn privacy_delete_keeps_audit_rows_but_strips_customer_pii_from_details() {
    let h = harness().await;
    let t = tenant(&h).await;
    let other = tenant(&h).await;
    let ln = provider(&h, &t, "lightning", json!({}), None).await;
    seed_payments(&h, &t, ln).await;
    for (tenant_id, details) in [
        (
            t.id,
            r#"{"note":"refund for a@example.com","customer_ref":"custa"}"#,
        ),
        (t.id, r#"{"unrelated":true}"#),
        (other.id, r#"{"note":"a@example.com"}"#),
    ] {
        sqlx::query("insert into audit_event (tenant_id, actor_kind, action, details, created) values (?, 'system', 'refund.requested', ?, ?)")
            .bind(tenant_id)
            .bind(details)
            .bind(Utc::now())
            .execute(&h.pool)
            .await
            .unwrap();
    }
    let before = count(&h.pool, "select count(*) from audit_event").await;
    let report = privacy::delete(
        &h.pool,
        t.id,
        &Subject::Email("a@example.com".into()),
        Actor::System,
    )
    .await
    .unwrap();
    assert_eq!(report.audit_events_scrubbed, 1);
    assert_eq!(
        count(&h.pool, "select count(*) from audit_event").await,
        before + 1
    );
    let mine: Vec<(Option<String>,)> = sqlx::query_as("select details from audit_event where tenant_id = ? and action = 'refund.requested' order by id")
        .bind(t.id)
        .fetch_all(&h.pool)
        .await
        .unwrap();
    assert!(!mine[0].0.as_deref().unwrap().contains("a@example.com"));
    assert!(!mine[0].0.as_deref().unwrap().contains("custa"));
    assert_eq!(mine[1].0.as_deref(), Some(r#"{"unrelated":true}"#));
    let theirs: Option<String> = sqlx::query_scalar(
        "select details from audit_event where tenant_id = ? and action = 'refund.requested'",
    )
    .bind(other.id)
    .fetch_one(&h.pool)
    .await
    .unwrap();
    assert!(theirs.unwrap().contains("a@example.com"));
}

#[tokio::test]
async fn retention_purges_dead_account_tokens_and_old_throttle_rows_but_keeps_live_ones() {
    let h = harness().await;
    let t = tenant(&h).await;
    let id = payment_gateway::accounts::insert_account(&h.pool, "tok@example.com", "hash", false)
        .await
        .unwrap();
    let now = Utc::now();
    let rows = [
        ("expired", -40i64, None),
        ("used_old", 5, Some(-40i64)),
        ("used_recent", 5, Some(-1)),
        ("live", 5, None),
        ("expired_recently", -1, None),
    ];
    for (hash, expires, used) in rows {
        sqlx::query("insert into account_token (account_id, kind, token_hash, created, expires, used_at) values (?, 'reset', ?, ?, ?, ?)")
            .bind(id)
            .bind(hash)
            .bind(now - Duration::days(60))
            .bind(now + Duration::days(expires))
            .bind(used.map(|d| now + Duration::days(d)))
            .execute(&h.pool)
            .await
            .unwrap();
    }
    for (kind, age) in [("old", 40), ("fresh", 1)] {
        sqlx::query(
            "insert into notification_throttle (tenant_id, kind, sent_at) values (?, ?, ?)",
        )
        .bind(t.id)
        .bind(kind)
        .bind(now - Duration::days(age))
        .execute(&h.pool)
        .await
        .unwrap();
    }
    let policy = RetentionPolicy {
        session_days: Some(7),
        ..Default::default()
    };
    let r = retention::purge(&h.pool, &policy, now, Actor::System)
        .await
        .unwrap();
    assert_eq!(r.account_tokens_deleted, 2);
    assert_eq!(r.throttle_rows_deleted, 1);
    let left: Vec<String> =
        sqlx::query_scalar("select token_hash from account_token order by token_hash")
            .fetch_all(&h.pool)
            .await
            .unwrap();
    assert_eq!(left, vec!["expired_recently", "live", "used_recent"]);
    assert_eq!(
        count(
            &h.pool,
            "select count(*) from notification_throttle where kind = 'fresh'"
        )
        .await,
        1
    );
}

#[tokio::test]
async fn retention_purges_expired_login_challenges_but_keeps_live_ones() {
    let h = harness().await;
    let id = payment_gateway::accounts::insert_account(&h.pool, "c@example.com", "hash", false)
        .await
        .unwrap();
    let now = Utc::now();
    for (hash, expires) in [("old", -40i64), ("recent", -1), ("live", 5)] {
        sqlx::query("insert into login_challenge (account_id, token_hash, created, expires) values (?, ?, ?, ?)")
            .bind(id)
            .bind(hash)
            .bind(now - Duration::days(50))
            .bind(now + Duration::days(expires))
            .execute(&h.pool)
            .await
            .unwrap();
    }
    let policy = RetentionPolicy {
        session_days: Some(7),
        ..Default::default()
    };
    let r = retention::purge(&h.pool, &policy, now, Actor::System)
        .await
        .unwrap();
    assert_eq!(r.login_challenges_deleted, 1);
    let left: Vec<String> =
        sqlx::query_scalar("select token_hash from login_challenge order by token_hash")
            .fetch_all(&h.pool)
            .await
            .unwrap();
    assert_eq!(left, vec!["live", "recent"]);
}

#[tokio::test]
async fn shared_provider_configs_are_rotated_with_the_other_sealed_columns() {
    let h = harness().await;
    assert!(
        SEALED_COLUMNS
            .iter()
            .any(|c| c.table == "shared_provider" && c.column == "config")
    );
    let k1 = sealer(&[("k1", 1)]);
    sqlx::query(
        "insert into shared_provider (name, method, provider_type, network, config) values ('s', 'lightning', 'mock', 'regtest', ?)",
    )
    .bind(k1.seal(b"{\"macaroon\":\"m\"}").unwrap())
    .execute(&h.pool)
    .await
    .unwrap();
    let k2 = sealer(&[("k2", 2), ("k1", 1)]);
    let report = rotate_keys(&h.pool, &k2, &RotateOptions::default(), Actor::System)
        .await
        .unwrap();
    assert_eq!(report.rotated(), 1);
    let (v,): (Vec<u8>,) = sqlx::query_as("select config from shared_provider")
        .fetch_one(&h.pool)
        .await
        .unwrap();
    assert_eq!(Sealer::kid_of(&v), Some("k2"));
}
