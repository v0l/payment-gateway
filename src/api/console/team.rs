use super::*;

pub(super) async fn members(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<accounts::MemberRow>> {
    sess.project(&s, id, Role::Viewer).await?;
    Ok(Json(accounts::list_members(s.pool(), id).await?))
}

#[derive(Deserialize)]
pub(super) struct MemberInvite {
    email: String,
    role: String,
}

pub(super) async fn invite_member(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(i): Json<MemberInvite>,
) -> ApiResult<Value> {
    let (_, my_role) = sess.project(&s, id, Role::Admin).await?;
    let role = Role::parse(&i.role)
        .ok_or_else(|| ApiError::bad_request("role must be viewer, admin or owner"))?;
    if role > my_role {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "you cannot grant a role above your own".into(),
        ));
    }
    let email = accounts::normalize_email(&i.email)
        .ok_or_else(|| ApiError::bad_request("invalid email"))?;
    let token = accounts::insert_invite(
        s.pool(),
        &accounts::NewInvite {
            email: &email,
            tenant_id: Some(id),
            role: Some(role.as_str()),
            operator: false,
            created_by: Some(sess.account.id),
        },
    )
    .await?;
    sess.event(Action::InviteCreated)
        .tenant(id)
        .target("invite", &email)
        .details(json!({"email": email, "role": role.as_str()}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(
        json!({"invite_url": format!("{}/console/invite/{token}", s.service.public_url), "email": email, "expires_in_days": accounts::INVITE_DAYS}),
    ))
}

pub(super) async fn remove_member(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, account)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    let (_, my_role) = sess.project(&s, id, Role::Admin).await?;
    let target = accounts::member_role(s.pool(), id, account)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if target > my_role {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "you cannot remove someone above your role".into(),
        ));
    }
    if target == Role::Owner && accounts::owner_count(s.pool(), id).await? <= 1 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "a project needs at least one owner".into(),
        ));
    }
    accounts::remove_member(s.pool(), id, account).await?;
    sess.event(Action::MemberRemoved)
        .tenant(id)
        .target("account", account)
        .details(json!({"role": target.as_str()}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"removed": true})))
}
