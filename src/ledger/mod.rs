mod read;

use chrono::Utc;
use sqlx::{MySql, Transaction};
use std::collections::BTreeMap;

pub use read::{
    AccountBalance, EntryView, Mismatch, StatementLine, balances, history, reconcile,
    reconcile_tenant, statement, tenant_balance,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AccountKind {
    TenantBalance,
    PlatformFees,
    PlatformWallet,
    PendingPayout,
    RefundReserve,
}

impl AccountKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TenantBalance => "tenant_balance",
            Self::PlatformFees => "platform_fees",
            Self::PlatformWallet => "platform_wallet",
            Self::PendingPayout => "pending_payout",
            Self::RefundReserve => "refund_reserve",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "tenant_balance" => Self::TenantBalance,
            "platform_fees" => Self::PlatformFees,
            "platform_wallet" => Self::PlatformWallet,
            "pending_payout" => Self::PendingPayout,
            "refund_reserve" => Self::RefundReserve,
            _ => return None,
        })
    }

    fn allows_negative_by_default(&self) -> bool {
        matches!(self, Self::PlatformWallet)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxKind {
    PaymentReceived,
    Fee,
    PayoutRequested,
    PayoutCompleted,
    PayoutCancelled,
    Refund,
    Adjustment,
}

impl TxKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PaymentReceived => "payment_received",
            Self::Fee => "fee",
            Self::PayoutRequested => "payout_requested",
            Self::PayoutCompleted => "payout_completed",
            Self::PayoutCancelled => "payout_cancelled",
            Self::Refund => "refund",
            Self::Adjustment => "adjustment",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AccountRef {
    pub tenant_id: Option<u64>,
    pub kind: AccountKind,
    pub currency: String,
}

impl AccountRef {
    pub fn tenant(tenant_id: u64, kind: AccountKind, currency: &str) -> Self {
        Self {
            tenant_id: Some(tenant_id),
            kind,
            currency: currency.to_string(),
        }
    }

    pub fn platform(kind: AccountKind, currency: &str) -> Self {
        Self {
            tenant_id: None,
            kind,
            currency: currency.to_string(),
        }
    }

    fn scope(&self) -> u64 {
        self.tenant_id.unwrap_or(0)
    }

    fn sort_key(&self) -> (u64, &'static str, &str) {
        (self.scope(), self.kind.as_str(), &self.currency)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub account: AccountRef,
    pub amount: i64,
}

impl Entry {
    pub fn new(account: AccountRef, amount: i64) -> Self {
        Self { account, amount }
    }
}

#[derive(Debug, Clone)]
pub struct Posting {
    pub kind: TxKind,
    pub reference: Option<(String, String)>,
    pub entries: Vec<Entry>,
}

impl Posting {
    pub fn new(kind: TxKind, entries: Vec<Entry>) -> Self {
        Self {
            kind,
            reference: None,
            entries,
        }
    }

    pub fn reference(mut self, kind: &str, id: impl ToString) -> Self {
        self.reference = Some((kind.to_string(), id.to_string()));
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Posted {
    pub transaction_id: u64,
    pub replayed: bool,
}

#[derive(Debug)]
pub enum LedgerError {
    Empty,
    ZeroAmount,
    Unbalanced { currency: String, sum: i128 },
    BadAccount(String),
    BadKey,
    Overdraw { account: AccountRef, balance: i64 },
    Overflow,
    Db(sqlx::Error),
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "a ledger transaction needs at least two entries"),
            Self::ZeroAmount => write!(f, "ledger entries cannot be zero"),
            Self::Unbalanced { currency, sum } => {
                write!(f, "entries in {currency} sum to {sum}, not zero")
            }
            Self::BadAccount(m) => write!(f, "bad ledger account: {m}"),
            Self::BadKey => write!(f, "idempotency key must be 1 to 191 characters"),
            Self::Overdraw { account, balance } => write!(
                f,
                "{} {} would be overdrawn (balance {balance})",
                account.kind.as_str(),
                account.currency
            ),
            Self::Overflow => write!(f, "ledger amount overflow"),
            Self::Db(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LedgerError {}

impl From<sqlx::Error> for LedgerError {
    fn from(e: sqlx::Error) -> Self {
        Self::Db(e)
    }
}

impl From<LedgerError> for crate::service::Error {
    fn from(e: LedgerError) -> Self {
        match e {
            LedgerError::Overdraw { .. } => Self::Conflict(e.to_string()),
            LedgerError::Db(e) => e.into(),
            e => Self::Invalid(e.to_string()),
        }
    }
}

pub fn validate(entries: &[Entry]) -> Result<(), LedgerError> {
    if entries.len() < 2 {
        return Err(LedgerError::Empty);
    }
    let mut sums: BTreeMap<&str, i128> = BTreeMap::new();
    for e in entries {
        if e.amount == 0 {
            return Err(LedgerError::ZeroAmount);
        }
        match (e.account.kind, e.account.tenant_id) {
            (AccountKind::TenantBalance, None) => {
                return Err(LedgerError::BadAccount(
                    "tenant_balance needs a tenant".into(),
                ));
            }
            (AccountKind::PlatformFees | AccountKind::PlatformWallet, Some(_)) => {
                return Err(LedgerError::BadAccount(format!(
                    "{} is a platform account",
                    e.account.kind.as_str()
                )));
            }
            _ => {}
        }
        if e.account.currency.is_empty() || e.account.currency.len() > 8 {
            return Err(LedgerError::BadAccount("currency".into()));
        }
        *sums.entry(e.account.currency.as_str()).or_default() += e.amount as i128;
    }
    if let Some((currency, sum)) = sums.into_iter().find(|(_, s)| *s != 0) {
        return Err(LedgerError::Unbalanced {
            currency: currency.to_string(),
            sum,
        });
    }
    Ok(())
}

pub async fn ensure_account(
    tx: &mut Transaction<'_, MySql>,
    account: &AccountRef,
    allow_negative: Option<bool>,
) -> Result<(u64, bool), LedgerError> {
    let allow = allow_negative.unwrap_or(account.kind.allows_negative_by_default());
    sqlx::query(
        "insert into ledger_account (tenant_id, kind, currency, balance, allow_negative, created) values (?, ?, ?, 0, ?, ?) on duplicate key update id = id",
    )
    .bind(account.tenant_id)
    .bind(account.kind.as_str())
    .bind(&account.currency)
    .bind(allow)
    .bind(Utc::now())
    .execute(&mut **tx)
    .await?;
    let (id, allow): (u64, bool) = sqlx::query_as(
        "select id, allow_negative from ledger_account where scope = ? and kind = ? and currency = ? for update",
    )
    .bind(account.scope())
    .bind(account.kind.as_str())
    .bind(&account.currency)
    .fetch_one(&mut **tx)
    .await?;
    Ok((id, allow))
}

pub async fn set_allow_negative(
    tx: &mut Transaction<'_, MySql>,
    account: &AccountRef,
    allow: bool,
) -> Result<(), LedgerError> {
    let (id, _) = ensure_account(tx, account, Some(allow)).await?;
    sqlx::query("update ledger_account set allow_negative = ? where id = ?")
        .bind(allow)
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn post(
    tx: &mut Transaction<'_, MySql>,
    posting: &Posting,
    idempotency_key: &str,
) -> Result<Posted, LedgerError> {
    validate(&posting.entries)?;
    if idempotency_key.is_empty() || idempotency_key.len() > 191 {
        return Err(LedgerError::BadKey);
    }
    let now = Utc::now();
    let (reference_kind, reference_id) = match &posting.reference {
        Some((k, i)) => (Some(k.as_str()), Some(i.as_str())),
        None => (None, None),
    };
    if let Some(id) = existing(tx, idempotency_key, false).await? {
        return Ok(Posted {
            transaction_id: id,
            replayed: true,
        });
    }
    let inserted = sqlx::query(
        "insert into ledger_transaction (kind, reference_kind, reference_id, idempotency_key, created) values (?, ?, ?, ?, ?)",
    )
    .bind(posting.kind.as_str())
    .bind(reference_kind)
    .bind(reference_id)
    .bind(idempotency_key)
    .bind(now)
    .execute(&mut **tx)
    .await;
    let transaction_id = match inserted {
        Ok(r) => r.last_insert_id(),
        Err(sqlx::Error::Database(d)) if d.is_unique_violation() => {
            let id = existing(tx, idempotency_key, true)
                .await?
                .ok_or(LedgerError::Db(sqlx::Error::RowNotFound))?;
            return Ok(Posted {
                transaction_id: id,
                replayed: true,
            });
        }
        Err(e) => return Err(e.into()),
    };

    let mut deltas: BTreeMap<(u64, &'static str, &str), (&AccountRef, i128)> = BTreeMap::new();
    for e in &posting.entries {
        deltas
            .entry(e.account.sort_key())
            .or_insert((&e.account, 0))
            .1 += e.amount as i128;
    }
    let mut ids: BTreeMap<(u64, &'static str, &str), (u64, bool)> = BTreeMap::new();
    for (key, (account, _)) in &deltas {
        ids.insert(*key, ensure_account(tx, account, None).await?);
    }
    for (key, (account, delta)) in &deltas {
        let (id, allow_negative) = ids[key];
        let (balance,): (i64,) =
            sqlx::query_as("select balance from ledger_account where id = ? for update")
                .bind(id)
                .fetch_one(&mut **tx)
                .await?;
        let next = i64::try_from(balance as i128 + delta).map_err(|_| LedgerError::Overflow)?;
        if next < 0 && *delta < 0 && !allow_negative {
            return Err(LedgerError::Overdraw {
                account: (*account).clone(),
                balance,
            });
        }
        sqlx::query("update ledger_account set balance = ? where id = ?")
            .bind(next)
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    for e in &posting.entries {
        let (id, _) = ids[&e.account.sort_key()];
        sqlx::query(
            "insert into ledger_entry (transaction_id, account_id, amount, currency, created) values (?, ?, ?, ?, ?)",
        )
        .bind(transaction_id)
        .bind(id)
        .bind(e.amount)
        .bind(&e.account.currency)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(Posted {
        transaction_id,
        replayed: false,
    })
}

async fn existing(
    tx: &mut Transaction<'_, MySql>,
    idempotency_key: &str,
    latest: bool,
) -> Result<Option<u64>, LedgerError> {
    let sql = if latest {
        "select id from ledger_transaction where idempotency_key = ? for update"
    } else {
        "select id from ledger_transaction where idempotency_key = ?"
    };
    let row: Option<(u64,)> = sqlx::query_as(sql)
        .bind(idempotency_key)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(row.map(|r| r.0))
}
