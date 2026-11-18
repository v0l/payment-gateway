use super::*;

pub(super) async fn list_payments(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let status = q.get("status").filter(|v| !v.is_empty());
    if status.is_some_and(|v| {
        ![
            "pending",
            "paid",
            "expired",
            "cancelled",
            "refunded",
            "partially_refunded",
        ]
        .contains(&v.as_str())
    }) {
        return Err(ApiError::bad_request("unknown status"));
    }
    let before = q
        .get("before")
        .filter(|v| !v.is_empty())
        .map(|v| parse_cursor(v))
        .transpose()
        .map_err(|_| ApiError::bad_request("before must be a cursor from a previous page"))?;
    let search = q.get("q").map(|v| v.trim()).filter(|v| !v.is_empty());
    let rows =
        db::list_payments(s.pool(), id, status.map(String::as_str), search, before, 51).await?;
    let more = rows.len() > 50;
    let mut out = vec![];
    for row in rows.iter().take(50) {
        out.push(s.service.view(row).await?);
    }
    let next = if more {
        out.last()
            .map(|p: &PaymentView| format!("{}_{}", p.created.timestamp(), p.id))
    } else {
        None
    };
    Ok(Json(json!({"payments": out, "next": next})))
}

pub(super) fn parse_cursor(v: &str) -> Result<(DateTime<Utc>, Vec<u8>), ()> {
    let (secs, id) = v.split_once('_').ok_or(())?;
    let at = DateTime::from_timestamp(secs.parse().map_err(|_| ())?, 0).ok_or(())?;
    Ok((at, hex::decode(id).map_err(|_| ())?))
}

pub(super) async fn get_payment(
    sess: Session,
    State(s): State<AppState>,
    Path((id, payment)): Path<(u64, String)>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Viewer).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    let row = s.service.payment_for_tenant(&t, &pid).await?;
    let attempts = db::attempts_for_payment(s.pool(), &pid).await?;
    let mut receipts = vec![];
    for a in &attempts {
        for r in db::receipts_for_attempt(s.pool(), a.id).await? {
            receipts.push(json!({"attempt_id": a.id, "ref": r.r#ref, "amount": r.amount, "confirmed": r.confirmed}));
        }
    }
    let deliveries: Vec<Value> = db::deliveries_for_payment(s.pool(), &pid)
        .await?
        .into_iter()
        .map(|d| json!({"id": d.id, "event_type": d.event_type, "attempts": d.attempts, "delivered_at": d.delivered_at, "abandoned_at": d.abandoned_at, "last_error": d.last_error}))
        .collect();
    Ok(Json(json!({
        "payment": s.service.view(&row).await?,
        "receipts": receipts,
        "deliveries": deliveries,
    })))
}

pub(super) async fn create_payment(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(input): Json<crate::model::CreatePayment>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let (row, _) = s.service.create_payment(&t, input, None).await?;
    Ok(Json(
        serde_json::to_value(s.service.view(&row).await?).map_err(anyhow::Error::from)?,
    ))
}

#[derive(Deserialize)]
pub(super) struct AttemptInput {
    method: String,
    provider_id: Option<u64>,
    option: Option<String>,
}

pub(super) async fn create_attempt(
    sess: Session,
    State(s): State<AppState>,
    Path((id, payment)): Path<(u64, String)>,
    Json(a): Json<AttemptInput>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    s.service
        .create_attempt(
            &t,
            &pid,
            CreateAttempt {
                method: a.method,
                provider_id: a.provider_id,
                option: a.option,
                ..Default::default()
            },
        )
        .await?;
    let row = s.service.payment_for_tenant(&t, &pid).await?;
    Ok(Json(
        serde_json::to_value(s.service.view(&row).await?).map_err(anyhow::Error::from)?,
    ))
}

pub(super) async fn cancel_payment(
    sess: Session,
    State(s): State<AppState>,
    Path((id, payment)): Path<(u64, String)>,
) -> ApiResult<PaymentView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    let row = s.service.cancel_payment(&t, &pid).await?;
    Ok(Json(s.service.view(&row).await?))
}

pub(super) async fn refund_payment(
    sess: Session,
    State(s): State<AppState>,
    Path((id, payment)): Path<(u64, String)>,
    Json(input): Json<RefundInput>,
) -> ApiResult<RefundView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    Ok(Json(s.service.refund(&t, &pid, input).await?))
}

pub(super) async fn stats(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let days = q
        .get("days")
        .and_then(|d| d.parse::<i64>().ok())
        .unwrap_or(30)
        .clamp(1, 365);
    let since = Utc::now() - Duration::days(days);
    Ok(Json(json!({
        "days": days,
        "by_status": db::payment_stats(s.pool(), id, since).await?,
        "paid_by_day": db::paid_by_day(s.pool(), id, since).await?,
        "by_method": db::method_counts(s.pool(), id, since).await?.into_iter().map(|(m, c)| json!({"method": m, "count": c})).collect::<Vec<_>>(),
        "pending_deliveries": db::pending_delivery_count(s.pool(), id).await?,
    })))
}

pub(super) async fn deliveries(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<db::DeliveryDetail>> {
    sess.project(&s, id, Role::Viewer).await?;
    let only = q
        .get("undelivered")
        .is_some_and(|v| v == "1" || v == "true");
    Ok(Json(db::deliveries_for_tenant(s.pool(), id, only).await?))
}

pub(super) async fn replay(
    sess: Session,
    State(s): State<AppState>,
    Path((id, delivery)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    if !db::replay_tenant_delivery(s.pool(), id, delivery, Utc::now()).await? {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({"queued": true})))
}
