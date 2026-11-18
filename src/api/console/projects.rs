use super::*;

#[derive(Serialize)]
pub(super) struct ProjectView {
    id: u64,
    name: String,
    role: Role,
    trusted: bool,
    require_2fa: bool,
    webhook_url: Option<String>,
    display_name: Option<String>,
    logo_url: Option<String>,
    accent_color: Option<String>,
    allowed_origins: Vec<String>,
    default_success_url: Option<String>,
    default_cancel_url: Option<String>,
    enabled: bool,
}

pub(super) fn project_view(t: Tenant, role: Role) -> ProjectView {
    let allowed_origins = t.origins();
    ProjectView {
        id: t.id,
        name: t.name,
        role,
        trusted: t.trusted,
        require_2fa: t.require_2fa,
        webhook_url: t.webhook_url,
        display_name: t.display_name,
        logo_url: t.logo_url,
        accent_color: t.accent_color,
        allowed_origins,
        default_success_url: t.default_success_url,
        default_cancel_url: t.default_cancel_url,
        enabled: t.enabled,
    }
}

pub(crate) fn settings_diff(before: &Tenant, after: &Tenant) -> [(&'static str, bool); 7] {
    [
        ("webhook_url", before.webhook_url != after.webhook_url),
        ("display_name", before.display_name != after.display_name),
        ("logo_url", before.logo_url != after.logo_url),
        ("accent_color", before.accent_color != after.accent_color),
        (
            "allowed_origins",
            before.allowed_origins != after.allowed_origins,
        ),
        (
            "default_success_url",
            before.default_success_url != after.default_success_url,
        ),
        (
            "default_cancel_url",
            before.default_cancel_url != after.default_cancel_url,
        ),
    ]
}

pub(super) async fn list_projects(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<Vec<accounts::ProjectRef>> {
    Ok(Json(
        accounts::projects_for_account(s.pool(), &sess.account).await?,
    ))
}

#[derive(Deserialize)]
pub(super) struct NewProject {
    name: String,
}

pub(super) async fn create_project(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(p): Json<NewProject>,
) -> ApiResult<Value> {
    let name = p.name.trim().to_string();
    if name.is_empty() || name.len() > 64 {
        return Err(ApiError::bad_request("name must be 1-64 characters"));
    }
    let owned = accounts::projects_for_account(s.pool(), &sess.account).await?;
    if !sess.account.is_operator && owned.len() >= s.max_projects {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            format!("project limit of {} reached", s.max_projects),
        ));
    }
    let key = new_api_key();
    let secret = new_webhook_secret();
    let sealed = s.service.sealer.seal(secret.as_bytes())?;
    let mut tx = s.pool().begin().await?;
    let id = accounts::insert_project(&mut *tx, &name, &hash_api_key(&key), &sealed).await?;
    db::set_default_key(&mut *tx, id, &hash_api_key(&key)).await?;
    accounts::add_member(&mut *tx, id, sess.account.id, Role::Owner).await?;
    tx.commit().await?;
    sess.event(Action::ProjectCreated)
        .tenant(id)
        .target("project", id)
        .details(json!({"name": name}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(
        json!({"id": id, "name": name, "api_key": key, "webhook_secret": secret}),
    ))
}

pub(super) async fn get_project(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<ProjectView> {
    let (t, role) = sess.project(&s, id, Role::Viewer).await?;
    Ok(Json(project_view(t, role)))
}

pub(super) async fn update_settings(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<SettingsInput>,
) -> ApiResult<ProjectView> {
    let (t, role) = sess.project(&s, id, Role::Admin).await?;
    let settings = validate::settings(t.trusted, req)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    s.service
        .check_webhook_endpoint(&t, settings.webhook_url.as_deref())?;
    db::update_tenant_settings(s.pool(), id, &settings).await?;
    let after = db::get_tenant(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    sess.event(Action::SettingsChanged)
        .tenant(id)
        .target("project", id)
        .details(audit::changed(&settings_diff(&t, &after)))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    let t = after;
    Ok(Json(project_view(t, role)))
}

pub(super) async fn rotate_key(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    let key = new_api_key();
    db::rotate_tenant_key(s.pool(), id, &hash_api_key(&key)).await?;
    sess.event(Action::KeyRotated)
        .tenant(id)
        .target("project", id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"api_key": key})))
}

pub(super) async fn rotate_webhook_secret(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    let secret = new_webhook_secret();
    let sealed = s.service.sealer.seal(secret.as_bytes())?;
    accounts::set_webhook_secret(s.pool(), id, &sealed).await?;
    db::set_default_endpoint_secret(s.pool(), id, &sealed).await?;
    sess.event(Action::WebhookSecretRotated)
        .tenant(id)
        .target("project", id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"webhook_secret": secret})))
}

#[derive(Serialize)]
pub(super) struct LimitsView {
    limits: crate::limits::Limits,
    defaults: crate::limits::Limits,
    usage: crate::limits::Usage,
    currency: &'static str,
}

async fn limits_view(s: &AppState, t: &Tenant) -> Result<LimitsView, ApiError> {
    Ok(LimitsView {
        limits: crate::limits::Limits::for_tenant(t),
        defaults: crate::limits::Limits::defaults(t.trusted),
        usage: s.service.usage(t).await?,
        currency: crate::limits::LIMIT_CURRENCY,
    })
}

pub(super) async fn get_limits(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<LimitsView> {
    let (t, _) = sess.project(&s, id, Role::Viewer).await?;
    Ok(Json(limits_view(&s, &t).await?))
}

pub(super) async fn set_limits(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<crate::limits::LimitsInput>,
) -> ApiResult<LimitsView> {
    sess.operator(&s)?;
    let t = db::get_tenant(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let updated = input.apply(t.limits);
    db::set_tenant_limits(s.pool(), id, &updated).await?;
    let after = db::get_tenant(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    sess.event(Action::LimitsChanged)
        .tenant(id)
        .target("project", id)
        .details(audit::changed(&crate::limits::diff(
            &t.limits,
            &after.limits,
        )))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(limits_view(&s, &after).await?))
}
