use super::*;

pub(super) async fn dashboard(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let days = match q.get("days").map(String::as_str) {
        None | Some("30") => 30,
        Some("7") => 7,
        Some("90") => 90,
        Some(_) => {
            return Err(ApiError::bad_request("days must be 7, 30 or 90").with_field("days"));
        }
    };
    let since = Utc::now() - Duration::days(days);
    let stats = db::dashboard_stats(s.pool(), id, since).await?;
    let mut out = serde_json::to_value(stats).map_err(anyhow::Error::from)?;
    out["days"] = json!(days);
    Ok(Json(out))
}
