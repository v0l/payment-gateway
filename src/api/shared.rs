use super::{ApiError, ApiResult, AppState};
use crate::audit::{Action, Event, Origin};
use crate::db::{self, FeeOverrideRow, NewSharedProvider, SharedProviderRow, SharedUpdate};
use crate::shared::{MAX_FEE_BPS, NETWORKS};
use crate::validate;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Serialize)]
pub struct SharedProviderView {
    #[serde(flatten)]
    provider: SharedProviderRow,
    config: Value,
    overrides: Vec<FeeOverrideRow>,
}

#[derive(Deserialize)]
pub struct SharedInput {
    pub name: String,
    pub method: String,
    pub provider_type: String,
    pub network: String,
    pub config: Value,
    #[serde(default)]
    pub fee_bps: u32,
    #[serde(default)]
    pub fee_base_msat: u64,
    #[serde(default)]
    pub allow_untrusted: bool,
    #[serde(default = "yes")]
    pub allow_trusted: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
pub struct SharedPatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub fee_bps: Option<u32>,
    pub fee_base_msat: Option<u64>,
    pub allow_untrusted: Option<bool>,
    pub allow_trusted: Option<bool>,
    pub config: Option<Value>,
}

#[derive(Deserialize)]
pub struct OverrideInput {
    pub fee_bps: Option<u32>,
    pub fee_base_msat: Option<u64>,
}

#[derive(Deserialize)]
pub struct AllowNegative {
    pub allow: bool,
}

fn check_fee(bps: u32) -> Result<(), ApiError> {
    if bps > MAX_FEE_BPS {
        return Err(ApiError::bad_request("fee_bps must be 0-10000"));
    }
    Ok(())
}

async fn view(s: &AppState, p: SharedProviderRow) -> Result<SharedProviderView, ApiError> {
    let plain = s.service.sealer.open(&p.config).ok();
    let config = plain
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .map(|c| validate::redacted(&c))
        .unwrap_or(Value::Null);
    let overrides = db::list_fee_overrides(s.pool(), p.id).await?;
    Ok(SharedProviderView {
        provider: p,
        config,
        overrides,
    })
}

pub async fn list(s: &AppState) -> ApiResult<Vec<SharedProviderView>> {
    let mut out = vec![];
    for p in db::list_shared_providers(s.pool(), false).await? {
        out.push(view(s, p).await?);
    }
    Ok(Json(out))
}

pub async fn create(
    s: &AppState,
    ev: impl FnOnce(Action) -> Event,
    origin: &Origin,
    req: SharedInput,
) -> ApiResult<Value> {
    if !matches!(req.method.as_str(), "lightning" | "onchain") {
        return Err(ApiError::bad_request(
            "shared providers are lightning or onchain",
        ));
    }
    if req.provider_type == "xpub" {
        return Err(ApiError::bad_request(
            "xpub providers belong to one merchant and cannot be shared",
        ));
    }
    if !NETWORKS.contains(&req.network.as_str()) {
        return Err(ApiError::bad_request(format!(
            "network must be one of {NETWORKS:?}"
        )));
    }
    let name = req.name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(ApiError::bad_request("name must be 1-128 characters"));
    }
    check_fee(req.fee_bps)?;
    validate::provider_config(
        true,
        s.allow_mock,
        &req.method,
        &req.provider_type,
        &req.config,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let sealed = s
        .service
        .sealer
        .seal(&serde_json::to_vec(&req.config).map_err(anyhow::Error::from)?)?;
    let id = db::insert_shared_provider(
        s.pool(),
        &NewSharedProvider {
            name,
            method: &req.method,
            provider_type: &req.provider_type,
            network: &req.network,
            sealed_config: &sealed,
            fee_bps: req.fee_bps,
            fee_base_msat: req.fee_base_msat,
            allow_untrusted: req.allow_untrusted,
            allow_trusted: req.allow_trusted,
        },
    )
    .await?;
    ev(Action::SharedProviderCreated)
        .target("shared_provider", id)
        .details(json!({"method": req.method, "network": req.network, "name": name}))
        .record(s.pool(), origin)
        .await;
    Ok(Json(json!({"id": id})))
}

pub async fn update(
    s: &AppState,
    ev: impl FnOnce(Action) -> Event,
    origin: &Origin,
    id: u64,
    req: SharedPatch,
) -> ApiResult<SharedProviderView> {
    let row = db::get_shared_provider(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut changed: Vec<String> = vec![];
    if let Some(patch) = &req.config {
        let plain = s.service.sealer.open(&row.config)?;
        let old: Value = serde_json::from_slice(&plain).map_err(anyhow::Error::from)?;
        let merged = validate::merge_update(&old, patch);
        validate::provider_config(true, s.allow_mock, &row.method, &row.provider_type, &merged)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()))?;
        changed.extend(validate::changed_config_keys(&old, &merged));
        let sealed = s
            .service
            .sealer
            .seal(&serde_json::to_vec(&merged).map_err(anyhow::Error::from)?)?;
        db::update_shared_config_blob(s.pool(), id, &sealed).await?;
        s.service.registry.invalidate(crate::shared::shared_key(id));
    }
    let name = req
        .name
        .as_deref()
        .map(str::trim)
        .unwrap_or(&row.name)
        .to_string();
    if name.is_empty() || name.len() > 128 {
        return Err(ApiError::bad_request("name must be 1-128 characters"));
    }
    let next = SharedUpdate {
        name: &name,
        enabled: req.enabled.unwrap_or(row.enabled),
        fee_bps: req.fee_bps.unwrap_or(row.fee_bps),
        fee_base_msat: req.fee_base_msat.unwrap_or(row.fee_base_msat),
        allow_untrusted: req.allow_untrusted.unwrap_or(row.allow_untrusted),
        allow_trusted: req.allow_trusted.unwrap_or(row.allow_trusted),
    };
    check_fee(next.fee_bps)?;
    for (field, differs) in [
        ("name", next.name != row.name),
        ("enabled", next.enabled != row.enabled),
        ("fee_bps", next.fee_bps != row.fee_bps),
        ("fee_base_msat", next.fee_base_msat != row.fee_base_msat),
        (
            "allow_untrusted",
            next.allow_untrusted != row.allow_untrusted,
        ),
        ("allow_trusted", next.allow_trusted != row.allow_trusted),
    ] {
        if differs {
            changed.push(field.to_string());
        }
    }
    db::update_shared_provider(s.pool(), id, &next).await?;
    ev(Action::SharedProviderUpdated)
        .target("shared_provider", id)
        .details(json!({"changed": changed}))
        .record(s.pool(), origin)
        .await;
    let fresh = db::get_shared_provider(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(view(s, fresh).await?))
}

pub async fn set_override(
    s: &AppState,
    ev: impl FnOnce(Action) -> Event,
    origin: &Origin,
    id: u64,
    tenant_id: u64,
    req: OverrideInput,
) -> ApiResult<Value> {
    db::get_shared_provider(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    db::get_tenant(s.pool(), tenant_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if let Some(bps) = req.fee_bps {
        check_fee(bps)?;
    }
    db::set_fee_override(s.pool(), tenant_id, id, req.fee_bps, req.fee_base_msat).await?;
    ev(Action::SharedFeeOverridden)
        .tenant(tenant_id)
        .target("shared_provider", id)
        .details(json!({"fee_bps": req.fee_bps, "fee_base_msat": req.fee_base_msat}))
        .record(s.pool(), origin)
        .await;
    Ok(Json(json!({"ok": true})))
}

pub async fn allow_negative(
    s: &AppState,
    ev: impl FnOnce(Action) -> Event,
    origin: &Origin,
    tenant_id: u64,
    req: AllowNegative,
) -> ApiResult<Value> {
    db::get_tenant(s.pool(), tenant_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    s.service.set_allow_negative(tenant_id, req.allow).await?;
    ev(Action::AllowNegativeChanged)
        .tenant(tenant_id)
        .details(json!({"allow": req.allow}))
        .record(s.pool(), origin)
        .await;
    Ok(Json(json!({"allow": req.allow})))
}

pub async fn tenant_enable(
    s: &AppState,
    ev: impl FnOnce(Action) -> Event,
    origin: &Origin,
    tenant: &db::Tenant,
    id: u64,
    enabled: bool,
) -> ApiResult<Value> {
    if enabled {
        s.service.enable_shared(tenant, id).await?;
    } else {
        s.service.disable_shared(tenant, id).await?;
    }
    let action = if enabled {
        Action::SharedMethodEnabled
    } else {
        Action::SharedMethodDisabled
    };
    ev(action)
        .tenant(tenant.id)
        .target("shared_provider", id)
        .record(s.pool(), origin)
        .await;
    Ok(Json(json!({"enabled": enabled})))
}
