use super::*;

#[derive(Serialize)]
pub(super) struct AccountView {
    id: u64,
    email: String,
    is_operator: bool,
    disabled: bool,
    created: DateTime<Utc>,
    last_login: Option<DateTime<Utc>>,
}

pub(super) async fn admin_accounts(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<Vec<AccountView>> {
    sess.operator()?;
    Ok(Json(
        accounts::list_accounts(s.pool())
            .await?
            .into_iter()
            .map(|a| AccountView {
                id: a.id,
                email: a.email,
                is_operator: a.is_operator,
                disabled: a.disabled,
                created: a.created,
                last_login: a.last_login,
            })
            .collect(),
    ))
}

pub(super) async fn admin_disable(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(e): Json<Enabled>,
) -> ApiResult<Value> {
    sess.operator()?;
    if id == sess.account.id {
        return Err(ApiError::bad_request("you cannot disable yourself"));
    }
    if !accounts::set_account_disabled(s.pool(), id, !e.enabled).await? {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({"enabled": e.enabled})))
}

#[derive(Deserialize)]
pub(super) struct OperatorInvite {
    email: String,
    #[serde(default)]
    operator: bool,
}

pub(super) async fn admin_invite(
    sess: Session,
    State(s): State<AppState>,
    Json(i): Json<OperatorInvite>,
) -> ApiResult<Value> {
    sess.operator()?;
    let email = accounts::normalize_email(&i.email)
        .ok_or_else(|| ApiError::bad_request("invalid email"))?;
    if accounts::account_by_email(s.pool(), &email)
        .await?
        .is_some()
    {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "an account with this email already exists".into(),
        ));
    }
    let token = accounts::insert_invite(
        s.pool(),
        &accounts::NewInvite {
            email: &email,
            tenant_id: None,
            role: None,
            operator: i.operator,
            created_by: Some(sess.account.id),
        },
    )
    .await?;
    Ok(Json(
        json!({"invite_url": format!("{}/console/invite/{token}", s.service.public_url), "email": email, "expires_in_days": accounts::INVITE_DAYS}),
    ))
}

#[derive(Deserialize)]
pub(super) struct Trusted {
    trusted: bool,
}

pub(super) async fn admin_trusted(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(t): Json<Trusted>,
) -> ApiResult<Value> {
    sess.operator()?;
    if !db::set_tenant_trusted(s.pool(), id, t.trusted).await? {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({"trusted": t.trusted})))
}
