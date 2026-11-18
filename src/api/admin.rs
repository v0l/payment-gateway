use super::{ApiError, ApiResult, AppState, bearer};
use crate::audit::{self, Action, Actor, Event, Origin};
use crate::crypto::{hash_api_key, new_api_key, new_webhook_secret};
use crate::db::{self, DeliveryRow, NewProviderConfig, ProviderConfigRow};
use crate::model::METHODS;
use crate::validate::{self, SettingsInput};
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::HeaderMap;
use axum::http::request::Parts;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
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
        .route("/tenants/{id}", get(get_tenant))
        .route("/tenants/{id}/settings", post(update_settings))
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
        .route("/invites", post(create_invite))
        .route("/audit", get(list_audit))
        .route("/deliveries", get(list_deliveries))
        .route("/deliveries/{id}/replay", post(replay_delivery))
}

#[derive(Serialize)]
struct TenantView {
    id: u64,
    name: String,
    webhook_url: Option<String>,
    enabled: bool,
    display_name: Option<String>,
    logo_url: Option<String>,
    accent_color: Option<String>,
    allowed_origins: Vec<String>,
    default_success_url: Option<String>,
    default_cancel_url: Option<String>,
}

impl From<db::Tenant> for TenantView {
    fn from(t: db::Tenant) -> Self {
        let allowed_origins = t.origins();
        Self {
            id: t.id,
            name: t.name,
            webhook_url: t.webhook_url,
            enabled: t.enabled,
            display_name: t.display_name,
            logo_url: t.logo_url,
            accent_color: t.accent_color,
            allowed_origins,
            default_success_url: t.default_success_url,
            default_cancel_url: t.default_cancel_url,
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

fn valid_url(url: &str) -> bool {
    url::Url::parse(url)
        .map(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
        .unwrap_or(false)
}

async fn create_tenant(
    _: AdminAuth,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<CreateTenant>,
) -> ApiResult<CreatedTenant> {
    if req.name.trim().is_empty() {
        return Err(ApiError::bad_request("name required"));
    }
    if req.webhook_url.as_deref().is_some_and(|u| !valid_url(u)) {
        return Err(ApiError::bad_request("invalid webhook_url"));
    }
    let api_key = new_api_key();
    let webhook_secret = new_webhook_secret();
    let sealed = s.service.sealer.seal(webhook_secret.as_bytes())?;
    let id = db::insert_tenant(
        s.pool(),
        req.name.trim(),
        &hash_api_key(&api_key),
        req.webhook_url.as_deref(),
        &sealed,
    )
    .await?;
    Event::new(Action::ProjectCreated, Actor::AdminToken)
        .tenant(id)
        .target("project", id)
        .details(json!({"name": req.name.trim()}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(CreatedTenant {
        id,
        api_key,
        webhook_secret,
    }))
}

async fn list_tenants(_: AdminAuth, State(s): State<AppState>) -> ApiResult<Vec<TenantView>> {
    Ok(Json(
        db::list_tenants(s.pool())
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
    db::get_tenant(s.pool(), id)
        .await?
        .map(|t| Json(t.into()))
        .ok_or_else(ApiError::not_found)
}

async fn update_settings(
    _: AdminAuth,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<SettingsInput>,
) -> ApiResult<TenantView> {
    let tenant = db::get_tenant(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let settings = validate::settings(tenant.trusted, req)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    s.service
        .check_webhook_endpoint(&tenant, settings.webhook_url.as_deref())?;
    if !db::update_tenant_settings(s.pool(), id, &settings).await? {
        return Err(ApiError::not_found());
    }
    let after = db::get_tenant(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Event::new(Action::SettingsChanged, Actor::AdminToken)
        .tenant(id)
        .target("project", id)
        .details(audit::changed(&crate::api::settings_diff(&tenant, &after)))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(after.into()))
}

#[derive(Deserialize)]
struct SetEnabled {
    enabled: bool,
}

async fn set_enabled(
    _: AdminAuth,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<SetEnabled>,
) -> ApiResult<Value> {
    if db::set_tenant_enabled(s.pool(), id, req.enabled).await? {
        Event::new(Action::ProjectEnabledChanged, Actor::AdminToken)
            .tenant(id)
            .target("project", id)
            .details(json!({"enabled": req.enabled}))
            .record(s.pool(), &Origin::from_headers(&headers))
            .await;
        Ok(Json(serde_json::json!({ "enabled": req.enabled })))
    } else {
        Err(ApiError::not_found())
    }
}

async fn rotate_key(
    _: AdminAuth,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    let api_key = new_api_key();
    if db::rotate_tenant_key(s.pool(), id, &hash_api_key(&api_key)).await? {
        Event::new(Action::KeyRotated, Actor::AdminToken)
            .tenant(id)
            .target("project", id)
            .record(s.pool(), &Origin::from_headers(&headers))
            .await;
        Ok(Json(serde_json::json!({ "api_key": api_key })))
    } else {
        Err(ApiError::not_found())
    }
}

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
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<CreateProvider>,
) -> ApiResult<Value> {
    if !METHODS.contains(&req.method.as_str()) {
        return Err(ApiError::bad_request("unknown method"));
    }
    if req.name.trim().is_empty() || req.provider_type.trim().is_empty() {
        return Err(ApiError::bad_request("name and provider_type required"));
    }
    validate::provider_config(
        true,
        s.allow_mock,
        &req.method,
        &req.provider_type,
        &req.config,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    if req.fee_rate.is_some_and(|r| !(0.0..=100.0).contains(&r)) {
        return Err(ApiError::bad_request("fee_rate must be 0-100"));
    }
    let tenant = db::get_tenant(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    s.service.check_provider_slot(&tenant).await?;
    let sealed = s
        .service
        .sealer
        .seal(&serde_json::to_vec(&req.config).map_err(anyhow::Error::from)?)?;
    let provider_id = db::insert_provider_config(
        s.pool(),
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
    Event::new(Action::ProviderCreated, Actor::AdminToken)
        .tenant(id)
        .target("provider", provider_id)
        .details(json!({"method": req.method, "provider_type": req.provider_type, "name": req.name.trim()}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(serde_json::json!({
        "id": provider_id,
        "hook_url": s.service.hook_url(id, provider_id),
    })))
}

async fn list_providers(
    _: AdminAuth,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<ProviderConfigRow>> {
    Ok(Json(db::list_provider_configs(s.pool(), id, false).await?))
}

async fn set_provider_enabled(
    _: AdminAuth,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, provider_id)): Path<(u64, u64)>,
    Json(req): Json<SetEnabled>,
) -> ApiResult<Value> {
    if db::set_provider_config_enabled(s.pool(), id, provider_id, req.enabled).await? {
        s.service.registry.invalidate(provider_id);
        let action = if req.enabled {
            Action::ProviderEnabled
        } else {
            Action::ProviderDisabled
        };
        Event::new(action, Actor::AdminToken)
            .tenant(id)
            .target("provider", provider_id)
            .record(s.pool(), &Origin::from_headers(&headers))
            .await;
        Ok(Json(serde_json::json!({ "enabled": req.enabled })))
    } else {
        Err(ApiError::not_found())
    }
}

async fn list_deliveries(
    _: AdminAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<DeliveryRow>> {
    let tenant = q.get("tenant_id").and_then(|t| t.parse().ok());
    let undelivered = q
        .get("undelivered")
        .is_some_and(|v| v == "1" || v == "true");
    Ok(Json(
        db::list_deliveries(s.pool(), tenant, undelivered).await?,
    ))
}

async fn replay_delivery(
    _: AdminAuth,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    if db::replay_delivery(s.pool(), id, chrono::Utc::now()).await? {
        Event::new(Action::DeliveryReplayed, Actor::AdminToken)
            .target("delivery", id)
            .record(s.pool(), &Origin::from_headers(&headers))
            .await;
        Ok(Json(serde_json::json!({ "queued": true })))
    } else {
        Err(ApiError::not_found())
    }
}

#[derive(Deserialize)]
struct InviteInput {
    email: String,
    #[serde(default)]
    operator: bool,
}

async fn create_invite(
    _: AdminAuth,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(i): Json<InviteInput>,
) -> ApiResult<Value> {
    let email = crate::accounts::normalize_email(&i.email)
        .ok_or_else(|| ApiError::bad_request("invalid email"))?;
    if crate::accounts::account_by_email(s.pool(), &email)
        .await?
        .is_some()
    {
        return Err(ApiError::new(
            axum::http::StatusCode::CONFLICT,
            "an account with this email already exists".into(),
        ));
    }
    let token = crate::accounts::insert_invite(
        s.pool(),
        &crate::accounts::NewInvite {
            email: &email,
            tenant_id: None,
            role: None,
            operator: i.operator,
            created_by: None,
        },
    )
    .await?;
    let url = format!("{}/console/invite/{token}", s.service.public_url);
    s.service.send_mail(crate::mailer::templates::invite(
        &email,
        &url,
        None,
        crate::accounts::INVITE_DAYS,
    ));
    Event::new(Action::InviteCreated, Actor::AdminToken)
        .target("invite", &email)
        .details(json!({"email": email, "operator": i.operator}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(serde_json::json!({
        "invite_url": url,
        "email": email,
        "expires_in_days": crate::accounts::INVITE_DAYS,
    })))
}

async fn list_audit(
    _: AdminAuth,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<audit::AuditEvent>> {
    let tenant = q.get("tenant_id").and_then(|t| t.parse().ok());
    Ok(Json(
        audit::list(s.pool(), &audit::Filter::from_query(tenant, &q)).await?,
    ))
}
