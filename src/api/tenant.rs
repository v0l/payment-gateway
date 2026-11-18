use super::page::{self, Page};
use super::{ApiError, ApiResult, AppState, bearer};
use crate::api_keys::*;
use crate::crypto::hash_api_key;
use crate::db::{self, Tenant, parse_payment_id};
use crate::model::{CreateAttempt, CreatePayment, PaymentView, RefundInput, RefundView};
use crate::service::MethodChoice;
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::StatusCode;
use axum::http::request::Parts;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

pub struct TenantAuth {
    pub tenant: Tenant,
    pub scopes: Scopes,
}

impl TenantAuth {
    pub fn require(self, scope: &str) -> Result<Tenant, ApiError> {
        if self.scopes.allows(scope) {
            Ok(self.tenant)
        } else {
            Err(ApiError::insufficient_scope(scope))
        }
    }
}

impl FromRequestParts<AppState> for TenantAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let key = bearer(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let row = db::api_key_by_hash(state.pool(), &hash_api_key(key))
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        let now = Utc::now();
        if row.revoked_at.is_some() || row.expires.is_some_and(|e| e <= now) {
            return Err(ApiError::unauthorized());
        }
        let tenant = db::get_tenant(state.pool(), row.tenant_id)
            .await?
            .filter(|t| t.enabled)
            .ok_or_else(ApiError::unauthorized)?;
        state
            .rate_limits
            .api_key
            .check(&format!("tenant:{}", tenant.id))
            .map_err(ApiError::rate_limited)?;
        crate::logging::set_tenant(tenant.id);
        db::touch_api_key(state.pool(), row.id, now).await?;
        Ok(TenantAuth {
            tenant,
            scopes: Scopes::parse(&row.scopes),
        })
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/openapi.json", get(super::docs::openapi_json))
        .route("/methods", get(list_methods))
        .route("/rates", get(rates))
        .route("/payments", post(create_payment).get(find_payments))
        .route("/payments/{id}", get(get_payment))
        .route("/payments/{id}/attempts", post(create_attempt))
        .route("/payments/{id}/cancel", post(cancel_payment))
        .route("/payments/{id}/refund", post(refund_payment))
        .route("/refunds", get(list_refunds))
        .route("/events", get(list_events))
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
    platform_managed: bool,
    method: String,
    name: String,
    fee_rate: Option<f64>,
    fee_base: Option<u64>,
    fee_currency: Option<String>,
}

async fn list_methods(
    auth: TenantAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page<MethodView>, ApiError> {
    let t = auth.require(SCOPE_PAYMENTS_READ)?;
    let limit = page::limit(q.get("limit"))?;
    let after = page::decode_id(q.get("cursor"))?.unwrap_or(0);
    let rows: Vec<MethodView> = s
        .service
        .usable_configs(&t)
        .await?
        .into_iter()
        .filter(|p| p.id > after)
        .map(|p| MethodView {
            platform_managed: p.shared_provider_id.is_some(),
            id: p.id,
            method: p.method,
            name: p.name,
            fee_rate: p.fee_rate,
            fee_base: p.fee_base,
            fee_currency: p.fee_currency,
        })
        .take(limit as usize + 1)
        .collect();
    Ok(Page::cut(rows, limit, |m| m.id.to_string()))
}

async fn rates(auth: TenantAuth, State(s): State<AppState>) -> ApiResult<Value> {
    auth.require(SCOPE_PAYMENTS_READ)?;
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
    auth: TenantAuth,
    State(s): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(input): Json<CreatePayment>,
) -> Result<(StatusCode, Json<CreatedPayment>), ApiError> {
    let t = auth.require(SCOPE_PAYMENTS_WRITE)?;
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
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<PaymentView> {
    let t = auth.require(SCOPE_PAYMENTS_READ)?;
    let row = s.service.payment_for_tenant(&t, &pid(&id)?).await?;
    Ok(Json(s.service.view(&row).await?))
}

fn parse_time(q: &HashMap<String, String>, key: &str) -> Result<Option<DateTime<Utc>>, ApiError> {
    match q.get(key) {
        None => Ok(None),
        Some(v) => DateTime::parse_from_rfc3339(v)
            .map(|d| Some(d.with_timezone(&Utc)))
            .or_else(|_| {
                v.parse::<i64>()
                    .ok()
                    .and_then(|n| DateTime::from_timestamp(n, 0))
                    .map(Some)
                    .ok_or(())
            })
            .map_err(|_| {
                ApiError::bad_request(format!("{key} must be RFC 3339 or unix seconds"))
                    .with_field(key)
            }),
    }
}

fn payment_position(p: &db::PaymentRow) -> String {
    format!("{}:{}", p.created.timestamp(), hex::encode(&p.id))
}

fn parse_payment_cursor(raw: &str) -> Result<(DateTime<Utc>, Vec<u8>), ApiError> {
    let bad = || ApiError::bad_request("invalid cursor").with_field("cursor");
    let pos = page::decode(raw)?;
    let (ts, id) = pos.split_once(':').ok_or_else(bad)?;
    let at = ts
        .parse::<i64>()
        .ok()
        .and_then(|n| DateTime::from_timestamp(n, 0))
        .ok_or_else(bad)?;
    Ok((at, hex::decode(id).map_err(|_| bad())?))
}

async fn find_payments(
    auth: TenantAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page<PaymentView>, ApiError> {
    let t = auth.require(SCOPE_PAYMENTS_READ)?;
    let limit = page::limit(q.get("limit"))?;
    let filter = db::PaymentFilter {
        status: q.get("status").map(String::as_str),
        reference: q.get("reference").map(String::as_str),
        customer_ref: q.get("customer_ref").map(String::as_str),
        created_from: parse_time(&q, "created_from")?,
        created_to: parse_time(&q, "created_to")?,
        after: q
            .get("cursor")
            .map(|c| parse_payment_cursor(c))
            .transpose()?,
    };
    let rows = db::list_payments_page(s.pool(), t.id, &filter, limit).await?;
    let more = rows.len() > limit as usize;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();
    let next = if more {
        rows.last().map(|r| page::encode(&payment_position(r)))
    } else {
        None
    };
    let mut items = vec![];
    for row in &rows {
        items.push(s.service.view(row).await?);
    }
    Ok(Page { items, next })
}

#[derive(Serialize)]
struct RefundListItem {
    payment_id: String,
    #[serde(flatten)]
    refund: RefundView,
}

async fn list_refunds(
    auth: TenantAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page<RefundListItem>, ApiError> {
    let t = auth.require(SCOPE_PAYMENTS_READ)?;
    let limit = page::limit(q.get("limit"))?;
    let after = page::decode_id(q.get("cursor"))?;
    let payment = q
        .get("payment_id")
        .map(|p| {
            parse_payment_id(p)
                .ok_or_else(|| ApiError::bad_request("invalid payment_id").with_field("payment_id"))
        })
        .transpose()?;
    let rows = db::list_refunds_page(
        s.pool(),
        t.id,
        after,
        payment.as_ref(),
        q.get("status").map(String::as_str),
        limit,
    )
    .await?;
    let items = rows
        .iter()
        .map(|r| RefundListItem {
            payment_id: hex::encode(&r.payment_id),
            refund: RefundView::from(r),
        })
        .collect::<Vec<_>>();
    Ok(Page::cut(items, limit, |r| r.refund.id.to_string()))
}

async fn list_events(
    auth: TenantAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page<Value>, ApiError> {
    let t = auth.require(SCOPE_PAYMENTS_READ)?;
    let limit = page::limit(q.get("limit"))?;
    let after = page::decode_id(q.get("cursor"))?.unwrap_or(0);
    let rows = db::events_after(
        s.pool(),
        t.id,
        after,
        q.get("type").map(String::as_str),
        limit + 1,
    )
    .await?;
    let more = rows.len() > limit as usize;
    let rows: Vec<_> = rows.into_iter().take(limit as usize).collect();
    let next = if more {
        rows.last().map(|(id, _)| page::encode(&id.to_string()))
    } else {
        None
    };
    let items = rows
        .into_iter()
        .map(|(_, body)| serde_json::from_str(&body).unwrap_or(Value::Null))
        .collect();
    Ok(Page { items, next })
}

async fn create_attempt(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CreateAttempt>,
) -> ApiResult<PaymentView> {
    let t = auth.require(SCOPE_PAYMENTS_WRITE)?;
    let id = pid(&id)?;
    s.service.create_attempt(&t, &id, input).await?;
    let row = s.service.payment_for_tenant(&t, &id).await?;
    Ok(Json(s.service.view(&row).await?))
}

async fn cancel_payment(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<PaymentView> {
    let t = auth.require(SCOPE_PAYMENTS_WRITE)?;
    let row = s.service.cancel_payment(&t, &pid(&id)?).await?;
    Ok(Json(s.service.view(&row).await?))
}

async fn refund_payment(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RefundInput>,
) -> ApiResult<RefundView> {
    let t = auth.require(SCOPE_REFUNDS_WRITE)?;
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
    auth: TenantAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Page<SavedView>, ApiError> {
    let t = if auth.scopes.allows(SCOPE_SAVED_WRITE) {
        auth.tenant
    } else {
        auth.require(SCOPE_PAYMENTS_READ)?
    };
    let limit = page::limit(q.get("limit"))?;
    let after = page::decode_id(q.get("cursor"))?;
    let rows: Vec<SavedView> = db::list_saved_methods(
        s.pool(),
        t.id,
        q.get("customer_ref").map(String::as_str),
        after,
        limit,
    )
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
    .collect();
    Ok(Page::cut(rows, limit, |m| m.id.to_string()))
}

async fn delete_saved(
    auth: TenantAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    let t = auth.require(SCOPE_SAVED_WRITE)?;
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
    auth: TenantAuth,
    State(s): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<CreatePayment>,
) -> Result<(StatusCode, Json<ChargeResult>), ApiError> {
    let t = auth.require(SCOPE_SAVED_WRITE)?;
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
