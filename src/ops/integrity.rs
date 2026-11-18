use anyhow::Result;
use serde::Serialize;
use sqlx::MySqlPool;

use super::rotate::SEALED_COLUMNS;

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Finding {
    pub check: &'static str,
    pub count: i64,
}

const CHECKS: &[(&str, &str)] = &[
    (
        "paid_payment_without_paid_attempt",
        "select count(*) from payment p where p.status in ('paid', 'refunded', 'partially_refunded') and not exists (select 1 from payment_attempt a where a.payment_id = p.id and a.status = 'paid')",
    ),
    (
        "paid_payment_without_paid_at",
        "select count(*) from payment where status in ('paid', 'refunded', 'partially_refunded') and paid_at is null",
    ),
    (
        "refunds_exceed_paid_amount",
        "select count(*) from (select p.id from payment p join refund r on r.payment_id = p.id where r.status <> 'failed' group by p.id, p.amount having sum(r.amount) > p.amount) x",
    ),
    (
        "attempt_without_payment",
        "select count(*) from payment_attempt a left join payment p on p.id = a.payment_id where p.id is null",
    ),
    (
        "attempt_without_provider",
        "select count(*) from payment_attempt a left join provider_config c on c.id = a.provider_config_id where c.id is null",
    ),
    (
        "receipt_without_attempt",
        "select count(*) from attempt_receipt r left join payment_attempt a on a.id = r.attempt_id where a.id is null",
    ),
    (
        "refund_without_payment",
        "select count(*) from refund r left join payment p on p.id = r.payment_id where p.id is null",
    ),
    (
        "refund_without_attempt",
        "select count(*) from refund r left join payment_attempt a on a.id = r.attempt_id where a.id is null",
    ),
    (
        "delivery_without_payment",
        "select count(*) from webhook_delivery d left join payment p on p.id = d.payment_id where p.id is null",
    ),
    (
        "payment_without_tenant",
        "select count(*) from payment p left join tenant t on t.id = p.tenant_id where t.id is null",
    ),
    (
        "provider_config_without_tenant",
        "select count(*) from provider_config c left join tenant t on t.id = c.tenant_id where t.id is null",
    ),
    (
        "saved_method_without_tenant",
        "select count(*) from saved_method s left join tenant t on t.id = s.tenant_id where t.id is null",
    ),
    (
        "session_without_account",
        "select count(*) from account_session s left join account a on a.id = s.account_id where a.id is null",
    ),
];

pub async fn check(pool: &MySqlPool) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    for (name, sql) in CHECKS {
        let count: i64 = sqlx::query_scalar(sql).fetch_one(pool).await?;
        if count != 0 {
            findings.push(Finding { check: name, count });
        }
    }
    let mismatches = crate::ledger::reconcile(pool).await?;
    let cached = mismatches.iter().filter(|m| m.account_id.is_some()).count() as i64;
    let unbalanced = mismatches.len() as i64 - cached;
    if cached != 0 {
        findings.push(Finding {
            check: "ledger_cached_balance_mismatch",
            count: cached,
        });
    }
    if unbalanced != 0 {
        findings.push(Finding {
            check: "ledger_unbalanced_transaction",
            count: unbalanced,
        });
    }
    Ok(findings)
}

pub async fn unreadable_sealed_values(
    pool: &MySqlPool,
    sealer: &crate::crypto::Sealer,
) -> Result<i64> {
    let mut bad = 0;
    for col in SEALED_COLUMNS {
        let rows: Vec<(Vec<u8>,)> = sqlx::query_as(&format!(
            "select {c} from {t} where {c} is not null",
            c = col.column,
            t = col.table
        ))
        .fetch_all(pool)
        .await?;
        bad += rows.iter().filter(|(v,)| sealer.open(v).is_err()).count() as i64;
    }
    Ok(bad)
}
