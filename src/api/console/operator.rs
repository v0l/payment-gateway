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
    sess.operator(&s)?;
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
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(e): Json<Enabled>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    if id == sess.account.id {
        return Err(ApiError::bad_request("you cannot disable yourself"));
    }
    if !accounts::set_account_disabled(s.pool(), id, !e.enabled).await? {
        return Err(ApiError::not_found());
    }
    let action = if e.enabled {
        Action::AccountEnabled
    } else {
        Action::AccountDisabled
    };
    sess.event(action)
        .target("account", id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
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
    headers: HeaderMap,
    Json(i): Json<OperatorInvite>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    let email = accounts::normalize_email(&i.email)
        .ok_or_else(|| ApiError::bad_request("invalid email"))?;
    if accounts::account_by_email(s.pool(), &email)
        .await?
        .is_some()
    {
        return Err(ApiError::new(
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
    let url = format!("{}/console/invite/{token}", s.service.public_url);
    s.service.send_mail(crate::mailer::templates::invite(
        &email,
        &url,
        None,
        accounts::INVITE_DAYS,
    ));
    sess.event(Action::InviteCreated)
        .target("invite", &email)
        .details(json!({"email": email, "operator": i.operator}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(
        json!({"invite_url": url, "email": email, "expires_in_days": accounts::INVITE_DAYS}),
    ))
}

#[derive(Deserialize)]
pub(super) struct Trusted {
    trusted: bool,
}

pub(super) async fn admin_trusted(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(t): Json<Trusted>,
) -> ApiResult<Value> {
    sess.operator(&s)?;
    if !db::set_tenant_trusted(s.pool(), id, t.trusted).await? {
        return Err(ApiError::not_found());
    }
    sess.event(Action::ProjectTrustChanged)
        .tenant(id)
        .target("project", id)
        .details(json!({"trusted": t.trusted}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"trusted": t.trusted})))
}

pub(super) async fn admin_providers(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<Vec<db::ProviderOverviewRow>> {
    sess.operator(&s)?;
    Ok(Json(db::provider_overview(s.pool()).await?))
}
