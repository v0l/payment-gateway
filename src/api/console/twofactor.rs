use super::*;
use crate::totp;
use crate::twofactor::{self, Factor};

fn limited() -> ApiError {
    ApiError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "too many attempts, try again later".into(),
    )
}

fn factor_key(account_id: u64) -> String {
    format!("2fa:{account_id}")
}

#[derive(Deserialize)]
pub(super) struct SecondStep {
    challenge: String,
    code: Option<String>,
    recovery_code: Option<String>,
}

pub(super) async fn login_second_step(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<SecondStep>,
) -> Result<Response, ApiError> {
    let ip = format!("ip:{}", client_ip(&headers));
    let origin = Origin::from_headers(&headers);
    let Some(account_id) = twofactor::challenge_account(s.pool(), &b.challenge).await? else {
        s.login_limiter.failure(&ip);
        return Err(ApiError::unauthorized_with("login expired, sign in again"));
    };
    let account = accounts::account_by_id(s.pool(), account_id)
        .await?
        .filter(|a| !a.disabled && a.totp_enabled)
        .ok_or_else(ApiError::unauthorized)?;
    let ek = format!("email:{}", account.email);
    if s.login_limiter.blocked(&ek) || s.login_limiter.blocked(&ip) {
        return Err(limited());
    }
    let factor = twofactor::verify_factor(
        s.pool(),
        &s.service.sealer,
        account.id,
        b.code.as_deref(),
        b.recovery_code.as_deref(),
    )
    .await?;
    let Some(factor) = factor else {
        s.login_limiter.failure(&ek);
        s.login_limiter.failure(&ip);
        Event::new(Action::TwoFactorLoginFailed, Actor::Account(account.id))
            .target("account", account.id)
            .record(s.pool(), &origin)
            .await;
        return Err(ApiError::unauthorized_with("wrong code"));
    };
    if !twofactor::consume_challenge(s.pool(), &b.challenge).await? {
        return Err(ApiError::unauthorized_with("login expired, sign in again"));
    }
    s.login_limiter.clear(&ek);
    if factor == Factor::Recovery {
        Event::new(Action::TwoFactorRecoveryUsed, Actor::Account(account.id))
            .target("account", account.id)
            .record(s.pool(), &origin)
            .await;
    }
    Event::new(Action::LoginOk, Actor::Account(account.id))
        .target("account", account.id)
        .details(
            json!({"second_factor": if factor == Factor::Recovery { "recovery" } else { "totp" }}),
        )
        .record(s.pool(), &origin)
        .await;
    accounts::touch_login(s.pool(), account.id).await?;
    let (token, csrf) = accounts::create_session(s.pool(), account.id).await?;
    let projects = accounts::projects_for_account(s.pool(), &account).await?;
    Ok(session_response(&s, account, token, csrf, projects))
}

pub(super) async fn two_factor_status(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<Value> {
    let remaining = twofactor::recovery_codes_remaining(s.pool(), sess.account.id).await?;
    Ok(Json(json!({
        "enabled": sess.account.totp_enabled,
        "recovery_codes_remaining": remaining,
        "operator_grace": sess.account.is_operator && s.operator_2fa_grace,
    })))
}

pub(super) async fn two_factor_setup(sess: Session, State(s): State<AppState>) -> ApiResult<Value> {
    let secret = totp::new_secret();
    if !twofactor::begin_enrolment(s.pool(), &s.service.sealer, sess.account.id, &secret).await? {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "two factor is already enabled".into(),
        ));
    }
    let uri = totp::otpauth_uri("Payment gateway", &sess.account.email, &secret);
    let qr = totp::qr_svg(&uri).map_err(|e| ApiError::bad_request(e.to_string()))?;
    Ok(Json(json!({
        "secret": totp::base32(&secret),
        "otpauth_uri": uri,
        "qr_svg": qr,
    })))
}

#[derive(Deserialize)]
pub(super) struct CodeBody {
    code: String,
}

pub(super) async fn two_factor_activate(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<CodeBody>,
) -> ApiResult<Value> {
    let key = factor_key(sess.account.id);
    if s.login_limiter.blocked(&key) {
        return Err(limited());
    }
    match twofactor::activate(s.pool(), &s.service.sealer, sess.account.id, &b.code).await? {
        Some(codes) => {
            s.login_limiter.clear(&key);
            sess.event(Action::TwoFactorEnabled)
                .target("account", sess.account.id)
                .record(s.pool(), &Origin::from_headers(&headers))
                .await;
            Ok(Json(json!({"enabled": true, "recovery_codes": codes})))
        }
        None => {
            s.login_limiter.failure(&key);
            Err(ApiError::bad_request("wrong code, or setup not started"))
        }
    }
}

#[derive(Deserialize)]
pub(super) struct PasswordAndCode {
    password: String,
    code: String,
}

async fn confirm_password_and_code(
    s: &AppState,
    sess: &Session,
    b: &PasswordAndCode,
) -> Result<(), ApiError> {
    let key = factor_key(sess.account.id);
    if s.login_limiter.blocked(&key) {
        return Err(limited());
    }
    if !sess.account.totp_enabled {
        return Err(ApiError::bad_request("two factor is not enabled"));
    }
    let password_ok = accounts::verify_password(&sess.account.password_hash, &b.password);
    let now = chrono::Utc::now().timestamp() as u64;
    let code_ok = password_ok
        && twofactor::check_totp(
            s.pool(),
            &s.service.sealer,
            sess.account.id,
            true,
            &b.code,
            now,
        )
        .await?;
    if !code_ok {
        s.login_limiter.failure(&key);
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "password or code is wrong".into(),
        ));
    }
    s.login_limiter.clear(&key);
    Ok(())
}

pub(super) async fn two_factor_disable(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<PasswordAndCode>,
) -> ApiResult<Value> {
    confirm_password_and_code(&s, &sess, &b).await?;
    twofactor::disable(s.pool(), sess.account.id).await?;
    sess.event(Action::TwoFactorDisabled)
        .target("account", sess.account.id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"enabled": false})))
}

pub(super) async fn two_factor_recovery_codes(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(b): Json<PasswordAndCode>,
) -> ApiResult<Value> {
    confirm_password_and_code(&s, &sess, &b).await?;
    let codes = twofactor::replace_recovery_codes(s.pool(), sess.account.id).await?;
    sess.event(Action::TwoFactorRecoveryRegenerated)
        .target("account", sess.account.id)
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"recovery_codes": codes})))
}

#[derive(Deserialize)]
pub(super) struct Require2fa {
    required: bool,
}

pub(super) async fn set_require_2fa(
    sess: Session,
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<u64>,
    Json(b): Json<Require2fa>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    if b.required && !sess.account.totp_enabled {
        return Err(ApiError::bad_request(
            "enable two factor on your own account first",
        ));
    }
    db::set_tenant_require_2fa(s.pool(), id, b.required).await?;
    sess.event(Action::ProjectRequire2faChanged)
        .tenant(id)
        .target("project", id)
        .details(json!({"require_2fa": b.required}))
        .record(s.pool(), &Origin::from_headers(&headers))
        .await;
    Ok(Json(json!({"require_2fa": b.required})))
}
