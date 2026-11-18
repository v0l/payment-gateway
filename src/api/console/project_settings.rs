use super::*;
use crate::crypto::{hash_api_key, new_webhook_secret};
use subtle::ConstantTimeEq;

pub const COOLING_OFF_HOURS: i64 = 24;

pub(super) async fn get_settings(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let settings = db::project_settings(s.pool(), id).await?;
    let mut out = serde_json::to_value(settings).map_err(anyhow::Error::from)?;
    out["cooling_off_hours"] = json!(COOLING_OFF_HOURS);
    Ok(Json(out))
}

#[derive(Deserialize)]
pub(super) struct LegalInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    vat_id: Option<String>,
    #[serde(default)]
    email: Option<String>,
}

fn clean(value: Option<String>, field: &str, max: usize) -> Result<Option<String>, ApiError> {
    let Some(v) = value
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
    else {
        return Ok(None);
    };
    if v.chars().count() > max || v.chars().any(|c| c.is_control() && c != '\n') {
        return Err(ApiError::bad_request(format!(
            "{field} is too long or has control characters"
        ))
        .with_field(field));
    }
    Ok(Some(v))
}

pub(super) async fn set_legal(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<LegalInput>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    let name = clean(input.name, "name", 128)?;
    let address = clean(input.address, "address", 512)?;
    let vat = clean(input.vat_id, "vat_id", 64)?;
    let email = clean(input.email, "email", 255)?;
    if let Some(e) = &email
        && (!e.contains('@') || e.contains(char::is_whitespace))
    {
        return Err(ApiError::bad_request("email is not valid").with_field("email"));
    }
    db::set_legal_details(
        s.pool(),
        id,
        name.as_deref(),
        address.as_deref(),
        vat.as_deref(),
        email.as_deref(),
    )
    .await?;
    sess.event(Action::LegalDetailsChanged)
        .tenant(id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(
        serde_json::to_value(db::project_settings(s.pool(), id).await?)
            .map_err(anyhow::Error::from)?,
    ))
}

#[derive(Deserialize)]
pub(super) struct NotifyInput {
    webhook_abandoned: bool,
    provider_failing: bool,
    refund_failed: bool,
}

pub(super) async fn set_project_notifications(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<NotifyInput>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    db::set_notification_prefs(
        s.pool(),
        id,
        input.webhook_abandoned,
        input.provider_failing,
        input.refund_failed,
    )
    .await?;
    sess.event(Action::NotificationPrefsChanged)
        .tenant(id)
        .details(json!({"webhook_abandoned": input.webhook_abandoned, "provider_failing": input.provider_failing, "refund_failed": input.refund_failed}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(
        serde_json::to_value(db::project_settings(s.pool(), id).await?)
            .map_err(anyhow::Error::from)?,
    ))
}

#[derive(Deserialize)]
pub(super) struct DisableInput {
    enabled: bool,
}

pub(super) async fn set_enabled(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<DisableInput>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    db::set_tenant_enabled(s.pool(), id, input.enabled).await?;
    sess.event(Action::ProjectEnabledChanged)
        .tenant(id)
        .details(json!({"enabled": input.enabled}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"enabled": input.enabled})))
}

async fn deletion_blockers(s: &AppState, id: u64) -> Result<Vec<&'static str>, ApiError> {
    let mut blockers = vec![];
    if db::tenant_has_pending(s.pool(), id).await? {
        blockers.push("pending payments");
    }
    if db::nonzero_balance(s.pool(), id).await? {
        blockers.push("a non-zero balance");
    }
    Ok(blockers)
}

fn blocked(blockers: &[&str]) -> ApiError {
    ApiError::new(
        StatusCode::CONFLICT,
        format!(
            "project cannot be deleted while it has {}",
            blockers.join(" and ")
        ),
    )
}

pub(super) async fn request_delete(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    let blockers = deletion_blockers(&s, id).await?;
    if !blockers.is_empty() {
        return Err(blocked(&blockers));
    }
    let token = new_webhook_secret().replacen("whsec_", "del_", 1);
    let now = Utc::now();
    db::store_delete_request(s.pool(), id, &hash_api_key(&token), now).await?;
    sess.event(Action::ProjectDeleteRequested)
        .tenant(id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({
        "token": token,
        "confirm_after": now + Duration::hours(COOLING_OFF_HOURS),
        "cooling_off_hours": COOLING_OFF_HOURS,
    })))
}

pub(super) async fn cancel_delete(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    db::clear_delete_request(s.pool(), id).await?;
    Ok(Json(json!({"cancelled": true})))
}

#[derive(Deserialize)]
pub(super) struct ConfirmDelete {
    token: String,
}

pub(super) async fn confirm_delete(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<ConfirmDelete>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    let Some((hash, requested)) = db::delete_request(s.pool(), id).await? else {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "no deletion has been requested".into(),
        ));
    };
    let given = hash_api_key(input.token.trim());
    if !bool::from(given.as_bytes().ct_eq(hash.as_bytes())) {
        return Err(ApiError::bad_request("token does not match").with_field("token"));
    }
    let ready = requested + Duration::hours(COOLING_OFF_HOURS);
    if Utc::now() < ready {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("cooling-off period ends at {}", ready.to_rfc3339()),
        ));
    }
    let blockers = deletion_blockers(&s, id).await?;
    if !blockers.is_empty() {
        return Err(blocked(&blockers));
    }
    db::finish_project_delete(s.pool(), id, Utc::now()).await?;
    sess.event(Action::ProjectDeleted)
        .tenant(id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"deleted": true})))
}
