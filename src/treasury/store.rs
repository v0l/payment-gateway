use super::{Details, Totals};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{FromRow, MySqlPool};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct RunSummary {
    pub id: u64,
    pub created: DateTime<Utc>,
    pub trigger_kind: String,
    pub tenant_balances: i64,
    pub pending_payouts: i64,
    pub platform_fees: i64,
    pub liabilities: i64,
    pub assets: i64,
    pub difference: i64,
    pub wallets_checked: u32,
    pub wallet_errors: u32,
    pub drift_accounts: u32,
    pub unbalanced_transactions: u32,
    pub stuck_count: u32,
    pub status: String,
    #[serde(skip)]
    pub alerts: String,
}

impl RunSummary {
    pub fn alert_list(&self) -> Vec<String> {
        serde_json::from_str(&self.alerts).unwrap_or_default()
    }
}

#[derive(Serialize)]
pub struct RunView {
    #[serde(flatten)]
    pub run: RunSummary,
    pub alerts: Vec<String>,
}

impl From<RunSummary> for RunView {
    fn from(run: RunSummary) -> Self {
        let alerts = run.alert_list();
        Self { run, alerts }
    }
}

const COLS: &str = "id, created, trigger_kind, tenant_balances, pending_payouts, platform_fees, liabilities, assets, difference, wallets_checked, wallet_errors, drift_accounts, unbalanced_transactions, stuck_count, status, alerts";

#[allow(clippy::too_many_arguments)]
pub async fn insert_run(
    pool: &MySqlPool,
    now: DateTime<Utc>,
    trigger: &str,
    totals: &Totals,
    assets: i64,
    wallets_checked: usize,
    wallet_errors: usize,
    drift_accounts: usize,
    unbalanced: usize,
    stuck: usize,
    alerts: &[String],
    details: &Details,
) -> Result<u64> {
    let status = if alerts.is_empty() { "ok" } else { "alert" };
    let res = sqlx::query(
        "insert into reconciliation_run (created, trigger_kind, tenant_balances, pending_payouts, platform_fees, liabilities, assets, difference, wallets_checked, wallet_errors, drift_accounts, unbalanced_transactions, stuck_count, status, alerts, details) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(now)
    .bind(trigger)
    .bind(totals.tenant_balances)
    .bind(totals.pending_payouts)
    .bind(totals.platform_fees)
    .bind(totals.liabilities)
    .bind(assets)
    .bind(assets - totals.liabilities)
    .bind(wallets_checked as u32)
    .bind(wallet_errors as u32)
    .bind(drift_accounts as u32)
    .bind(unbalanced as u32)
    .bind(stuck as u32)
    .bind(status)
    .bind(serde_json::to_string(alerts)?)
    .bind(serde_json::to_string(details)?)
    .execute(pool)
    .await?;
    Ok(res.last_insert_id())
}

pub async fn latest_run(pool: &MySqlPool) -> Result<Option<RunSummary>> {
    Ok(sqlx::query_as(&format!(
        "select {COLS} from reconciliation_run order by id desc limit 1"
    ))
    .fetch_optional(pool)
    .await?)
}

pub async fn list_runs(pool: &MySqlPool, limit: u32) -> Result<Vec<RunSummary>> {
    Ok(sqlx::query_as(&format!(
        "select {COLS} from reconciliation_run order by id desc limit ?"
    ))
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await?)
}

pub async fn latest_details(pool: &MySqlPool) -> Result<Option<serde_json::Value>> {
    let row: Option<String> =
        sqlx::query_scalar("select details from reconciliation_run order by id desc limit 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|d| serde_json::from_str(&d).ok()))
}

pub async fn totals(pool: &MySqlPool) -> Result<Totals> {
    let rows: Vec<(String, i64)> = sqlx::query_as(
        "select kind, cast(coalesce(sum(balance), 0) as signed) from ledger_account where currency = 'BTC' and kind in ('tenant_balance', 'pending_payout', 'platform_fees') group by kind",
    )
    .fetch_all(pool)
    .await?;
    let get = |k: &str| rows.iter().find(|r| r.0 == k).map(|r| r.1).unwrap_or(0);
    let tenant_balances = get("tenant_balance");
    let pending_payouts = get("pending_payout");
    let platform_fees = get("platform_fees");
    Ok(Totals {
        tenant_balances,
        pending_payouts,
        platform_fees,
        liabilities: tenant_balances + pending_payouts + platform_fees,
    })
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct TenantLine {
    pub tenant_id: u64,
    pub name: String,
    pub balance: i64,
    pub pending_payout: i64,
}

pub async fn tenant_lines(pool: &MySqlPool) -> Result<Vec<TenantLine>> {
    Ok(sqlx::query_as(
        "select t.id as tenant_id, t.name, cast(coalesce(sum(case when a.kind = 'tenant_balance' then a.balance end), 0) as signed) as balance, cast(coalesce(sum(case when a.kind = 'pending_payout' then a.balance end), 0) as signed) as pending_payout from tenant t join ledger_account a on a.tenant_id = t.id and a.currency = 'BTC' group by t.id, t.name having balance <> 0 or pending_payout <> 0 order by balance desc, t.id",
    )
    .fetch_all(pool)
    .await?)
}
