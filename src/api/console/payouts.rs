use super::*;
use crate::payouts::{self, DestinationKind, DestinationView, WithdrawalView};
use crate::service::{WithdrawalRequest, verify_challenge};

#[derive(Serialize)]
pub(super) struct PayoutsOverview {
    limits: crate::service::PayoutLimits,
    destinations: Vec<Value>,
    withdrawals: Vec<WithdrawalView>,
    node_ready: bool,
}

pub(super) async fn project_payouts(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<PayoutsOverview> {
    let (tenant, _) = sess.project(&s, id, Role::Viewer).await?;
    let now = Utc::now();
    let destinations = db::list_destinations(s.pool(), id)
        .await?
        .iter()
        .map(|d| {
            let mut v = json!(payouts::destination_view(d, &s.service.sealer, now));
            if !d.verified && d.kind == DestinationKind::Onchain.as_str() {
                v["challenge"] = json!(verify_challenge(d));
            }
            v
        })
        .collect();
    let withdrawals = db::withdrawals_for_tenant(s.pool(), id, 100)
        .await?
        .iter()
        .map(WithdrawalView::from)
        .collect();
    Ok(Json(PayoutsOverview {
        limits: s.service.payout_limits(&tenant).await?,
        destinations,
        withdrawals,
        node_ready: s.service.payouts.node().is_some(),
    }))
}

#[derive(Deserialize)]
pub(super) struct NewDestinationInput {
    kind: String,
    label: String,
    value: String,
}

pub(super) async fn add_payout_destination(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(i): Json<NewDestinationInput>,
) -> ApiResult<DestinationView> {
    let (tenant, _) = sess.project(&s, id, Role::Owner).await?;
    let kind = DestinationKind::parse(&i.kind)
        .ok_or_else(|| ApiError::bad_request("unknown destination kind"))?;
    let d = s
        .service
        .add_destination(&tenant, kind, &i.label, &i.value, Some(sess.account.id))
        .await?;
    sess.event(Action::PayoutDestinationAdded)
        .tenant(id)
        .target("payout_destination", d.id)
        .details(json!({"kind": d.kind, "label": d.label}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(payouts::destination_view(
        &d,
        &s.service.sealer,
        Utc::now(),
    )))
}

pub(super) async fn remove_payout_destination(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, did)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    let (tenant, _) = sess.project(&s, id, Role::Owner).await?;
    s.service.remove_destination(&tenant, did).await?;
    sess.event(Action::PayoutDestinationRemoved)
        .tenant(id)
        .target("payout_destination", did)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"removed": true})))
}

#[derive(Deserialize, Default)]
pub(super) struct VerifyInput {
    signature: Option<String>,
}

pub(super) async fn verify_payout_destination(
    sess: Session,
    State(s): State<AppState>,
    Path((id, did)): Path<(u64, u64)>,
    body: Option<Json<VerifyInput>>,
) -> ApiResult<Value> {
    let (tenant, _) = sess.project(&s, id, Role::Owner).await?;
    let input = body.map(|b| b.0).unwrap_or_default();
    match input.signature {
        Some(sig) => {
            let d = s
                .service
                .verify_destination_signature(&tenant, did, &sig)
                .await?;
            Ok(Json(
                json!({"verified": d.verified, "method": "signed_message"}),
            ))
        }
        None => {
            let w = s
                .service
                .verify_destination_payout(&tenant, did, Some(sess.account.id))
                .await?;
            Ok(Json(
                json!({"verified": false, "method": "test_payout", "withdrawal_id": w.id, "amount": w.amount}),
            ))
        }
    }
}

#[derive(Deserialize)]
pub(super) struct WithdrawalInput {
    amount: u64,
    destination_id: Option<u64>,
    invoice: Option<String>,
}

pub(super) async fn request_withdrawal(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(i): Json<WithdrawalInput>,
) -> ApiResult<WithdrawalView> {
    let (tenant, _) = sess.project(&s, id, Role::Owner).await?;
    let w = s
        .service
        .request_withdrawal(
            &tenant,
            WithdrawalRequest {
                amount: i.amount,
                destination_id: i.destination_id,
                invoice: i.invoice,
                requested_by: Some(sess.account.id),
            },
        )
        .await?;
    sess.event(Action::WithdrawalRequested)
        .tenant(id)
        .target("withdrawal", w.id)
        .details(json!({"amount": w.amount, "kind": w.destination_kind, "status": w.status}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(WithdrawalView::from(&w)))
}

pub(super) async fn cancel_withdrawal(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, wid)): Path<(u64, u64)>,
) -> ApiResult<WithdrawalView> {
    let (tenant, _) = sess.project(&s, id, Role::Owner).await?;
    let w = s.service.cancel_withdrawal(&tenant, wid).await?;
    sess.event(Action::WithdrawalCancelled)
        .tenant(id)
        .target("withdrawal", wid)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(WithdrawalView::from(&w)))
}

#[derive(Serialize)]
pub(super) struct QueueItem {
    withdrawal: WithdrawalView,
    project: String,
    trusted: bool,
    balances: Vec<crate::ledger::AccountBalance>,
    recent: Vec<WithdrawalView>,
    completed_count: i64,
    failed_count: i64,
}

pub(super) async fn admin_withdrawals(
    sess: Session,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<QueueItem>> {
    sess.operator(&s)?;
    let status = q.get("status").map(String::as_str).unwrap_or("requested");
    let rows = db::withdrawals_by_status(s.pool(), status, 100).await?;
    let mut out = Vec::new();
    for w in rows {
        let tenant = s.service.tenant(w.tenant_id).await?;
        let counts = db::withdrawal_counts(s.pool(), w.tenant_id).await?;
        let count = |k: &str| counts.iter().find(|c| c.0 == k).map(|c| c.1).unwrap_or(0);
        out.push(QueueItem {
            project: tenant.name.clone(),
            trusted: tenant.trusted,
            balances: crate::ledger::balances(s.pool(), w.tenant_id).await?,
            recent: db::withdrawals_for_tenant(s.pool(), w.tenant_id, 10)
                .await?
                .iter()
                .map(WithdrawalView::from)
                .collect(),
            completed_count: count("completed"),
            failed_count: count("failed") + count("rejected"),
            withdrawal: WithdrawalView::from(&w),
        });
    }
    Ok(Json(out))
}

pub(super) async fn admin_approve_withdrawal(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(wid): Path<u64>,
) -> ApiResult<WithdrawalView> {
    sess.operator(&s)?;
    let w = s
        .service
        .approve_withdrawal(wid, Some(sess.account.id))
        .await?;
    sess.event(Action::WithdrawalApproved)
        .tenant(w.tenant_id)
        .target("withdrawal", wid)
        .details(json!({"amount": w.amount}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(WithdrawalView::from(&w)))
}

#[derive(Deserialize)]
pub(super) struct RejectInput {
    #[serde(default)]
    reason: String,
}

pub(super) async fn admin_reject_withdrawal(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(wid): Path<u64>,
    Json(i): Json<RejectInput>,
) -> ApiResult<WithdrawalView> {
    sess.operator(&s)?;
    let w = s.service.reject_withdrawal(wid, &i.reason).await?;
    sess.event(Action::WithdrawalRejected)
        .tenant(w.tenant_id)
        .target("withdrawal", wid)
        .details(json!({"reason": i.reason}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(WithdrawalView::from(&w)))
}

#[derive(Deserialize)]
pub(super) struct PayoutLimitsInput {
    daily_max: Option<u64>,
    auto_threshold: Option<u64>,
}

pub(super) async fn admin_set_payout_limits(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(i): Json<PayoutLimitsInput>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    s.service.tenant(id).await?;
    db::set_payout_limits(
        s.pool(),
        id,
        db::PayoutLimitRow {
            daily_max: i.daily_max,
            auto_threshold: i.auto_threshold,
        },
    )
    .await?;
    sess.event(Action::PayoutLimitsChanged)
        .tenant(id)
        .target("project", id)
        .details(json!({"daily_max": i.daily_max, "auto_threshold": i.auto_threshold}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(
        json!({"daily_max": i.daily_max, "auto_threshold": i.auto_threshold}),
    ))
}
