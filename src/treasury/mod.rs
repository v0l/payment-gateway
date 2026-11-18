mod store;
mod stuck;

use crate::ledger::Mismatch;
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};

pub use store::{
    RunSummary, RunView, TenantLine, insert_run, latest_details, latest_run, list_runs,
    tenant_lines, totals,
};
pub use stuck::{
    StuckItem, StuckKind, WEBHOOK_GRACE_SECS, WITHDRAWAL_SENDING_LIMIT_SECS, find_stuck,
};

pub const RUN_INTERVAL_SECS: i64 = 3600;
pub const ALERT_KIND: &str = "treasury_alert";

#[derive(Default)]
pub struct Settings {
    tolerance_msat: AtomicU64,
}

impl Settings {
    pub fn tolerance_msat(&self) -> u64 {
        self.tolerance_msat.load(Ordering::Relaxed)
    }

    pub fn set_tolerance_msat(&self, v: u64) {
        self.tolerance_msat.store(v, Ordering::Relaxed);
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WalletLine {
    pub identity: String,
    pub kind: String,
    pub msat: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Totals {
    pub tenant_balances: i64,
    pub pending_payouts: i64,
    pub platform_fees: i64,
    pub liabilities: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct Details {
    pub wallets: Vec<WalletLine>,
    pub mismatches: Vec<Mismatch>,
    pub stuck: Vec<StuckItem>,
}

pub fn assess(
    liabilities: i64,
    assets: i64,
    wallet_errors: usize,
    drift_accounts: usize,
    unbalanced: usize,
    tolerance_msat: u64,
) -> Vec<String> {
    let mut alerts = vec![];
    if wallet_errors > 0 {
        alerts.push(format!(
            "{wallet_errors} wallet balance lookups failed, solvency could not be verified"
        ));
    } else if liabilities - assets > tolerance_msat as i64 {
        alerts.push(format!(
            "liabilities exceed assets by {} sats",
            (liabilities - assets) / 1000
        ));
    }
    if unbalanced > 0 {
        alerts.push(format!(
            "{unbalanced} ledger transactions fail the zero-sum check"
        ));
    }
    if drift_accounts > 0 {
        alerts.push(format!(
            "{drift_accounts} ledger accounts have a cached balance that differs from their entries"
        ));
    }
    alerts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_has_no_alerts() {
        assert!(assess(1000, 1000, 0, 0, 0, 0).is_empty());
        assert!(assess(1000, 5000, 0, 0, 0, 0).is_empty());
    }

    #[test]
    fn deficit_beyond_tolerance_alerts() {
        assert!(assess(2000, 1000, 0, 0, 0, 1000).is_empty());
        assert_eq!(assess(2001, 1000, 0, 0, 0, 1000).len(), 1);
    }

    #[test]
    fn failed_wallet_lookup_is_never_silent() {
        assert_eq!(assess(0, 0, 1, 0, 0, 0).len(), 1);
    }
}
