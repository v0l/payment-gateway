use super::*;
use crate::treasury::{self, RunView};

#[derive(Serialize)]
pub(super) struct TreasuryPage {
    latest: Option<RunView>,
    liabilities: i64,
    tenant_balances: i64,
    pending_payouts: i64,
    fees_earned: i64,
    assets: Option<i64>,
    difference: Option<i64>,
    tolerance_msat: u64,
    tenants: Vec<treasury::TenantLine>,
    runs: Vec<RunView>,
    wallets: Value,
    stuck: Vec<treasury::StuckItem>,
}

pub(super) async fn admin_treasury(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<TreasuryPage> {
    sess.operator(&s)?;
    let totals = treasury::totals(s.pool()).await?;
    let latest = treasury::latest_run(s.pool()).await?;
    let details = treasury::latest_details(s.pool()).await?;
    let stuck = treasury::find_stuck(
        s.pool(),
        s.service.clock.now(),
        crate::service::UNCONFIRMED_GRACE_SECS,
    )
    .await?;
    Ok(Json(TreasuryPage {
        assets: latest.as_ref().map(|r| r.assets),
        difference: latest.as_ref().map(|r| r.difference),
        latest: latest.map(RunView::from),
        liabilities: totals.liabilities,
        tenant_balances: totals.tenant_balances,
        pending_payouts: totals.pending_payouts,
        fees_earned: totals.platform_fees,
        tolerance_msat: s.service.treasury.tolerance_msat(),
        tenants: treasury::tenant_lines(s.pool()).await?,
        runs: treasury::list_runs(s.pool(), 50)
            .await?
            .into_iter()
            .map(RunView::from)
            .collect(),
        wallets: details
            .and_then(|d| d.get("wallets").cloned())
            .unwrap_or(json!([])),
        stuck,
    }))
}

pub(super) async fn admin_treasury_run(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
) -> ApiResult<RunView> {
    sess.operator(&s)?;
    let run = s.service.run_reconciliation("manual").await?;
    sess.event(Action::TreasuryReconciled)
        .details(json!({"run": run.id, "status": run.status}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(run.into()))
}

pub(super) async fn admin_treasury_banner(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    let alerts = treasury::latest_run(s.pool())
        .await?
        .map(|r| r.alert_list())
        .unwrap_or_default();
    Ok(Json(json!({"alerts": alerts})))
}
