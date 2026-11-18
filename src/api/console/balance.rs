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
