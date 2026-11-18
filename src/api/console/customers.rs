use super::*;
use crate::db::SearchFilter;

pub(super) async fn list_customers(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let search = q.get("q").map(|v| v.trim()).filter(|v| !v.is_empty());
    Ok(Json(
        json!({"customers": db::list_customers(s.pool(), id, search, 100).await?}),
    ))
}

pub(super) async fn get_customer(
    sess: Session,
    State(s): State<AppState>,
    Path((id, customer)): Path<(u64, String)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let filter = SearchFilter {
        customer: Some(customer.clone()),
        ..Default::default()
    };
    let payments = db::search_payments(s.pool(), id, &filter, 100).await?;
    let methods = db::customer_saved_methods(s.pool(), id, &customer).await?;
    let plans: std::collections::HashMap<u64, db::PlanRow> =
        db::list_plans(s.pool(), id, None, 100)
            .await?
            .into_iter()
            .map(|p| (p.id, p))
            .collect();
    let subscriptions: Vec<Value> =
        db::list_subscriptions(s.pool(), id, None, Some(&customer), None, 50)
            .await?
            .iter()
            .take(50)
            .map(|sub| {
                let plan = plans.get(&sub.plan_id);
                json!({
                    "id": sub.id,
                    "status": sub.status,
                    "plan_id": sub.plan_id,
                    "plan_name": plan.map(|p| p.name.clone()),
                    "amount": plan.map(|p| p.amount),
                    "currency": plan.map(|p| p.currency.clone()),
                    "next_charge": sub.next_charge,
                    "current_period_end": sub.current_period_end,
                    "cancel_at_period_end": sub.cancel_at_period_end,
                })
            })
            .collect();
    if payments.is_empty() && methods.is_empty() && subscriptions.is_empty() {
        return Err(ApiError::not_found());
    }
    let email = payments.iter().find_map(|p| p.customer_email.clone());
    let mut totals: std::collections::BTreeMap<String, u64> = Default::default();
    for p in payments
        .iter()
        .filter(|p| matches!(p.status.as_str(), "paid" | "partially_refunded"))
    {
        *totals.entry(p.currency.clone()).or_default() += p.amount;
    }
    Ok(Json(json!({
        "customer_ref": customer,
        "email": email,
        "paid_totals": totals,
        "payments": payments.iter().map(|p| json!({
            "id": hex::encode(&p.id), "reference": p.reference, "status": p.status,
            "currency": p.currency, "amount": p.amount, "created": p.created, "paid_at": p.paid_at,
        })).collect::<Vec<_>>(),
        "saved_methods": methods.iter().map(|m| json!({
            "id": m.id, "brand": m.brand, "last4": m.last4,
            "exp_month": m.exp_month, "exp_year": m.exp_year, "created": m.created,
        })).collect::<Vec<_>>(),
        "subscriptions": subscriptions,
    })))
}

pub(super) async fn delete_customer_method(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, customer, method)): Path<(u64, String, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    let owned = db::get_saved_method(s.pool(), id, method)
        .await?
        .filter(|m| m.customer_ref == customer)
        .ok_or_else(ApiError::not_found)?;
    db::disable_saved_method(s.pool(), id, owned.id).await?;
    sess.event(Action::SavedMethodDeleted)
        .tenant(id)
        .target("saved_method", owned.id)
        .details(json!({"customer_ref": customer, "last4": owned.last4}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"deleted": true})))
}
