use super::*;

pub(super) async fn tenant_overview(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    let since = Utc::now() - Duration::days(30);
    Ok(Json(
        json!({"tenants": db::tenant_overviews(s.pool(), since).await?}),
    ))
}

pub(super) async fn support_view(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    let tenant = db::get_tenant(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    sess.event(Action::SupportViewed)
        .tenant(id)
        .target("project", id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    let since = Utc::now() - Duration::days(30);
    let filter = db::SearchFilter::default();
    let recent: Vec<Value> = db::search_payments(s.pool(), id, &filter, 20)
        .await?
        .iter()
        .map(|p| json!({"id": hex::encode(&p.id), "reference": p.reference, "status": p.status, "currency": p.currency, "amount": p.amount, "created": p.created}))
        .collect();
    let providers: Vec<Value> = db::list_provider_configs(s.pool(), id, false)
        .await?
        .iter()
        .map(|p| json!({"id": p.id, "method": p.method, "type": p.provider_type, "name": p.name, "enabled": p.enabled}))
        .collect();
    let endpoints = db::endpoint_health(s.pool(), id, since).await?;
    Ok(Json(json!({
        "read_only": true,
        "project": {"id": tenant.id, "name": tenant.name, "enabled": tenant.enabled, "trusted": tenant.trusted},
        "stats": db::dashboard_stats(s.pool(), id, since).await?,
        "recent_payments": recent,
        "providers": providers,
        "webhook_endpoints": endpoints,
        "members": crate::accounts::list_members(s.pool(), id).await?.len(),
    })))
}

pub(super) async fn global_search(
    sess: Session,
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    let term = q.get("q").map(|v| v.trim()).unwrap_or("");
    if term.len() < 4 || term.len() > 200 {
        return Err(ApiError::bad_request("search needs 4 to 200 characters").with_field("q"));
    }
    Ok(Json(json!({"hits": db::global_search(s.pool(), term, 50).await?})))
}

pub(super) async fn platform_health(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    let (pending, oldest) = db::pending_delivery_stats(s.pool(), Utc::now())
        .await?
        .unwrap_or((0, 0));
    let since = Utc::now() - Duration::hours(24);
    let abandoned: i64 = sqlx::query_scalar(
        "select count(*) from webhook_delivery where abandoned_at is not null and created >= ?",
    )
    .bind(since)
    .fetch_one(s.pool())
    .await?;
    let pending_payments: i64 =
        sqlx::query_scalar("select count(*) from payment where status = 'pending'")
            .fetch_one(s.pool())
            .await?;
    let tenants = db::list_tenants(s.pool()).await?;
    Ok(Json(json!({
        "tenants": tenants.len(),
        "tenants_disabled": tenants.iter().filter(|t| !t.enabled).count(),
        "pending_payments": pending_payments,
        "pending_deliveries": pending,
        "oldest_pending_delivery_seconds": oldest,
        "abandoned_deliveries_24h": abandoned,
        "uptime_seconds": s.ops.started.elapsed().as_secs(),
    })))
}
