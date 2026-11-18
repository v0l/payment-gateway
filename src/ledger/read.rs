use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, MySqlPool};

#[derive(Debug, Clone, Serialize, FromRow, PartialEq, Eq)]
pub struct AccountBalance {
    pub kind: String,
    pub currency: String,
    pub balance: i64,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct EntryView {
    pub id: u64,
    pub transaction_id: u64,
    pub account_kind: String,
    pub amount: i64,
    pub currency: String,
    pub transaction_kind: String,
    pub reference_kind: Option<String>,
    pub reference_id: Option<String>,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Mismatch {
    pub account_id: Option<u64>,
    pub transaction_id: Option<u64>,
    pub currency: String,
    pub cached: i64,
    pub computed: i64,
}

pub async fn balances(
    pool: &MySqlPool,
    tenant_id: u64,
) -> Result<Vec<AccountBalance>, sqlx::Error> {
    sqlx::query_as(
        "select kind, currency, balance from ledger_account where tenant_id = ? order by kind, currency",
    )
    .bind(tenant_id)
    .fetch_all(pool)
    .await
}

pub async fn tenant_balance(
    pool: &MySqlPool,
    tenant_id: u64,
    currency: &str,
) -> Result<i64, sqlx::Error> {
    let row: Option<(i64,)> = sqlx::query_as(
        "select balance from ledger_account where tenant_id = ? and kind = 'tenant_balance' and currency = ?",
    )
    .bind(tenant_id)
    .bind(currency)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| r.0).unwrap_or(0))
}

pub async fn history(
    pool: &MySqlPool,
    tenant_id: u64,
    before: Option<u64>,
    limit: u32,
) -> Result<Vec<EntryView>, sqlx::Error> {
    sqlx::query_as(
        "select e.id, e.transaction_id, a.kind as account_kind, e.amount, e.currency, t.kind as transaction_kind, t.reference_kind, t.reference_id, e.created from ledger_entry e join ledger_account a on a.id = e.account_id join ledger_transaction t on t.id = e.transaction_id where a.tenant_id = ? and (? is null or e.id < ?) order by e.id desc limit ?",
    )
    .bind(tenant_id)
    .bind(before)
    .bind(before)
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await
}

pub async fn reconcile(pool: &MySqlPool) -> Result<Vec<Mismatch>, sqlx::Error> {
    reconcile_filtered(pool, None).await
}

pub async fn reconcile_tenant(
    pool: &MySqlPool,
    tenant_id: u64,
) -> Result<Vec<Mismatch>, sqlx::Error> {
    reconcile_filtered(pool, Some(tenant_id)).await
}

async fn reconcile_filtered(
    pool: &MySqlPool,
    tenant_id: Option<u64>,
) -> Result<Vec<Mismatch>, sqlx::Error> {
    let rows: Vec<(u64, String, i64, i64)> = sqlx::query_as(
        "select a.id, a.currency, a.balance, cast(coalesce(sum(e.amount), 0) as signed) from ledger_account a left join ledger_entry e on e.account_id = a.id where (? is null or a.tenant_id = ?) group by a.id, a.currency, a.balance having a.balance <> cast(coalesce(sum(e.amount), 0) as signed) order by a.id",
    )
    .bind(tenant_id)
    .bind(tenant_id)
    .fetch_all(pool)
    .await?;
    let mut out: Vec<Mismatch> = rows
        .into_iter()
        .map(|(id, currency, cached, computed)| Mismatch {
            account_id: Some(id),
            transaction_id: None,
            currency,
            cached,
            computed,
        })
        .collect();
    if tenant_id.is_none() {
        let unbalanced: Vec<(u64, String, i64)> = sqlx::query_as(
            "select transaction_id, currency, cast(sum(amount) as signed) from ledger_entry group by transaction_id, currency having sum(amount) <> 0 order by transaction_id",
        )
        .fetch_all(pool)
        .await?;
        out.extend(unbalanced.into_iter().map(|(t, currency, sum)| Mismatch {
            account_id: None,
            transaction_id: Some(t),
            currency,
            cached: 0,
            computed: sum,
        }));
    }
    Ok(out)
}
