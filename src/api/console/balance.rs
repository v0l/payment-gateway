use super::*;
use crate::api::ledger as view;

pub(super) async fn project_balance(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<view::BalanceView> {
    sess.project(&s, id, Role::Viewer).await?;
    view::balance_view(&s, id).await
}

pub(super) async fn project_ledger(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<crate::ledger::EntryView>> {
    sess.project(&s, id, Role::Viewer).await?;
    view::entries_view(&s, id, &q).await
}

pub(super) async fn project_statement(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<crate::ledger::StatementLine>> {
    sess.project(&s, id, Role::Viewer).await?;
    let before = match q.get("before").filter(|v| !v.is_empty()) {
        None => None,
        Some(v) => Some(v.parse::<u64>().map_err(|_| {
            ApiError::bad_request("before must be an entry id").with_field("before")
        })?),
    };
    let limit = match q.get("limit").filter(|v| !v.is_empty()) {
        None => 50,
        Some(v) => v
            .parse::<u32>()
            .ok()
            .filter(|n| (1..=200).contains(n))
            .ok_or_else(|| ApiError::bad_request("limit must be 1 to 200").with_field("limit"))?,
    };
    Ok(Json(
        crate::ledger::statement(s.pool(), id, "tenant_balance", "BTC", before, limit).await?,
    ))
}
