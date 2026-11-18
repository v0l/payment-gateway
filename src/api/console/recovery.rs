use super::*;
use crate::account_tokens::{self, EMAIL_CHANGE, RESET};
use crate::audit::{Action, Actor, Event, Origin};
use crate::mailer::templates;

#[derive(Deserialize)]
pub(super) struct ResetRequest {
    email: String,
}

fn too_many() -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "too many attempts, try again later".into(),
    )
}

async fn send_reset(s: AppState, email: String, origin: Origin) {
    let result: anyhow::Result<()> = async {
        let Some(account) = accounts::account_by_email(s.pool(), &email).await? else {
            return Ok(());
        };
        if account.disabled {
            return Ok(());
        }
        let token = account_tokens::issue(s.pool(), account.id, RESET, None).await?;
        let link = format!("{}/console/reset/{token}", s.service.public_url);
        s.service
            .send_mail(templates::password_reset(&account.email, &link));
        Event::new(Action::PasswordResetRequested, Actor::System)
            .target("account", account.id)
            .record(s.pool(), &origin)
            .await;
        Ok(())
    }
    .await;
    if let Err(e) = result {
        log::error!("password reset failed: {e:#}");
    }
}

pub(super) async fn request_reset(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(r): Json<ResetRequest>,
) -> ApiResult<Value> {
    let email = accounts::normalize_email(&r.email).unwrap_or_default();
    if let Err(secs) = s.rate_limits.reset_email.check(&email) {
        return Err(ApiError::rate_limited(secs));
    }
    if s.service.mailer.enabled() && !email.is_empty() {
        tokio::spawn(send_reset(s.clone(), email, Origin::from_headers(&headers)));
    }
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
pub(super) struct ResetConfirm {
    token: String,
    password: String,
}

pub(super) async fn confirm_reset(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(c): Json<ResetConfirm>,
) -> ApiResult<Value> {
    accounts::check_password(&c.password).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let invalid = || ApiError::bad_request("this reset link is not valid or has expired");
    let Some(token) = account_tokens::find_valid(s.pool(), RESET, &c.token).await? else {
        return Err(invalid());
    };
    if !account_tokens::consume(s.pool(), token.id).await? {
        return Err(invalid());
    }
    let hash = accounts::hash_password(&c.password)?;
    accounts::set_password(s.pool(), token.account_id, &hash).await?;
    accounts::delete_all_sessions(s.pool(), token.account_id).await?;
    account_tokens::revoke_all(s.pool(), token.account_id, RESET).await?;
    Event::new(Action::PasswordReset, Actor::Account(token.account_id))
        .target("account", token.account_id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"ok": true})))
}

#[derive(Deserialize)]
pub(super) struct EmailChange {
    password: String,
    new_email: String,
}

pub(super) async fn request_email_change(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(c): Json<EmailChange>,
) -> ApiResult<Value> {
    if !s.service.mailer.enabled() {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "email is not configured on this gateway".into(),
        ));
    }
    let key = format!("email-change:{}", sess.account.id);
    if s.login_limiter.blocked(&key) {
        return Err(too_many());
    }
    if !accounts::verify_password(&sess.account.password_hash, &c.password) {
        s.login_limiter.failure(&key);
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "current password is wrong".into(),
        ));
    }
    let new_email = accounts::normalize_email(&c.new_email)
        .ok_or_else(|| ApiError::bad_request("invalid email"))?;
    if accounts::account_by_email(s.pool(), &new_email)
        .await?
        .is_some()
    {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "an account with this email already exists".into(),
        ));
    }
    account_tokens::revoke_all(s.pool(), sess.account.id, EMAIL_CHANGE).await?;
    let token =
        account_tokens::issue(s.pool(), sess.account.id, EMAIL_CHANGE, Some(&new_email)).await?;
    let link = format!("{}/console/confirm-email/{token}", s.service.public_url);
    s.service
        .send_mail(templates::email_change(&new_email, &link));
    sess.event(Action::EmailChangeRequested)
        .target("account", sess.account.id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"ok": true, "email": new_email})))
}

#[derive(Deserialize)]
pub(super) struct EmailConfirm {
    token: String,
}

pub(super) async fn confirm_email_change(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(c): Json<EmailConfirm>,
) -> ApiResult<Value> {
    let invalid = || ApiError::bad_request("this confirmation link is not valid or has expired");
    let Some(token) = account_tokens::find_valid(s.pool(), EMAIL_CHANGE, &c.token).await? else {
        return Err(invalid());
    };
    let new_email = token.new_email.clone().ok_or_else(invalid)?;
    let account = accounts::account_by_id(s.pool(), token.account_id)
        .await?
        .ok_or_else(invalid)?;
    if !account_tokens::consume(s.pool(), token.id).await? {
        return Err(invalid());
    }
    accounts::set_email(s.pool(), account.id, &new_email).await?;
    account_tokens::revoke_all(s.pool(), account.id, EMAIL_CHANGE).await?;
    s.service
        .send_mail(templates::email_changed(&account.email, &new_email));
    Event::new(Action::EmailChanged, Actor::Account(account.id))
        .target("account", account.id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"ok": true, "email": new_email})))
}

#[derive(Deserialize)]
pub(super) struct NotifySetting {
    enabled: bool,
}

pub(super) async fn set_notifications(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(n): Json<NotifySetting>,
) -> ApiResult<Value> {
    accounts::set_notify(s.pool(), sess.account.id, n.enabled).await?;
    sess.event(Action::NotificationsChanged)
        .target("account", sess.account.id)
        .details(json!({"enabled": n.enabled}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"notify": n.enabled})))
}
