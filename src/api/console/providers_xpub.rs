use super::*;

async fn xpub_row(s: &AppState, id: u64, pid: u64) -> Result<ProviderConfigRow, ApiError> {
    let row = db::get_tenant_provider(s.pool(), id, pid)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if row.shared_provider_id.is_some() || row.provider_type != "xpub" {
        return Err(ApiError::not_found());
    }
    Ok(row)
}

pub(super) async fn xpub_status(
    sess: Session,
    State(s): State<AppState>,
    Path((id, pid)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    xpub_row(&s, id, pid).await?;
    let provider = s
        .service
        .registry
        .get(pid)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    let mut view = provider.describe().await?.ok_or_else(ApiError::not_found)?;
    let issued = db::xpub_issued(s.pool(), pid).await?;
    view["issued"] = issued
        .iter()
        .map(|a| json!({"index": a.idx, "address": a.address}))
        .collect();
    Ok(Json(view))
}

#[derive(Deserialize)]
pub(super) struct RescanInput {
    from: u32,
    count: u32,
}

pub(super) async fn xpub_rescan(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, pid)): Path<(u64, u64)>,
    Json(req): Json<RescanInput>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    xpub_row(&s, id, pid).await?;
    let provider = s
        .service
        .registry
        .get(pid)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    let message = provider
        .rescan(req.from, req.count)
        .await
        .map_err(|e| ApiError::bad_request(format!("{e:#}")))?;
    sess.event(Action::ProviderRescanned)
        .tenant(id)
        .target("provider", pid)
        .details(json!({"from": req.from, "count": req.count}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"message": message})))
}
