use super::page::{self, Page};
use super::tenant::TenantAuth;
use super::{ApiError, AppState};
use crate::api_keys::{SCOPE_WITHDRAWALS_READ, SCOPE_WITHDRAWALS_WRITE};
use crate::payouts::WithdrawalView;
use crate::service::CreateWithdrawal;
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use std::collections::HashMap;

pub async fn create_withdrawal(
    auth: TenantAuth,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateWithdrawal>,
) -> Result<(StatusCode, Json<WithdrawalView>), ApiError> {
    let t = auth.require(SCOPE_WITHDRAWALS_WRITE)?;
    let key = headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::bad_request("Idempotency-Key header is required"))?;
    let (row, created) = s.service.create_withdrawal(&t, input, key).await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(WithdrawalView::from(&row))))
}

pub async fn get_withdrawal(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> Result<Json<WithdrawalView>, ApiError> {
    let t = auth.require(SCOPE_WITHDRAWALS_READ)?;
    let w = s.service.withdrawal_for_tenant(&t, id).await?;
    if w.purpose != "payout" {
        return Err(ApiError::not_found());
    }
    Ok(Json(WithdrawalView::from(&w)))
}

pub async fn list_withdrawals(
    auth: TenantAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page<WithdrawalView>, ApiError> {
    let t = auth.require(SCOPE_WITHDRAWALS_READ)?;
    let limit = page::limit(q.get("limit"))?;
    let before = page::decode_id(q.get("cursor"))?;
    let rows = s
        .service
        .list_withdrawals(&t, before, q.get("status").map(String::as_str), limit)
        .await?;
    let items = rows.iter().map(WithdrawalView::from).collect::<Vec<_>>();
    Ok(Page::cut(items, limit, |w| w.id.to_string()))
}
