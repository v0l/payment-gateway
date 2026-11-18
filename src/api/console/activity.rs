use super::*;

pub(super) async fn project_activity(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<audit::AuditEvent>> {
    sess.project(&s, id, Role::Viewer).await?;
    Ok(Json(
        audit::list(s.pool(), &audit::Filter::from_query(Some(id), &q)).await?,
    ))
}

pub(super) async fn admin_audit(
    sess: Session,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<audit::AuditEvent>> {
    sess.operator()?;
    let tenant = q.get("tenant_id").and_then(|t| t.parse().ok());
    Ok(Json(
        audit::list(s.pool(), &audit::Filter::from_query(tenant, &q)).await?,
    ))
}
