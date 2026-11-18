use super::*;
use crate::api::shared as api;
use crate::service::SharedChoice;

pub(super) async fn project_shared(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<SharedChoice>> {
    let (tenant, _) = sess.project(&s, id, Role::Viewer).await?;
    Ok(Json(s.service.shared_choices(&tenant).await?))
}

pub(super) async fn project_shared_enabled(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, shared)): Path<(u64, u64)>,
    Json(req): Json<Enabled>,
) -> ApiResult<Value> {
    let (tenant, _) = sess.project(&s, id, Role::Admin).await?;
    api::tenant_enable(
        &s,
        |a| sess.event(a),
        &Origin::from_headers(&headers),
        &tenant,
        shared,
        req.enabled,
    )
    .await
}

pub(super) async fn admin_shared_list(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<Vec<api::SharedProviderView>> {
    sess.operator(&s)?;
    api::list(&s).await
}

pub(super) async fn admin_shared_create(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<api::SharedInput>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    api::create(&s, |a| sess.event(a), &Origin::from_headers(&headers), req).await
}

pub(super) async fn admin_shared_update(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<api::SharedPatch>,
) -> ApiResult<api::SharedProviderView> {
    sess.operator(&s)?;
    api::update(
        &s,
        |a| sess.event(a),
        &Origin::from_headers(&headers),
        id,
        req,
    )
    .await
}

pub(super) async fn admin_shared_override(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, tenant)): Path<(u64, u64)>,
    Json(req): Json<api::OverrideInput>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    api::set_override(
        &s,
        |a| sess.event(a),
        &Origin::from_headers(&headers),
        id,
        tenant,
        req,
    )
    .await
}

pub(super) async fn admin_allow_negative(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<api::AllowNegative>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    api::allow_negative(
        &s,
        |a| sess.event(a),
        &Origin::from_headers(&headers),
        id,
        req,
    )
    .await
}
