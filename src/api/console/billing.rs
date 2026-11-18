use super::*;
use crate::billing_views::*;

pub(super) async fn list_plans(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<PlanView>> {
    sess.project(&s, id, Role::Viewer).await?;
    let rows = crate::db::list_plans(s.pool(), id, None, 99).await?;
    Ok(Json(rows.iter().take(100).map(PlanView::from).collect()))
}

pub(super) async fn create_plan(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<CreatePlan>,
) -> ApiResult<PlanView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let plan = s.service.create_plan(&t, input).await?;
    sess.event(Action::PlanCreated)
        .tenant(id)
        .target("plan", plan.id.to_string())
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(PlanView::from(&plan)))
}

pub(super) async fn archive_plan(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, plan)): Path<(u64, u64)>,
) -> ApiResult<PlanView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let row = s.service.archive_plan(&t, plan).await?;
    sess.event(Action::PlanArchived)
        .tenant(id)
        .target("plan", plan.to_string())
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(PlanView::from(&row)))
}

pub(super) async fn list_subscriptions(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<SubscriptionView>> {
    sess.project(&s, id, Role::Viewer).await?;
    let rows = crate::db::list_subscriptions(
        s.pool(),
        id,
        None,
        q.get("customer_ref").map(String::as_str),
        q.get("status").map(String::as_str),
        99,
    )
    .await?;
    Ok(Json(
        rows.iter()
            .take(100)
            .map(|r| SubscriptionView::build(r, &[], &s.service.public_url))
            .collect(),
    ))
}

pub(super) async fn get_subscription(
    sess: Session,
    State(s): State<AppState>,
    Path((id, sub)): Path<(u64, u64)>,
) -> ApiResult<SubscriptionView> {
    let (t, _) = sess.project(&s, id, Role::Viewer).await?;
    let row = s.service.subscription_row(&t, sub).await?;
    Ok(Json(s.service.subscription_view(&t, &row).await?))
}

pub(super) async fn create_subscription(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(input): Json<CreateSubscription>,
) -> ApiResult<SubscriptionView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let row = s.service.create_subscription(&t, input).await?;
    sess.event(Action::SubscriptionCreated)
        .tenant(id)
        .target("subscription", row.id.to_string())
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(s.service.subscription_view(&t, &row).await?))
}

pub(super) async fn subscription_action(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, sub, action)): Path<(u64, u64, String)>,
    body: Option<Json<Value>>,
) -> ApiResult<SubscriptionView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let body = body.map(|b| b.0).unwrap_or(Value::Null);
    let row = match action.as_str() {
        "pause" => s.service.pause_subscription(&t, sub).await?,
        "resume" => s.service.resume_subscription(&t, sub).await?,
        "cancel" => {
            let input = CancelSubscription {
                at_period_end: body["at_period_end"].as_bool().unwrap_or(false),
            };
            s.service.cancel_subscription(&t, sub, input).await?
        }
        "change-plan" => {
            let input = ChangePlan {
                plan_id: body["plan_id"]
                    .as_u64()
                    .ok_or_else(|| ApiError::bad_request("plan_id is required"))?,
                proration: body["proration"].as_str().map(str::to_string),
            };
            s.service.change_plan(&t, sub, input).await?
        }
        _ => return Err(ApiError::not_found()),
    };
    sess.event(Action::SubscriptionChanged)
        .tenant(id)
        .target("subscription", sub.to_string())
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(s.service.subscription_view(&t, &row).await?))
}
