use super::*;
use crate::limits::LimitOverrides;
use crate::platform_views::{PlatformAccountView, PlatformInvoiceView, PlatformPlanView};
use crate::service::PlanDefinition;

#[derive(Serialize)]
pub(super) struct PlatformBillingView {
    account: PlatformAccountView,
    invoices: Vec<PlatformInvoiceView>,
}

pub(super) async fn project_platform_billing(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<PlatformBillingView> {
    let (t, _) = sess.project(&s, id, Role::Viewer).await?;
    let account = s.service.ensure_platform_account(&t).await?;
    let plan = s.service.platform_plan(account.plan_id).await?;
    let usage = s
        .service
        .metered_usage(id, account.period_start, account.period_end, &plan.currency)
        .await?;
    let rows = crate::db::list_platform_invoices(s.pool(), id, 100).await?;
    let mut invoices = vec![];
    for r in &rows {
        invoices.push(s.service.invoice_view(r).await?);
    }
    Ok(Json(PlatformBillingView {
        account: PlatformAccountView::build(&account, &plan, usage),
        invoices,
    }))
}

pub(super) async fn list_platform_plans(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<Vec<PlatformPlanView>> {
    let rows = crate::db::list_platform_plans(s.pool()).await?;
    let _ = &sess;
    Ok(Json(rows.iter().map(PlatformPlanView::from).collect()))
}

#[derive(Deserialize)]
pub(super) struct PlanBody {
    name: String,
    #[serde(default)]
    live: bool,
    #[serde(default)]
    monthly_price: u64,
    currency: Option<String>,
    #[serde(default)]
    included_volume: u64,
    #[serde(default)]
    fee_bps: u32,
    #[serde(default)]
    fee_base: u64,
    #[serde(default)]
    limits: LimitOverrides,
    active: Option<bool>,
}

impl From<PlanBody> for PlanDefinition {
    fn from(b: PlanBody) -> Self {
        Self {
            name: b.name,
            live: b.live,
            monthly_price: b.monthly_price,
            currency: b
                .currency
                .unwrap_or_else(|| crate::platform::CURRENCY.to_string()),
            included_volume: b.included_volume,
            fee_bps: b.fee_bps,
            fee_base: b.fee_base,
            limits: b.limits,
            active: b.active.unwrap_or(true),
        }
    }
}

pub(super) async fn admin_define_platform_plan(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(b): Json<PlanBody>,
) -> ApiResult<PlatformPlanView> {
    sess.operator(&s)?;
    let plan = s.service.define_platform_plan(Some(id), b.into()).await?;
    plan_event(&sess, &s, &headers, &plan).await;
    Ok(Json(PlatformPlanView::from(&plan)))
}

pub(super) async fn admin_create_platform_plan(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<PlanBody>,
) -> ApiResult<PlatformPlanView> {
    sess.operator(&s)?;
    let plan = s.service.define_platform_plan(None, b.into()).await?;
    plan_event(&sess, &s, &headers, &plan).await;
    Ok(Json(PlatformPlanView::from(&plan)))
}

async fn plan_event(
    sess: &Session,
    s: &AppState,
    headers: &HeaderMap,
    plan: &crate::db::PlatformPlanRow,
) {
    sess.event(Action::PlatformPlanDefined)
        .target("platform_plan", plan.id)
        .details(json!({
            "name": plan.name,
            "monthly_price": plan.monthly_price,
            "fee_bps": plan.fee_bps,
            "fee_base": plan.fee_base,
            "included_volume": plan.included_volume,
            "active": plan.active,
        }))
        .record(s.pool(), &Origin::from_headers(headers))
        .await;
}

#[derive(Deserialize)]
pub(super) struct PlatformTenantBody {
    tenant_id: Option<u64>,
}

pub(super) async fn admin_set_platform_tenant(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<PlatformTenantBody>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    s.service.set_platform_tenant(b.tenant_id).await?;
    sess.event(Action::PlatformAccountSet)
        .details(json!({"tenant_id": b.tenant_id}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"tenant_id": b.tenant_id})))
}

#[derive(Deserialize)]
pub(super) struct ChangePlanBody {
    plan_id: u64,
    reason: String,
}

pub(super) async fn admin_change_platform_plan(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(b): Json<ChangePlanBody>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    let (old, new, diff) = s
        .service
        .change_platform_plan(id, b.plan_id, &b.reason)
        .await?;
    sess.event(Action::PlatformPlanChanged)
        .tenant(id)
        .target("project", id)
        .details(json!({"from": old.id, "to": new.id, "proration": diff, "reason": b.reason}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"plan_id": new.id, "proration": diff})))
}

#[derive(Deserialize)]
pub(super) struct CreditBody {
    amount: u64,
    reason: String,
}

pub(super) async fn admin_credit_platform(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(b): Json<CreditBody>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    let credit = s
        .service
        .grant_platform_credit(id, b.amount, &b.reason)
        .await?;
    sess.event(Action::PlatformCredited)
        .tenant(id)
        .target("project", id)
        .details(json!({"amount": b.amount, "reason": b.reason}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"credit": credit})))
}

#[derive(Deserialize)]
pub(super) struct WaiveBody {
    reason: String,
}

pub(super) async fn admin_waive_platform_invoice(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, invoice)): Path<(u64, u64)>,
    Json(b): Json<WaiveBody>,
) -> ApiResult<PlatformInvoiceView> {
    sess.operator(&s)?;
    let row = s
        .service
        .waive_platform_invoice(id, invoice, &b.reason)
        .await?;
    sess.event(Action::PlatformInvoiceWaived)
        .tenant(id)
        .target("platform_invoice", invoice)
        .details(json!({"reason": b.reason, "total": row.total}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(s.service.invoice_view(&row).await?))
}
