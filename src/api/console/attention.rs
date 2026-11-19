use super::*;

const LOOKBACK_DAYS: i64 = 14;

#[derive(Serialize)]
struct Row {
    key: String,
    payment_id: Option<String>,
    title: String,
    who: Option<String>,
    method: Option<String>,
    why: String,
    due: Option<String>,
    due_soon: bool,
    at: Option<DateTime<Utc>>,
    amount: Option<u64>,
    currency: Option<String>,
    action: &'static str,
    action_label: &'static str,
    bulk: Option<&'static str>,
}

#[derive(Serialize)]
struct Group {
    kind: &'static str,
    title: &'static str,
    tone: &'static str,
    rows: Vec<Row>,
}

fn title_of(p: &db::AttentionPayment) -> String {
    p.description.clone().unwrap_or_else(|| p.reference.clone())
}

fn pay_row(
    p: &db::AttentionPayment,
    why: String,
    due: Option<String>,
    action: &'static str,
    label: &'static str,
    bulk: Option<&'static str>,
) -> Row {
    Row {
        key: format!("payment:{}", p.id),
        payment_id: Some(p.id.clone()),
        title: title_of(p),
        who: p.customer_ref.clone(),
        method: p.method.clone(),
        why,
        due,
        due_soon: false,
        at: p.at,
        amount: Some(p.amount),
        currency: Some(p.currency.clone()),
        action,
        action_label: label,
        bulk,
    }
}

fn btc(msat: i64) -> String {
    let sats = msat / 1000;
    let s = sats.abs().to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    format!("{}{out} sats", if sats < 0 { "-" } else { "" })
}

fn received_of(p: &db::AttentionPayment) -> (i64, i64) {
    (p.received.unwrap_or(0), p.required.unwrap_or(0))
}

pub(super) async fn attention(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let now = s.service.clock.now();
    let since = now - Duration::days(LOOKBACK_DAYS);
    let mut groups: Vec<Group> = vec![];

    let disputes = db::open_disputes(s.pool(), id).await?;
    groups.push(Group {
        kind: "disputes",
        title: "Disputes",
        tone: "bad",
        rows: disputes
            .iter()
            .map(|p| {
                pay_row(
                    p,
                    "The customer disputed this payment with their bank".into(),
                    p.at.map(|t| format!("Opened {}", t.format("%-d %b"))),
                    "open",
                    "Review",
                    None,
                )
            })
            .collect(),
    });

    let mut wrong = vec![];
    for p in db::paid_after_close(s.pool(), id, since).await? {
        wrong.push(pay_row(
            &p,
            format!("Paid in full after the payment was {}", p.status),
            p.at.map(|t| format!("Paid {}", t.format("%-d %b %H:%M"))),
            "mark_paid",
            "Accept",
            Some("mark_paid"),
        ));
    }
    for p in db::underpaid(s.pool(), id, since).await? {
        let (got, need) = received_of(&p);
        wrong.push(pay_row(
            &p,
            format!(
                "Paid {} of {}, short {}",
                btc(got),
                btc(need),
                btc(need - got)
            ),
            p.at.map(|t| format!("Last seen {}", t.format("%-d %b %H:%M"))),
            "open",
            "Decide",
            None,
        ));
    }
    groups.push(Group {
        kind: "wrong_amount",
        title: "Paid late or short",
        tone: "warn",
        rows: wrong,
    });

    groups.push(Group {
        kind: "confirming",
        title: "Waiting for confirmations",
        tone: "warn",
        rows: db::awaiting_confirmation(s.pool(), id)
            .await?
            .iter()
            .map(|p| {
                let (got, _) = received_of(p);
                pay_row(
                    p,
                    format!("{} seen on-chain, not confirmed yet", btc(got)),
                    p.at.map(|t| format!("Seen {}", t.format("%H:%M"))),
                    "open",
                    "Watch",
                    None,
                )
            })
            .collect(),
    });

    groups.push(Group {
        kind: "declined",
        title: "Declined",
        tone: "bad",
        rows: db::declined_unpaid(s.pool(), id, since)
            .await?
            .iter()
            .map(|p| {
                pay_row(
                    p,
                    "The last attempt was declined and the customer has not tried again".into(),
                    p.at.map(|t| format!("Since {}", t.format("%-d %b %H:%M"))),
                    "copy_link",
                    "Copy pay link",
                    Some("cancel"),
                )
            })
            .collect(),
    });

    groups.push(Group {
        kind: "refunds_failed",
        title: "Refunds that failed",
        tone: "bad",
        rows: db::failed_refunds(s.pool(), id, since)
            .await?
            .into_iter()
            .map(|r| Row {
                key: format!("refund:{}", r.refund_id),
                payment_id: Some(r.id.clone()),
                title: r.description.clone().unwrap_or(r.reference.clone()),
                who: r.customer_ref.clone(),
                method: Some(r.method.clone()),
                why: r
                    .error
                    .as_deref()
                    .map(crate::logsafe::clean)
                    .unwrap_or_else(|| "The provider did not accept the refund".into()),
                due: Some(format!("Failed {}", r.at.format("%-d %b %H:%M"))),
                due_soon: false,
                at: Some(r.at),
                amount: Some(r.amount),
                currency: Some(r.currency.clone()),
                action: "open",
                action_label: "Try again",
                bulk: None,
            })
            .collect(),
    });

    let endpoints = db::failing_endpoints(s.pool(), id, now - Duration::days(7)).await?;
    groups.push(Group {
        kind: "webhooks",
        title: "Webhook endpoint failing",
        tone: "bad",
        rows: endpoints
            .into_iter()
            .map(|e| {
                let answer = e
                    .last_status
                    .map(|c| format!("Answers {c}"))
                    .unwrap_or_else(|| "No answer".into());
                Row {
                    key: format!("endpoint:{}", e.endpoint_id.unwrap_or(0)),
                    payment_id: None,
                    title: e.url.clone().unwrap_or_else(|| "Default endpoint".into()),
                    who: Some(format!(
                        "{} event{}",
                        e.abandoned + e.retrying,
                        if e.abandoned + e.retrying == 1 {
                            ""
                        } else {
                            "s"
                        }
                    )),
                    method: None,
                    why: format!(
                        "{answer} since {}, {} given up, {} retrying",
                        e.since.format("%-d %b %H:%M"),
                        e.abandoned,
                        e.retrying
                    ),
                    due: None,
                    due_soon: false,
                    at: Some(e.since),
                    amount: None,
                    currency: None,
                    action: "resend_failed",
                    action_label: "Resend failed",
                    bulk: None,
                }
            })
            .collect(),
    });

    let providers = db::tenant_provider_status(s.pool(), id).await?;
    groups.push(Group {
        kind: "providers",
        title: "Payment method switched off",
        tone: "bad",
        rows: providers
            .into_iter()
            .filter(|p| p.auto_disabled_at.is_some())
            .map(|p| Row {
                key: format!("provider:{}", p.id),
                payment_id: None,
                title: p.name.clone(),
                who: None,
                method: Some(p.method.clone()),
                why: "Switched off after repeated errors. Customers cannot pay with it".into(),
                due: p
                    .auto_disabled_at
                    .map(|t| format!("Since {}", t.format("%-d %b %H:%M"))),
                due_soon: true,
                at: p.auto_disabled_at,
                amount: None,
                currency: None,
                action: "providers",
                action_label: "Check settings",
                bulk: None,
            })
            .collect(),
    });

    groups.push(Group {
        kind: "past_due",
        title: "Subscriptions past due",
        tone: "warn",
        rows: db::past_due_subscriptions(s.pool(), id)
            .await?
            .into_iter()
            .map(|x| Row {
                key: format!("subscription:{}", x.id),
                payment_id: None,
                title: x.plan_name.clone().unwrap_or_else(|| "Subscription".into()),
                who: Some(x.customer_ref.clone()),
                method: None,
                why: "The renewal was not paid. Retries continue for a few days".into(),
                due: x
                    .next_charge
                    .map(|t| format!("Next try {}", t.format("%-d %b"))),
                due_soon: false,
                at: x.next_charge,
                amount: x.amount,
                currency: x.currency.clone(),
                action: "subscription",
                action_label: "Open",
                bulk: None,
            })
            .collect(),
    });

    groups.push(Group {
        kind: "payouts",
        title: "Payouts that failed",
        tone: "bad",
        rows: db::failed_withdrawals(s.pool(), id, since)
            .await?
            .into_iter()
            .map(|w| Row {
                key: format!("withdrawal:{}", w.id),
                payment_id: None,
                title: format!("Payout to {}", w.destination_label),
                who: None,
                method: None,
                why: w
                    .error
                    .as_deref()
                    .map(crate::logsafe::clean)
                    .unwrap_or_else(|| format!("The payout was {}", w.status)),
                due: Some(format!("{}", w.at.format("%-d %b %H:%M"))),
                due_soon: false,
                at: Some(w.at),
                amount: Some(w.amount),
                currency: Some(w.currency.clone()),
                action: "payouts",
                action_label: "Open payouts",
                bulk: None,
            })
            .collect(),
    });

    groups.retain(|g| !g.rows.is_empty());
    let open: usize = groups.iter().map(|g| g.rows.len()).sum();
    let waiting = db::waiting_for_customer(s.pool(), id).await?;
    Ok(Json(json!({
        "open": open,
        "groups": groups,
        "waiting": waiting,
        "manual_paid_allowed": db::manual_paid_allowed(s.pool(), id).await?,
    })))
}

#[derive(Deserialize)]
pub(super) struct BulkInput {
    action: String,
    payments: Vec<String>,
    reason: Option<String>,
}

pub(super) async fn attention_bulk(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<BulkInput>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    if input.payments.is_empty() || input.payments.len() > 50 {
        return Err(
            ApiError::bad_request("choose between 1 and 50 payments").with_field("payments")
        );
    }
    let reason = input.reason.as_deref().map(str::trim).unwrap_or("");
    match input.action.as_str() {
        "cancel" => {}
        "mark_paid" => {
            if !db::manual_paid_allowed(s.pool(), id).await? {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "manual mark as paid is not enabled for this project".into(),
                ));
            }
            if reason.chars().count() < 3 || reason.chars().count() > 500 {
                return Err(ApiError::bad_request("reason must be 3 to 500 characters")
                    .with_field("reason"));
            }
        }
        _ => {
            return Err(
                ApiError::bad_request("action must be cancel or mark_paid").with_field("action")
            );
        }
    }
    let mut done = vec![];
    let mut failed = vec![];
    for raw in &input.payments {
        let Some(pid) = parse_payment_id(raw) else {
            failed.push(json!({"payment": raw, "error": "not found"}));
            continue;
        };
        let outcome = if input.action == "cancel" {
            s.service.cancel_payment(&t, &pid).await.map(|_| ())
        } else {
            s.service
                .mark_paid_manually(&t, &pid, reason)
                .await
                .map(|_| ())
        };
        match outcome {
            Ok(()) => done.push(raw.clone()),
            Err(e) => failed.push(json!({"payment": raw, "error": ApiError::from(e).message})),
        }
    }
    let action = if input.action == "cancel" {
        Action::PaymentsBulkCancelled
    } else {
        Action::PaymentMarkedPaid
    };
    sess.event(action)
        .tenant(id)
        .details(json!({"source": "attention", "done": done.len(), "failed": failed.len()}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"done": done, "failed": failed})))
}

pub(super) async fn payment_view_counts(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let now = s.service.clock.now();
    let all = db::SearchFilter::default();
    let status = |v: &str| db::SearchFilter {
        status: Some(v.into()),
        ..Default::default()
    };
    let week = db::SearchFilter {
        created_from: Some(now - Duration::days(7)),
        ..Default::default()
    };
    let disputed = db::SearchFilter {
        disputed: true,
        ..Default::default()
    };
    let mut counts = serde_json::Map::new();
    for (key, f) in [
        ("all", all),
        ("pending", status("pending")),
        ("paid", status("paid")),
        ("refunded", status("refunded")),
        ("partially_refunded", status("partially_refunded")),
        ("expired", status("expired")),
        ("cancelled", status("cancelled")),
        ("disputed", disputed),
        ("last_7_days", week),
    ] {
        counts.insert(
            key.into(),
            json!(db::count_payments(s.pool(), id, &f).await?),
        );
    }
    Ok(Json(Value::Object(counts)))
}
