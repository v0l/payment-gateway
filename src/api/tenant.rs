use super::{ApiError, ApiResult, AppState, bearer};
use crate::crypto::hash_api_key;
use crate::db::{self, Tenant, parse_payment_id};
use crate::model::{CreateAttempt, CreatePayment, PaymentView, RefundInput, RefundView};
use crate::service::MethodChoice;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub struct TenantAuth(pub Tenant);

impl FromRequestParts<AppState> for TenantAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let key = bearer(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let tenant = db::tenant_by_key_hash(state.pool(), &hash_api_key(key))
            .await?
            .filter(|t| t.enabled)
            .ok_or_else(ApiError::unauthorized)?;
        Ok(TenantAuth(tenant))
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/methods", get(list_methods))
        .route("/rates", get(rates))
        .route("/payments", post(create_payment).get(find_payments))
        .route("/payments/{id}", get(get_payment))
        .route("/payments/{id}/attempts", post(create_attempt))
        .route("/payments/{id}/cancel", post(cancel_payment))
        .route("/payments/{id}/refund", post(refund_payment))
        .route("/saved-methods", get(list_saved))
        .route("/saved-methods/{id}", delete(delete_saved))
        .route("/saved-methods/{id}/charge", post(charge_saved))
}

fn pid(raw: &str) -> Result<db::PaymentId, ApiError> {
    parse_payment_id(raw).ok_or_else(ApiError::not_found)
}

fn idempotency(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get("idempotency-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
}

#[derive(Serialize)]
struct MethodView {
    id: u64,
    method: String,
    name: String,
    fee_rate: Option<f64>,
    fee_base: Option<u64>,
    fee_currency: Option<String>,
}

async fn list_methods(
    TenantAuth(t): TenantAuth,
    State(s): State<AppState>,
) -> ApiResult<Vec<MethodView>> {
    Ok(Json(
        db::list_provider_configs(s.pool(), t.id, true)
            .await?
            .into_iter()
            .map(|p| MethodView {
                id: p.id,
                method: p.method,
                name: p.name,
                fee_rate: p.fee_rate,
                fee_base: p.fee_base,
                fee_currency: p.fee_currency,
            })
            .collect(),
    ))
}

async fn rates(TenantAuth(_): TenantAuth, State(s): State<AppState>) -> ApiResult<Value> {
    let all = s.service.rates.all().await?;
    Ok(Json(serde_json::json!({ "btc": all })))
}

#[derive(Serialize)]
struct CreatedPayment {
    #[serde(flatten)]
    payment: PaymentView,
    methods: Vec<MethodChoice>,
}

async fn create_payment(
    TenantAuth(t): TenantAuth,
    State(s): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(input): Json<CreatePayment>,
) -> Result<(StatusCode, Json<CreatedPayment>), ApiError> {
    let (row, created) = s
        .service
        .create_payment(&t, input, idempotency(&headers).as_deref())
        .await?;
    let payment = s.service.view(&row).await?;
    let methods = s.service.available_methods(&t, &row).await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(CreatedPayment { payment, methods })))
}

async fn get_payment(
    TenantAuth(t): TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<PaymentView> {
    let row = s.service.payment_for_tenant(&t, &pid(&id)?).await?;
    Ok(Json(s.service.view(&row).await?))
}

async fn find_payments(
    TenantAuth(t): TenantAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<PaymentView>> {
    let reference = q
        .get("reference")
        .ok_or_else(|| ApiError::bad_request("reference query parameter required"))?;
    let mut out = vec![];
    for row in db::payments_by_reference(s.pool(), t.id, reference).await? {
        out.push(s.service.view(&row).await?);
    }
    Ok(Json(out))
}

async fn create_attempt(
    TenantAuth(t): TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CreateAttempt>,
) -> ApiResult<PaymentView> {
    let id = pid(&id)?;
    s.service.create_attempt(&t, &id, input).await?;
    let row = s.service.payment_for_tenant(&t, &id).await?;
    Ok(Json(s.service.view(&row).await?))
}

async fn cancel_payment(
    TenantAuth(t): TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<PaymentView> {
    let row = s.service.cancel_payment(&t, &pid(&id)?).await?;
    Ok(Json(s.service.view(&row).await?))
}

async fn refund_payment(
    TenantAuth(t): TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RefundInput>,
) -> ApiResult<RefundView> {
    Ok(Json(s.service.refund(&t, &pid(&id)?, input).await?))
}

#[derive(Serialize)]
struct SavedView {
    id: u64,
    customer_ref: String,
    provider_id: u64,
    brand: Option<String>,
    last4: Option<String>,
    exp_month: Option<u16>,
    exp_year: Option<u16>,
}

async fn list_saved(
    TenantAuth(t): TenantAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<SavedView>> {
    let customer = q
        .get("customer_ref")
        .ok_or_else(|| ApiError::bad_request("customer_ref query parameter required"))?;
    Ok(Json(
        db::list_saved_methods(s.pool(), t.id, customer)
            .await?
            .into_iter()
            .map(|m| SavedView {
                id: m.id,
                customer_ref: m.customer_ref,
                provider_id: m.provider_config_id,
                brand: m.brand,
                last4: m.last4,
                exp_month: m.exp_month,
                exp_year: m.exp_year,
            })
            .collect(),
    ))
}

async fn delete_saved(
    TenantAuth(t): TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    if db::disable_saved_method(s.pool(), t.id, id).await? {
        Ok(Json(serde_json::json!({ "deleted": true })))
    } else {
        Err(ApiError::not_found())
    }
}

#[derive(Serialize)]
struct ChargeResult {
    #[serde(flatten)]
    payment: PaymentView,
}

async fn charge_saved(
    TenantAuth(t): TenantAuth,
    State(s): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<CreatePayment>,
) -> Result<(StatusCode, Json<ChargeResult>), ApiError> {
    let (row, created) = s
        .service
        .charge_saved(&t, id, input, idempotency(&headers).as_deref())
        .await?;
    let status = if created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    Ok((
        status,
        Json(ChargeResult {
            payment: s.service.view(&row).await?,
        }),
    ))
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct _Unused;
