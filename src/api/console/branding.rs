use super::*;
use crate::branding::Branding;

#[derive(Serialize)]
pub(super) struct BrandingView {
    #[serde(flatten)]
    branding: Branding,
    can_hide_powered_by: bool,
}

pub(super) async fn get_branding(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<BrandingView> {
    sess.project(&s, id, Role::Viewer).await?;
    Ok(Json(BrandingView {
        branding: db::get_branding(s.pool(), id).await?,
        can_hide_powered_by: sess.account.is_operator,
    }))
}

pub(super) async fn set_branding(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<Branding>,
) -> ApiResult<BrandingView> {
    sess.project(&s, id, Role::Admin).await?;
    let before = db::get_branding(s.pool(), id).await?;
    let mut next = req
        .validated()
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    if !sess.account.is_operator {
        next.show_powered_by = before.show_powered_by;
    }
    db::set_branding(s.pool(), id, &next).await?;
    sess.event(Action::SettingsChanged)
        .tenant(id)
        .target("project", id)
        .details(json!({"changed": ["branding"]}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(BrandingView {
        branding: next,
        can_hide_powered_by: sess.account.is_operator,
    }))
}
