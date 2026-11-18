use super::*;
use crate::db::EndpointRow;

#[derive(Serialize)]
pub(super) struct EndpointView {
    id: u64,
    url: String,
    description: Option<String>,
    events: Vec<String>,
    enabled: bool,
    is_default: bool,
    created: DateTime<Utc>,
    delivered: i64,
    failed: i64,
    pending: i64,
}

fn view(e: EndpointRow, stats: &[db::EndpointStats]) -> EndpointView {
    let st = stats.iter().find(|s| s.endpoint_id == Some(e.id));
    EndpointView {
        id: e.id,
        url: e.url,
        description: e.description,
        events: split_events(e.events.as_deref()),
        enabled: e.enabled,
        is_default: e.is_default,
        created: e.created,
        delivered: st.map_or(0, |s| s.delivered),
        failed: st.map_or(0, |s| s.failed),
        pending: st.map_or(0, |s| s.pending),
    }
}

fn split_events(raw: Option<&str>) -> Vec<String> {
    raw.unwrap_or("")
        .split(',')
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .collect()
}

fn clean_events(events: &[String]) -> Result<Option<String>, ApiError> {
    let mut out = vec![];
    for e in events {
        let e = e.trim();
        let valid = !e.is_empty()
            && e.len() <= 48
            && e.chars()
                .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_' || c == '*');
        if !valid {
            return Err(
                ApiError::bad_request(format!("invalid event filter {e}")).with_field("events")
            );
        }
        if !out.contains(&e) {
            out.push(e);
        }
    }
    Ok((!out.is_empty()).then(|| out.join(",")))
}

async fn checked_url(trusted: bool, url: &str) -> Result<(), ApiError> {
    if !validate::valid_url(url) || url.len() > 512 {
        return Err(ApiError::bad_request("invalid url").with_field("url"));
    }
    if !trusted {
        crate::ssrf::require_https(url)
            .map_err(|e| ApiError::bad_request(e.to_string()).with_field("url"))?;
        crate::ssrf::resolve_public(url)
            .await
            .map_err(|e| ApiError::bad_request(e.to_string()).with_field("url"))?;
    }
    Ok(())
}

pub(super) async fn list_endpoints(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<EndpointView>> {
    sess.project(&s, id, Role::Viewer).await?;
    let stats = db::endpoint_stats(s.pool(), id).await?;
    Ok(Json(
        db::list_endpoints(s.pool(), id)
            .await?
            .into_iter()
            .map(|e| view(e, &stats))
            .collect(),
    ))
}

#[derive(Deserialize)]
pub(super) struct NewEndpoint {
    url: String,
    description: Option<String>,
    #[serde(default)]
    events: Vec<String>,
}

pub(super) async fn create_endpoint(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(req): Json<NewEndpoint>,
) -> ApiResult<Value> {
    let (tenant, _) = sess.project(&s, id, Role::Admin).await?;
    checked_url(tenant.trusted, &req.url).await?;
    s.service.check_new_endpoint(&tenant).await?;
    let events = clean_events(&req.events)?;
    let secret = new_webhook_secret();
    let sealed = s.service.sealer.seal(secret.as_bytes())?;
    let endpoint_id = db::insert_endpoint(
        s.pool(),
        id,
        &req.url,
        &sealed,
        req.description.as_deref(),
        events.as_deref(),
    )
    .await?;
    sess.event(Action::WebhookEndpointCreated)
        .tenant(id)
        .target("webhook_endpoint", endpoint_id)
        .details(json!({"url": req.url, "events": events}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(
        json!({"id": endpoint_id, "url": req.url, "events": split_events(events.as_deref()), "enabled": true, "secret": secret}),
    ))
}

#[derive(Deserialize)]
pub(super) struct EndpointUpdate {
    url: Option<String>,
    description: Option<String>,
    events: Option<Vec<String>>,
    enabled: Option<bool>,
}

pub(super) async fn update_endpoint(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, endpoint_id)): Path<(u64, u64)>,
    Json(req): Json<EndpointUpdate>,
) -> ApiResult<Value> {
    let (tenant, _) = sess.project(&s, id, Role::Admin).await?;
    let mut e = db::get_endpoint(s.pool(), id, endpoint_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if let Some(url) = req.url {
        checked_url(tenant.trusted, &url).await?;
        e.url = url;
    }
    if let Some(d) = req.description {
        e.description = Some(d).filter(|d| !d.is_empty());
    }
    if let Some(events) = &req.events {
        e.events = clean_events(events)?;
    }
    if let Some(on) = req.enabled {
        e.enabled = on;
    }
    db::update_endpoint(s.pool(), id, &e).await?;
    sess.event(Action::WebhookEndpointUpdated)
        .tenant(id)
        .target("webhook_endpoint", endpoint_id)
        .details(json!({"url": e.url, "events": e.events, "enabled": e.enabled}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"updated": true})))
}

pub(super) async fn delete_endpoint(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, endpoint_id)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    if !db::delete_endpoint(s.pool(), id, endpoint_id).await? {
        return Err(ApiError::not_found());
    }
    sess.event(Action::WebhookEndpointDeleted)
        .tenant(id)
        .target("webhook_endpoint", endpoint_id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"deleted": true})))
}

pub(super) async fn rotate_endpoint_secret(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, endpoint_id)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    let secret = new_webhook_secret();
    let sealed = s.service.sealer.seal(secret.as_bytes())?;
    let endpoint = db::get_endpoint(s.pool(), id, endpoint_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    db::set_endpoint_secret(s.pool(), id, endpoint_id, &sealed).await?;
    if endpoint.is_default {
        crate::accounts::set_webhook_secret(s.pool(), id, &sealed).await?;
    }
    sess.event(Action::WebhookEndpointSecretRotated)
        .tenant(id)
        .target("webhook_endpoint", endpoint_id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"secret": secret})))
}

pub(super) async fn test_endpoint(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, endpoint_id)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    let (tenant, _) = sess.project(&s, id, Role::Admin).await?;
    let e = db::get_endpoint(s.pool(), id, endpoint_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let outcome = s.service.send_test_event(&tenant, &e).await;
    sess.event(Action::WebhookTestSent)
        .tenant(id)
        .target("webhook_endpoint", endpoint_id)
        .details(json!({"ok": outcome.is_ok()}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(match outcome {
        Ok(event_id) => json!({"ok": true, "event_id": event_id}),
        Err(err) => json!({"ok": false, "error": err.to_string()}),
    }))
}
