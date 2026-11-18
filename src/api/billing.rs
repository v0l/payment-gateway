use super::page::{self, Page};
use super::tenant::TenantAuth;
use super::{ApiError, ApiResult, AppState};
use crate::api_keys::*;
use crate::billing_views::*;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use std::collections::HashMap;

pub(super) async fn create_plan(
    auth: TenantAuth,
    State(s): State<AppState>,
    Json(input): Json<CreatePlan>,
) -> Result<(StatusCode, Json<PlanView>), ApiError> {
    let t = auth.require(SCOPE_PAYMENTS_WRITE)?;
    let plan = s.service.create_plan(&t, input).await?;
    Ok((StatusCode::CREATED, Json(PlanView::from(&plan))))
}

pub(super) async fn list_plans(
    auth: TenantAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page<PlanView>, ApiError> {
    let t = auth.require(SCOPE_PAYMENTS_READ)?;
    let limit = page::limit(q.get("limit"))?;
    let after = page::decode_id(q.get("cursor"))?;
    let rows = crate::db::list_plans(s.pool(), t.id, after, limit).await?;
    let items = rows.iter().map(PlanView::from).collect();
    Ok(Page::cut(items, limit, |p: &PlanView| p.id.to_string()))
}

pub(super) async fn get_plan(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<PlanView> {
    let t = auth.require(SCOPE_PAYMENTS_READ)?;
    Ok(Json(PlanView::from(
        &s.service.plan_for_tenant(&t, id).await?,
    )))
}

pub(super) async fn archive_plan(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<PlanView> {
    let t = auth.require(SCOPE_PAYMENTS_WRITE)?;
    Ok(Json(PlanView::from(&s.service.archive_plan(&t, id).await?)))
}

pub(super) async fn create_subscription(
    auth: TenantAuth,
    State(s): State<AppState>,
    Json(input): Json<CreateSubscription>,
) -> Result<(StatusCode, Json<SubscriptionView>), ApiError> {
    let t = auth.require(SCOPE_PAYMENTS_WRITE)?;
    let sub = s.service.create_subscription(&t, input).await?;
    let view = s.service.subscription_view(&t, &sub).await?;
    Ok((StatusCode::CREATED, Json(view)))
}

pub(super) async fn list_subscriptions(
    auth: TenantAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page<SubscriptionView>, ApiError> {
    let t = auth.require(SCOPE_PAYMENTS_READ)?;
    let limit = page::limit(q.get("limit"))?;
    let after = page::decode_id(q.get("cursor"))?;
    let rows = crate::db::list_subscriptions(
        s.pool(),
        t.id,
        after,
        q.get("customer_ref").map(String::as_str),
        q.get("status").map(String::as_str),
        limit,
    )
    .await?;
    let items = rows
        .iter()
        .map(|r| SubscriptionView::build(r, &[], &s.service.public_url))
        .collect();
    Ok(Page::cut(items, limit, |p: &SubscriptionView| {
        p.id.to_string()
    }))
}

pub(super) async fn get_subscription(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<SubscriptionView> {
    let t = auth.require(SCOPE_PAYMENTS_READ)?;
    let sub = s.service.subscription_row(&t, id).await?;
    Ok(Json(s.service.subscription_view(&t, &sub).await?))
}

pub(super) async fn pause(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<SubscriptionView> {
    let t = auth.require(SCOPE_PAYMENTS_WRITE)?;
    let sub = s.service.pause_subscription(&t, id).await?;
    Ok(Json(s.service.subscription_view(&t, &sub).await?))
}

pub(super) async fn resume(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<SubscriptionView> {
    let t = auth.require(SCOPE_PAYMENTS_WRITE)?;
    let sub = s.service.resume_subscription(&t, id).await?;
    Ok(Json(s.service.subscription_view(&t, &sub).await?))
}

pub(super) async fn cancel(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    body: Option<Json<CancelSubscription>>,
) -> ApiResult<SubscriptionView> {
    let t = auth.require(SCOPE_PAYMENTS_WRITE)?;
    let input = body.map(|b| b.0).unwrap_or_default();
    let sub = s.service.cancel_subscription(&t, id, input).await?;
    Ok(Json(s.service.subscription_view(&t, &sub).await?))
}

pub(super) async fn change_plan(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(input): Json<ChangePlan>,
) -> ApiResult<SubscriptionView> {
    let t = auth.require(SCOPE_PAYMENTS_WRITE)?;
    let sub = s.service.change_plan(&t, id, input).await?;
    Ok(Json(s.service.subscription_view(&t, &sub).await?))
}
