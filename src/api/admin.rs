use super::{ApiError, ApiResult, AppState, bearer};
use crate::crypto::{hash_api_key, new_api_key, new_webhook_secret};
use crate::db::{self, NewProviderConfig, ProviderConfigRow};
use axum::extract::{FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use subtle::ConstantTimeEq;

pub struct AdminAuth;

impl FromRequestParts<AppState> for AdminAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = bearer(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        if token.as_bytes().ct_eq(state.admin_token.as_bytes()).into() {
            Ok(AdminAuth)
        } else {
            Err(ApiError::unauthorized())
        }
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/tenants", post(create_tenant).get(list_tenants))
        .route("/tenants/{id}/enabled", post(set_enabled))
        .route("/tenants/{id}/rotate-key", post(rotate_key))
        .route(
            "/tenants/{id}/providers",
            post(create_provider).get(list_providers),
        )
        .route(
            "/tenants/{id}/providers/{provider_id}/enabled",
            post(set_provider_enabled),
        )
        .route("/tenants/{id}", get(get_tenant))
}

#[derive(Serialize)]
struct TenantView {
    id: u64,
    name: String,
    webhook_url: Option<String>,
    enabled: bool,
}

impl From<db::Tenant> for TenantView {
    fn from(t: db::Tenant) -> Self {
        Self {
            id: t.id,
            name: t.name,
            webhook_url: t.webhook_url,
            enabled: t.enabled,
        }
    }
}

#[derive(Deserialize)]
struct CreateTenant {
    name: String,
    webhook_url: Option<String>,
}

#[derive(Serialize)]
struct CreatedTenant {
    id: u64,
    api_key: String,
    webhook_secret: String,
}

fn valid_webhook_url(url: &str) -> bool {
    url::Url::parse(url)
        .map(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
        .unwrap_or(false)
}

async fn create_tenant(
    _: AdminAuth,
    State(s): State<AppState>,
    Json(req): Json<CreateTenant>,
) -> ApiResult<CreatedTenant> {
    if req.name.trim().is_empty() {
        return Err(ApiError::bad_request("name required"));
    }
    if let Some(u) = &req.webhook_url {
        if !valid_webhook_url(u) {
            return Err(ApiError::bad_request("invalid webhook_url"));
        }
    }
    let api_key = new_api_key();
    let webhook_secret = new_webhook_secret();
    let sealed = s.sealer.seal(webhook_secret.as_bytes())?;
    let id = db::insert_tenant(
        &s.pool,
        req.name.trim(),
        &hash_api_key(&api_key),
        req.webhook_url.as_deref(),
        &sealed,
    )
    .await?;
    Ok(Json(CreatedTenant {
        id,
        api_key,
        webhook_secret,
    }))
}

async fn list_tenants(_: AdminAuth, State(s): State<AppState>) -> ApiResult<Vec<TenantView>> {
    Ok(Json(
        db::list_tenants(&s.pool)
            .await?
            .into_iter()
            .map(Into::into)
            .collect(),
    ))
}

async fn get_tenant(
    _: AdminAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<TenantView> {
    db::get_tenant(&s.pool, id)
        .await?
        .map(|t| Json(t.into()))
        .ok_or_else(ApiError::not_found)
}

#[derive(Deserialize)]
struct SetEnabled {
    enabled: bool,
}

async fn set_enabled(
    _: AdminAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<SetEnabled>,
) -> ApiResult<Value> {
    if db::set_tenant_enabled(&s.pool, id, req.enabled).await? {
        Ok(Json(serde_json::json!({ "enabled": req.enabled })))
    } else {
        Err(ApiError::not_found())
    }
}

async fn rotate_key(
    _: AdminAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    let api_key = new_api_key();
    if db::rotate_tenant_key(&s.pool, id, &hash_api_key(&api_key)).await? {
        Ok(Json(serde_json::json!({ "api_key": api_key })))
    } else {
        Err(ApiError::not_found())
    }
}

const METHODS: &[&str] = &[
    "lightning",
    "onchain",
    "stripe",
    "revolut",
    "paypal",
    "fixedfloat",
];

#[derive(Deserialize)]
struct CreateProvider {
    method: String,
    provider_type: String,
    name: String,
    config: Value,
    fee_rate: Option<f64>,
    fee_base: Option<u64>,
    fee_currency: Option<String>,
}

async fn create_provider(
    _: AdminAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<CreateProvider>,
) -> ApiResult<Value> {
    if !METHODS.contains(&req.method.as_str()) {
        return Err(ApiError::bad_request("unknown method"));
    }
    if req.name.trim().is_empty() || req.provider_type.trim().is_empty() {
        return Err(ApiError::bad_request("name and provider_type required"));
    }
    if req.fee_rate.is_some_and(|r| !(0.0..=100.0).contains(&r)) {
        return Err(ApiError::bad_request("fee_rate must be 0-100"));
    }
    if db::get_tenant(&s.pool, id).await?.is_none() {
        return Err(ApiError::not_found());
    }
    let sealed = s.sealer.seal(&serde_json::to_vec(&req.config).map_err(anyhow::Error::from)?)?;
    let provider_id = db::insert_provider_config(
        &s.pool,
        NewProviderConfig {
            tenant_id: id,
            method: &req.method,
            provider_type: &req.provider_type,
            name: req.name.trim(),
            sealed_config: &sealed,
            fee_rate: req.fee_rate,
            fee_base: req.fee_base,
            fee_currency: req.fee_currency.as_deref(),
        },
    )
    .await?;
    Ok(Json(serde_json::json!({ "id": provider_id })))
}

async fn list_providers(
    _: AdminAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<ProviderConfigRow>> {
    Ok(Json(db::list_provider_configs(&s.pool, id, false).await?))
}

async fn set_provider_enabled(
    _: AdminAuth,
    State(s): State<AppState>,
    Path((id, provider_id)): Path<(u64, u64)>,
    Json(req): Json<SetEnabled>,
) -> ApiResult<Value> {
    if db::set_provider_config_enabled(&s.pool, id, provider_id, req.enabled).await? {
        Ok(Json(serde_json::json!({ "enabled": req.enabled })))
    } else {
        Err(ApiError::not_found())
    }
}
