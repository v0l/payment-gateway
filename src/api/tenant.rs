use super::{ApiError, ApiResult, AppState, bearer};
use crate::crypto::hash_api_key;
use crate::db::{self, Tenant};
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

pub struct TenantAuth(pub Tenant);

impl FromRequestParts<AppState> for TenantAuth {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let key = bearer(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let tenant = db::tenant_by_key_hash(&state.pool, &hash_api_key(key))
            .await?
            .filter(|t| t.enabled)
            .ok_or_else(ApiError::unauthorized)?;
        Ok(TenantAuth(tenant))
    }
}

pub fn router() -> Router<AppState> {
    Router::new().route("/methods", get(list_methods))
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
        db::list_provider_configs(&s.pool, t.id, true)
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
