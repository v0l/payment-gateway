use super::*;

#[derive(Deserialize)]
pub(super) struct NoteInput {
    body: String,
}

pub(super) async fn list_notes(
    sess: Session,
    State(s): State<AppState>,
    Path((id, payment)): Path<(u64, String)>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Viewer).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    s.service.payment_for_tenant(&t, &pid).await?;
    Ok(Json(json!({"notes": db::payment_notes(s.pool(), id, &pid).await?})))
}

pub(super) async fn add_note(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, payment)): Path<(u64, String)>,
    Json(input): Json<NoteInput>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    s.service.payment_for_tenant(&t, &pid).await?;
    let body = input.body.trim();
    if body.is_empty() || body.chars().count() > 2000 {
        return Err(ApiError::bad_request("note must be 1 to 2000 characters").with_field("body"));
    }
    let note_id = db::add_payment_note(s.pool(), id, &pid, sess.account.id, body).await?;
    sess.event(Action::PaymentNoteAdded)
        .tenant(id)
        .target("payment", &payment)
        .details(json!({"note_id": note_id, "length": body.chars().count()}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"id": note_id})))
}

#[derive(Deserialize)]
pub(super) struct ManualPaidInput {
    reason: String,
}

pub(super) async fn mark_paid(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, payment)): Path<(u64, String)>,
    Json(input): Json<ManualPaidInput>,
) -> ApiResult<PaymentView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    if !db::manual_paid_allowed(s.pool(), id).await? {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "manual mark as paid is not enabled for this project".into(),
        ));
    }
    let reason = input.reason.trim();
    if reason.chars().count() < 3 || reason.chars().count() > 500 {
        return Err(ApiError::bad_request("reason must be 3 to 500 characters").with_field("reason"));
    }
    let row = s.service.mark_paid_manually(&t, &pid, reason).await?;
    sess.event(Action::PaymentMarkedPaid)
        .tenant(id)
        .target("payment", &payment)
        .details(json!({"reason": reason}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(s.service.view(&row).await?))
}

#[derive(Deserialize)]
pub(super) struct ManualPaidSetting {
    enabled: bool,
}

pub(super) async fn set_manual_paid(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<ManualPaidSetting>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    db::set_manual_paid_allowed(s.pool(), id, input.enabled).await?;
    sess.event(Action::ManualPaidToggled)
        .tenant(id)
        .details(json!({"enabled": input.enabled}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"allow_manual_paid": input.enabled})))
}

#[derive(Deserialize)]
pub(super) struct BulkCancel {
    older_than_hours: i64,
}

pub(super) async fn bulk_cancel(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<BulkCancel>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    if !(1..=24 * 365).contains(&input.older_than_hours) {
        return Err(ApiError::bad_request("older_than_hours must be between 1 and 8760")
            .with_field("older_than_hours"));
    }
    let cutoff = Utc::now() - Duration::hours(input.older_than_hours);
    let stale = db::stale_pending_payments(s.pool(), id, cutoff, 200).await?;
    let mut cancelled = 0;
    let mut skipped = 0;
    for p in &stale {
        match s.service.cancel_payment(&t, &p.pid()).await {
            Ok(_) => cancelled += 1,
            Err(_) => skipped += 1,
        }
    }
    sess.event(Action::PaymentsBulkCancelled)
        .tenant(id)
        .details(json!({"older_than_hours": input.older_than_hours, "cancelled": cancelled, "skipped": skipped}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"cancelled": cancelled, "skipped": skipped, "more": stale.len() == 200})))
}

#[derive(Deserialize)]
pub(super) struct ViewInput {
    name: String,
    filter: Value,
}

pub(super) async fn list_views(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let views: Vec<Value> = db::list_saved_views(s.pool(), id, sess.account.id)
        .await?
        .into_iter()
        .map(|v| json!({"id": v.id, "name": v.name, "filter": serde_json::from_str::<Value>(&v.filter).unwrap_or(Value::Null)}))
        .collect();
    Ok(Json(json!({"views": views})))
}

pub(super) async fn save_view(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<ViewInput>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let name = input.name.trim();
    if name.is_empty() || name.chars().count() > 64 {
        return Err(ApiError::bad_request("name must be 1 to 64 characters").with_field("name"));
    }
    let Some(filter) = input.filter.as_object() else {
        return Err(ApiError::bad_request("filter must be an object").with_field("filter"));
    };
    let mut clean = HashMap::new();
    for (k, v) in filter {
        if let Some(text) = v.as_str() {
            clean.insert(k.clone(), text.to_string());
        }
    }
    parse_filter(&clean)?;
    let existing = db::list_saved_views(s.pool(), id, sess.account.id).await?;
    if existing.len() >= 20 && !existing.iter().any(|v| v.name == name) {
        return Err(ApiError::bad_request("at most 20 saved views"));
    }
    let encoded = serde_json::to_string(&clean).map_err(anyhow::Error::from)?;
    db::upsert_saved_view(s.pool(), id, sess.account.id, name, &encoded).await?;
    sess.event(Action::SavedViewChanged)
        .tenant(id)
        .details(json!({"name": name, "removed": false}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"name": name, "filter": clean})))
}

pub(super) async fn delete_view(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, view)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    if !db::delete_saved_view(s.pool(), id, sess.account.id, view).await? {
        return Err(ApiError::not_found());
    }
    sess.event(Action::SavedViewChanged)
        .tenant(id)
        .details(json!({"view_id": view, "removed": true}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"deleted": true})))
}
