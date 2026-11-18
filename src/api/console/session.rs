use super::*;

#[derive(Deserialize)]
pub(super) struct Credentials {
    email: String,
    password: String,
}

#[derive(Serialize)]
pub(super) struct Me {
    id: u64,
    email: String,
    is_operator: bool,
    csrf: String,
    projects: Vec<accounts::ProjectRef>,
}

pub(super) async fn me_for(s: &AppState, sess: &Session) -> Result<Me, ApiError> {
    Ok(Me {
        id: sess.account.id,
        email: sess.account.email.clone(),
        is_operator: sess.account.is_operator,
        csrf: sess.session.csrf.clone(),
        projects: accounts::projects_for_account(s.pool(), &sess.account).await?,
    })
}

pub(super) async fn login(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(c): Json<Credentials>,
) -> Result<Response, ApiError> {
    let email = accounts::normalize_email(&c.email).unwrap_or_default();
    let ip = format!("ip:{}", client_ip(&headers));
    let ek = format!("email:{email}");
    if s.login_limiter.blocked(&ek) || s.login_limiter.blocked(&ip) {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many attempts, try again later".into(),
        ));
    }
    let origin = Origin::from_headers(&headers);
    let account = accounts::account_by_email(s.pool(), &email).await?;
    let ok = match &account {
        Some(a) if !a.disabled => accounts::verify_password(&a.password_hash, &c.password),
        _ => {
            accounts::burn_password_check(&c.password);
            false
        }
    };
    if !ok {
        s.login_limiter.failure(&ek);
        s.login_limiter.failure(&ip);
        let actor = account
            .as_ref()
            .map(|a| Actor::Account(a.id))
            .unwrap_or(Actor::System);
        Event::new(Action::LoginFailed, actor)
            .target("email", &email)
            .record(s.pool(), &origin)
            .await;
        return Err(ApiError::unauthorized_with("wrong email or password"));
    }
    let account = account.unwrap();
    s.login_limiter.clear(&ek);
    accounts::touch_login(s.pool(), account.id).await?;
    let (token, csrf) = accounts::create_session(s.pool(), account.id).await?;
    Event::new(Action::LoginOk, Actor::Account(account.id))
        .target("account", account.id)
        .record(s.pool(), &origin)
        .await;
    let projects = accounts::projects_for_account(s.pool(), &account).await?;
    let body = Me {
        id: account.id,
        email: account.email,
        is_operator: account.is_operator,
        csrf,
        projects,
    };
    let mut r = Json(body).into_response();
    r.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&s, &token, accounts::SESSION_HOURS * 3600),
    );
    Ok(r)
}

pub(super) async fn logout(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    accounts::delete_session(s.pool(), sess.session.id).await?;
    sess.event(Action::Logout)
        .target("account", sess.account.id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    let mut r = Json(json!({"ok": true})).into_response();
    r.headers_mut()
        .insert(header::SET_COOKIE, session_cookie(&s, "", 0));
    Ok(r)
}

pub(super) async fn me(sess: Session, State(s): State<AppState>) -> ApiResult<Me> {
    Ok(Json(me_for(&s, &sess).await?))
}

#[derive(Deserialize)]
pub(super) struct PasswordChange {
    current: String,
    new: String,
}

pub(super) async fn change_password(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(p): Json<PasswordChange>,
) -> ApiResult<Value> {
    if !accounts::verify_password(&sess.account.password_hash, &p.current) {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "current password is wrong".into(),
        ));
    }
    accounts::check_password(&p.new).map_err(|e| ApiError::bad_request(e.to_string()))?;
    accounts::set_password(s.pool(), sess.account.id, &accounts::hash_password(&p.new)?).await?;
    accounts::delete_other_sessions(s.pool(), sess.account.id, sess.session.id).await?;
    sess.event(Action::PasswordChanged)
        .target("account", sess.account.id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"ok": true})))
}

#[derive(Serialize)]
pub(super) struct InviteInfo {
    email: String,
    project: Option<String>,
    operator: bool,
}

pub(super) async fn invite_info(
    State(s): State<AppState>,
    Path(token): Path<String>,
) -> ApiResult<InviteInfo> {
    let invite = accounts::invite_by_token(s.pool(), &token)
        .await?
        .filter(|i| i.used_at.is_none() && i.expires > Utc::now())
        .ok_or_else(ApiError::not_found)?;
    let project = match invite.tenant_id {
        Some(t) => db::get_tenant(s.pool(), t).await?.map(|t| t.name),
        None => None,
    };
    Ok(Json(InviteInfo {
        email: invite.email,
        project,
        operator: invite.operator,
    }))
}

#[derive(Deserialize)]
pub(super) struct Signup {
    token: String,
    password: String,
}

pub(super) async fn signup(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(i): Json<Signup>,
) -> Result<Response, ApiError> {
    let ip = format!("signup:{}", client_ip(&headers));
    if s.login_limiter.blocked(&ip) {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "too many attempts, try again later".into(),
        ));
    }
    accounts::check_password(&i.password).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let hash = accounts::hash_password(&i.password)?;
    let mut member_change = None;
    let mut tx = s.pool().begin().await?;
    let Some(invite) = accounts::invite_by_token(&mut *tx, &i.token)
        .await?
        .filter(|i| i.used_at.is_none() && i.expires > Utc::now())
    else {
        s.login_limiter.failure(&ip);
        return Err(ApiError::not_found());
    };
    if !accounts::use_invite(&mut *tx, invite.id).await? {
        return Err(ApiError::not_found());
    }
    let existing_account = accounts::account_by_email(s.pool(), &invite.email).await?;
    let account_id = match existing_account {
        Some(existing) => {
            if invite.tenant_id.is_none() {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "an account with this email already exists".into(),
                ));
            }
            if !accounts::verify_password(&existing.password_hash, &i.password) {
                return Err(ApiError(
                    StatusCode::CONFLICT,
                    "this email already has an account: sign in, then ask for a new invite".into(),
                ));
            }
            existing.id
        }
        None => accounts::insert_account(&mut *tx, &invite.email, &hash, invite.operator).await?,
    };
    if let (Some(tenant), Some(role)) = (
        invite.tenant_id,
        invite.role.as_deref().and_then(Role::parse),
    ) {
        let previous = accounts::member_role(s.pool(), tenant, account_id).await?;
        accounts::add_member(&mut *tx, tenant, account_id, role).await?;
        member_change = Some((tenant, previous, role));
    }
    tx.commit().await?;
    let origin = Origin::from_headers(&headers);
    Event::new(Action::InviteUsed, Actor::Account(account_id))
        .target("invite", invite.id)
        .details(json!({"email": invite.email, "operator": invite.operator}))
        .record(s.pool(), &origin)
        .await;
    if let Some((tenant, previous, role)) = member_change {
        let action = match previous {
            Some(p) if p != role => Action::MemberRoleChanged,
            Some(_) => Action::MemberAdded,
            None => Action::MemberAdded,
        };
        Event::new(action, Actor::Account(account_id))
            .tenant(tenant)
            .target("account", account_id)
            .details(json!({"role": role.as_str(), "previous_role": previous.map(|p| p.as_str())}))
            .record(s.pool(), &origin)
            .await;
    }
    let (token, csrf) = accounts::create_session(s.pool(), account_id).await?;
    let account = accounts::account_by_id(s.pool(), account_id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    let projects = accounts::projects_for_account(s.pool(), &account).await?;
    let mut r = Json(Me {
        id: account.id,
        email: account.email,
        is_operator: account.is_operator,
        csrf,
        projects,
    })
    .into_response();
    r.headers_mut().insert(
        header::SET_COOKIE,
        session_cookie(&s, &token, accounts::SESSION_HOURS * 3600),
    );
    Ok(r)
}
