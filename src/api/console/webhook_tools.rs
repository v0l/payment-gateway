use super::*;
use crate::service::webhook_signature;
use subtle::ConstantTimeEq;

pub(super) async fn delivery_detail(
    sess: Session,
    State(s): State<AppState>,
    Path((id, delivery)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let d = db::get_tenant_delivery(s.pool(), id, delivery)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let url = match d.endpoint_id {
        Some(e) => db::get_endpoint(s.pool(), id, e).await?.map(|e| e.url),
        None => None,
    };
    let time_to_deliver_ms = d
        .delivered_at
        .map(|at| (at - d.created).num_milliseconds().max(0));
    Ok(Json(json!({
        "id": d.id,
        "event_id": d.event_id,
        "event_type": d.event_type,
        "attempts": d.attempts,
        "created": d.created,
        "delivered_at": d.delivered_at,
        "abandoned_at": d.abandoned_at,
        "next_attempt": d.next_attempt,
        "last_error": d.last_error,
        "request": {
            "method": "POST",
            "url": url,
            "headers": {
                "content-type": "application/json",
                "x-gateway-event-id": d.event_id,
                "x-gateway-signature": "t=<timestamp>,v1=<hmac sha256 of timestamp.body>",
            },
            "body": d.body,
        },
        "response": {
            "status": d.last_status,
            "body_snippet": d.last_response,
            "duration_ms": d.last_duration_ms,
            "at": d.last_attempt_at,
        },
        "time_to_deliver_ms": time_to_deliver_ms,
    })))
}

pub(super) async fn resend_failed(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    let queued = db::replay_abandoned_deliveries(s.pool(), id, Utc::now()).await?;
    sess.event(Action::DeliveriesResent)
        .tenant(id)
        .details(json!({"queued": queued}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"queued": queued})))
}

pub(super) async fn webhook_health(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let since = Utc::now() - Duration::days(7);
    let health = db::endpoint_health(s.pool(), id, since).await?;
    let endpoints = db::list_endpoints(s.pool(), id).await?;
    let rows: Vec<Value> = endpoints
        .iter()
        .map(|e| {
            let h = health.iter().find(|h| h.endpoint_id == Some(e.id));
            let total = h.map_or(0, |h| h.recent_total);
            let failed = h.map_or(0, |h| h.recent_failed);
            let state = if !e.enabled {
                "disabled"
            } else if total == 0 {
                "idle"
            } else if failed * 2 > total {
                "failing"
            } else if failed > 0 {
                "degraded"
            } else {
                "healthy"
            };
            json!({
                "endpoint_id": e.id,
                "url": e.url,
                "state": state,
                "deliveries_7d": total,
                "failed_7d": failed,
                "last_success": h.and_then(|h| h.last_success),
                "last_failure": h.and_then(|h| h.last_failure),
                "avg_duration_ms": h.and_then(|h| h.avg_duration_ms),
            })
        })
        .collect();
    Ok(Json(json!({"endpoints": rows})))
}

#[derive(Deserialize)]
pub(super) struct VerifyInput {
    body: String,
    secret: String,
    signature: String,
}

pub(super) async fn verify_signature(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(input): Json<VerifyInput>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let mut timestamp = None;
    let mut provided = vec![];
    for part in input.signature.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) => timestamp = v.parse::<i64>().ok(),
            Some(("v1", v)) => provided.push(v.to_string()),
            _ => {}
        }
    }
    let ts = timestamp.ok_or_else(|| {
        ApiError::bad_request("signature must look like t=<timestamp>,v1=<hex>")
            .with_field("signature")
    })?;
    let expected = webhook_signature(input.secret.trim(), ts, &input.body);
    let valid = provided
        .iter()
        .any(|p| bool::from(p.as_bytes().ct_eq(expected.as_bytes())));
    Ok(Json(json!({
        "valid": valid,
        "timestamp": ts,
        "age_seconds": Utc::now().timestamp() - ts,
    })))
}

pub(super) async fn resend_payment_webhook(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, payment)): Path<(u64, String)>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    s.service.payment_for_tenant(&t, &pid).await?;
    let queued = db::replay_latest_payment_event(s.pool(), id, &pid, Utc::now()).await?;
    if queued == 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "this payment has no webhook to resend".into(),
        ));
    }
    sess.event(Action::DeliveriesResent)
        .tenant(id)
        .target("payment", &payment)
        .details(json!({"queued": queued}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"queued": queued})))
}
