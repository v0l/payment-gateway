use super::*;

#[derive(Serialize)]
pub(super) struct ProviderView {
    id: u64,
    method: String,
    provider_type: String,
    name: String,
    pub(super) enabled: bool,
    fee_rate: Option<f64>,
    fee_base: Option<u64>,
    fee_currency: Option<String>,
    config: Value,
    hook_url: String,
}

pub(super) fn provider_view(s: &AppState, tenant: u64, p: &ProviderConfigRow) -> ProviderView {
    let config = s
        .service
        .registry
        .open_config(p)
        .map(|c| validate::redacted(&c))
        .unwrap_or(Value::Null);
    ProviderView {
        id: p.id,
        method: p.method.clone(),
        provider_type: p.provider_type.clone(),
        name: p.name.clone(),
        enabled: p.enabled,
        fee_rate: p.fee_rate,
        fee_base: p.fee_base,
        fee_currency: p.fee_currency.clone(),
        config,
        hook_url: s.service.hook_url(tenant, p.id),
    }
}

pub(super) async fn list_providers(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<ProviderView>> {
    sess.project(&s, id, Role::Viewer).await?;
    Ok(Json(
        db::list_provider_configs(s.pool(), id, false)
            .await?
            .iter()
            .map(|p| provider_view(&s, id, p))
            .collect(),
    ))
}

#[derive(Deserialize)]
pub(super) struct ProviderInput {
    method: String,
    provider_type: String,
    name: String,
    config: Value,
    fee_rate: Option<f64>,
    fee_base: Option<u64>,
    fee_currency: Option<String>,
}

pub(super) fn check_fees(
    rate: Option<f64>,
    currency: &Option<String>,
    base: Option<u64>,
) -> Result<(), ApiError> {
    if rate.is_some_and(|r| !(0.0..=100.0).contains(&r) || r.is_nan()) {
        return Err(ApiError::bad_request("fee_rate must be 0-100"));
    }
    if base.is_some()
        && currency
            .as_deref()
            .map(|c| crate::rates::unit(c).is_err())
            .unwrap_or(true)
    {
        return Err(ApiError::bad_request(
            "fee_currency must be a supported fiat or BTC currency",
        ));
    }
    Ok(())
}

pub(super) async fn create_provider(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<ProviderInput>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    if !METHODS.contains(&req.method.as_str()) {
        return Err(ApiError::bad_request("unknown method"));
    }
    let name = req.name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(ApiError::bad_request("name must be 1-128 characters"));
    }
    check_fees(req.fee_rate, &req.fee_currency, req.fee_base)?;
    validate::provider_config(
        t.trusted,
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
    let pid = db::insert_provider_config(
        s.pool(),
        NewProviderConfig {
            tenant_id: id,
            method: &req.method,
            provider_type: &req.provider_type,
            name,
            sealed_config: &sealed,
            fee_rate: req.fee_rate,
            fee_base: req.fee_base,
            fee_currency: req.fee_currency.as_deref(),
        },
    )
    .await?;
    sess.event(Action::ProviderCreated)
        .tenant(id)
        .target("provider", pid)
        .details(json!({"method": req.method, "provider_type": req.provider_type, "name": name}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(
        json!({"id": pid, "hook_url": s.service.hook_url(id, pid)}),
    ))
}

pub(super) async fn update_provider(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, pid)): Path<(u64, u64)>,
    Json(req): Json<ProviderInput>,
) -> ApiResult<ProviderView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let row = db::get_tenant_provider(s.pool(), id, pid)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if req.method != row.method || req.provider_type != row.provider_type {
        return Err(ApiError::bad_request(
            "method and provider_type cannot change",
        ));
    }
    check_fees(req.fee_rate, &req.fee_currency, req.fee_base)?;
    let old = s.service.registry.open_config(&row)?;
    let merged = validate::merge_update(&old, &req.config);
    validate::provider_config(
        t.trusted,
        s.allow_mock,
        &row.method,
        &row.provider_type,
        &merged,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    s.service.registry.persist_config(pid, &merged).await?;
    db::update_provider_fees(
        s.pool(),
        pid,
        req.name.trim(),
        req.fee_rate,
        req.fee_base,
        req.fee_currency.as_deref(),
    )
    .await?;
    s.service.registry.invalidate(pid);
    let updated = db::get_tenant_provider(s.pool(), id, pid)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let mut fields = validate::changed_config_keys(&old, &merged);
    for (name, changed) in [
        ("name", row.name != updated.name),
        ("fee_rate", row.fee_rate != updated.fee_rate),
        ("fee_base", row.fee_base != updated.fee_base),
        ("fee_currency", row.fee_currency != updated.fee_currency),
    ] {
        if changed {
            fields.push(name.to_string());
        }
    }
    sess.event(Action::ProviderUpdated)
        .tenant(id)
        .target("provider", pid)
        .details(json!({"changed": fields}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    let row = updated;
    Ok(Json(provider_view(&s, id, &row)))
}

#[derive(Deserialize)]
pub(super) struct Enabled {
    pub(super) enabled: bool,
}

pub(super) async fn set_provider_enabled(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, pid)): Path<(u64, u64)>,
    Json(e): Json<Enabled>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    if !db::set_provider_config_enabled(s.pool(), id, pid, e.enabled).await? {
        return Err(ApiError::not_found());
    }
    s.service.registry.invalidate(pid);
    let action = if e.enabled {
        Action::ProviderEnabled
    } else {
        Action::ProviderDisabled
    };
    sess.event(action)
        .tenant(id)
        .target("provider", pid)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"enabled": e.enabled})))
}

pub(super) async fn remove_provider(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, pid)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    let removed = db::get_tenant_provider(s.pool(), id, pid)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if db::provider_attempt_count(s.pool(), pid).await? > 0 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "this provider has payment history, disable it instead".into(),
        ));
    }
    db::delete_provider(s.pool(), id, pid).await?;
    s.service.registry.invalidate(pid);
    sess.event(Action::ProviderDeleted)
        .tenant(id)
        .target("provider", pid)
        .details(json!({"method": removed.method, "provider_type": removed.provider_type, "name": removed.name}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"deleted": true})))
}
