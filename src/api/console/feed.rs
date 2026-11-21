use super::*;

#[derive(Serialize)]
struct FeedItem {
    at: DateTime<Utc>,
    kind: &'static str,
    tone: &'static str,
    title: String,
    amount: Option<u64>,
    currency: Option<String>,
    payment_id: Option<String>,
    reference: Option<String>,
    customer: Option<String>,
    method: Option<String>,
    detail: Option<String>,
}

fn event_item(e: &db::EventRow) -> Option<FeedItem> {
    let body: Value = serde_json::from_str(&e.body).ok()?;
    let data = &body["data"];
    let detail = &data["detail"];
    if let Some(p) = data.get("payment").filter(|p| p.is_object()) {
        let method = p["attempts"]
            .as_array()
            .and_then(|a| {
                a.iter()
                    .find(|x| x["status"] == "paid")
                    .or_else(|| a.last())
            })
            .and_then(|a| a["method"].as_str())
            .map(str::to_string);
        let (kind, tone, title, extra): (&str, &str, &str, Option<String>) =
            match e.event_type.as_str() {
                "payment.paid" if detail["manual"] == true => (
                    "payment",
                    "ok",
                    "Marked paid",
                    detail["reason"].as_str().map(str::to_string),
                ),
                "payment.paid" => ("payment", "ok", "Paid", None),
                "payment.late_payment" => ("payment", "warn", "Paid after it closed", None),
                "payment.expired" => ("payment", "muted", "Expired unpaid", None),
                "payment.cancelled" => ("payment", "muted", "Cancelled", None),
                "payment.disputed" => (
                    "dispute",
                    "bad",
                    "Dispute opened",
                    detail["reason"].as_str().map(str::to_string),
                ),
                "payment.dispute_closed" => (
                    "dispute",
                    if detail["dispute"] == "lost" {
                        "bad"
                    } else {
                        "ok"
                    },
                    if detail["dispute"] == "lost" {
                        "Dispute lost"
                    } else {
                        "Dispute won"
                    },
                    detail["reason"].as_str().map(str::to_string),
                ),
                "attempt.failed" => (
                    "payment",
                    "bad",
                    "Declined",
                    detail["reason"].as_str().map(str::to_string),
                ),
                "refund.completed" => ("refund", "warn", "Refunded", None),
                "refund.failed" => (
                    "refund",
                    "bad",
                    "Refund failed",
                    detail["error"].as_str().map(str::to_string),
                ),
                "refund.requested" => ("refund", "muted", "Refund requested", None),
                _ => return None,
            };
        let amount = if kind == "refund" {
            detail["amount"].as_u64().or_else(|| p["amount"].as_u64())
        } else {
            p["amount"].as_u64()
        };
        return Some(FeedItem {
            at: e.created,
            kind,
            tone,
            title: title.into(),
            amount,
            currency: p["currency"].as_str().map(str::to_string),
            payment_id: p["id"].as_str().map(str::to_string),
            reference: p["description"]
                .as_str()
                .or(p["reference"].as_str())
                .map(str::to_string),
            customer: p["customer_ref"].as_str().map(str::to_string),
            method,
            detail: extra.map(|x| crate::logsafe::clean(&x)),
        });
    }
    if let Some(w) = data.get("withdrawal").filter(|w| w.is_object()) {
        let (tone, title) = match e.event_type.as_str() {
            "withdrawal.paid" => return None,
            "withdrawal.completed" => ("ok", "Payout sent"),
            "withdrawal.failed" => ("bad", "Payout failed"),
            "withdrawal.rejected" => ("bad", "Payout rejected"),
            "withdrawal.requested" => ("muted", "Payout requested"),
            "withdrawal.approved" => ("muted", "Payout approved"),
            "withdrawal.cancelled" => ("muted", "Payout cancelled"),
            _ => return None,
        };
        return Some(FeedItem {
            at: e.created,
            kind: "payout",
            tone,
            title: title.into(),
            amount: w["amount"].as_u64(),
            currency: w["currency"].as_str().map(str::to_string),
            payment_id: None,
            reference: w["destination_label"].as_str().map(str::to_string),
            customer: None,
            method: None,
            detail: w["error"].as_str().map(crate::logsafe::clean),
        });
    }
    None
}

pub(super) async fn feed(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let kind = q.get("kind").map(String::as_str).unwrap_or("all");
    let before = match q.get("before").filter(|v| !v.is_empty()) {
        None => s.service.clock.now(),
        Some(v) => DateTime::parse_from_rfc3339(v)
            .map(|t| t.with_timezone(&Utc))
            .map_err(|_| {
                ApiError::bad_request("before must be a timestamp").with_field("before")
            })?,
    };
    let limit = 30u32;
    let mut items: Vec<FeedItem> = vec![];
    let prefixes: &[&str] = match kind {
        "all" => &["payment.", "attempt.", "refund.", "withdrawal."],
        "payments" => &[
            "payment.paid",
            "payment.late",
            "payment.expired",
            "payment.cancelled",
            "attempt.",
        ],
        "refunds" => &["refund.", "payment.dispute"],
        "payouts" => &["withdrawal."],
        "webhooks" => &[],
        _ => {
            return Err(ApiError::bad_request(
                "kind must be all, payments, refunds, payouts or webhooks",
            )
            .with_field("kind"));
        }
    };
    if !prefixes.is_empty() {
        for e in db::feed_events(s.pool(), id, before, prefixes, limit * 2).await? {
            if let Some(item) = event_item(&e) {
                items.push(item);
            }
        }
    }
    if kind == "all" || kind == "payments" {
        for p in db::feed_created(s.pool(), id, before, limit).await? {
            items.push(FeedItem {
                at: p.created,
                kind: "payment",
                tone: "muted",
                title: "Created".into(),
                amount: Some(p.amount),
                currency: Some(p.currency.clone()),
                payment_id: Some(hex::encode(&p.id)),
                reference: p.description.clone().or(Some(p.reference.clone())),
                customer: p.customer_ref.clone(),
                method: None,
                detail: None,
            });
        }
    }
    if kind == "all" || kind == "webhooks" {
        let failed = db::feed_failed_deliveries(s.pool(), id, before, limit * 4).await?;
        let mut groups: Vec<(Option<String>, Vec<&db::FailedDelivery>)> = vec![];
        for d in &failed {
            match groups.iter_mut().find(|(url, _)| *url == d.url) {
                Some((_, list)) if kind == "all" => list.push(d),
                _ => groups.push((d.url.clone(), vec![d])),
            }
        }
        for (url, list) in groups {
            let d = list[0];
            let pending = list.iter().filter(|x| !x.delivered && !x.abandoned).count();
            let state = if list.len() > 1 {
                format!("{} events failed, {pending} still retrying", list.len())
            } else if d.delivered {
                "delivered after retrying".into()
            } else if d.abandoned {
                "gave up after retrying".into()
            } else {
                "retrying".into()
            };
            items.push(FeedItem {
                at: d.at,
                kind: "webhook",
                tone: if list.iter().all(|x| x.delivered) {
                    "warn"
                } else {
                    "bad"
                },
                title: if list.len() > 1 {
                    "Webhooks failing".into()
                } else {
                    format!("Webhook {} failed", d.event_type)
                },
                amount: None,
                currency: None,
                payment_id: if list.len() > 1 {
                    None
                } else {
                    d.payment_id.clone()
                },
                reference: url,
                customer: None,
                method: None,
                detail: Some(format!(
                    "{}{}, {state}",
                    d.last_status
                        .map(|s| format!("answered {s}"))
                        .unwrap_or_else(|| "no answer".into()),
                    d.last_error
                        .as_deref()
                        .filter(|_| d.last_status.is_none())
                        .map(|e| format!(": {}", crate::logsafe::clean(e)))
                        .unwrap_or_default(),
                )),
            });
        }
    }
    items.sort_by_key(|i| std::cmp::Reverse(i.at));
    items.truncate(limit as usize);
    let next = (items.len() == limit as usize)
        .then(|| {
            items
                .last()
                .map(|i| (i.at - Duration::milliseconds(1)).to_rfc3339())
        })
        .flatten();
    Ok(Json(json!({"items": items, "next": next})))
}
