use super::*;

#[derive(Serialize)]
pub(super) struct ProjectView {
    id: u64,
    name: String,
    role: Role,
    trusted: bool,
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
    Json(p): Json<NewProject>,
) -> ApiResult<Value> {
    let name = p.name.trim().to_string();
    if name.is_empty() || name.len() > 64 {
        return Err(ApiError::bad_request("name must be 1-64 characters"));
    }
    let owned = accounts::projects_for_account(s.pool(), &sess.account).await?;
    if !sess.account.is_operator && owned.len() >= s.max_projects {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            format!("project limit of {} reached", s.max_projects),
        ));
    }
    let key = new_api_key();
    let secret = new_webhook_secret();
    let sealed = s.service.sealer.seal(secret.as_bytes())?;
    let mut tx = s.pool().begin().await?;
    let id = accounts::insert_project(&mut *tx, &name, &hash_api_key(&key), &sealed).await?;
    accounts::add_member(&mut *tx, id, sess.account.id, Role::Owner).await?;
    tx.commit().await?;
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
    Path(id): Path<u64>,
    Json(req): Json<SettingsInput>,
) -> ApiResult<ProjectView> {
    let (t, role) = sess.project(&s, id, Role::Admin).await?;
    let settings = validate::settings(t.trusted, req)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    db::update_tenant_settings(s.pool(), id, &settings).await?;
    let t = db::get_tenant(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(project_view(t, role)))
}

pub(super) async fn rotate_key(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    let key = new_api_key();
    db::rotate_tenant_key(s.pool(), id, &hash_api_key(&key)).await?;
    Ok(Json(json!({"api_key": key})))
}

pub(super) async fn rotate_webhook_secret(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    let secret = new_webhook_secret();
    accounts::set_webhook_secret(s.pool(), id, &s.service.sealer.seal(secret.as_bytes())?).await?;
    Ok(Json(json!({"webhook_secret": secret})))
}
