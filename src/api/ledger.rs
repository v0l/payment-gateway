use super::{ApiResult, AppState};
use crate::ledger::{self, AccountBalance, EntryView, Mismatch};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct BalanceView {
    pub balances: Vec<AccountBalance>,
}

pub async fn balance_view(s: &AppState, tenant_id: u64) -> ApiResult<BalanceView> {
    Ok(axum::Json(BalanceView {
        balances: ledger::balances(s.pool(), tenant_id).await?,
    }))
}

pub async fn entries_view(
    s: &AppState,
    tenant_id: u64,
    q: &HashMap<String, String>,
) -> ApiResult<Vec<EntryView>> {
    let before = q.get("before").and_then(|v| v.parse().ok());
    let limit = q.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50);
    Ok(axum::Json(
        ledger::history(s.pool(), tenant_id, before, limit).await?,
    ))
}

pub async fn reconcile_view(s: &AppState) -> ApiResult<Vec<Mismatch>> {
    Ok(axum::Json(ledger::reconcile(s.pool()).await?))
}
