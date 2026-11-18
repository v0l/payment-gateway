use super::{ApiError, ApiResult, AppState};
use crate::accounts::{self, AccountRow, Role, SessionRow};
use crate::crypto::{hash_api_key, new_api_key, new_webhook_secret};
use crate::db::{self, NewProviderConfig, ProviderConfigRow, Tenant, parse_payment_id};
use crate::model::{CreateAttempt, METHODS, PaymentView, RefundInput, RefundView};
use crate::validate::{self, SettingsInput};
use axum::extract::{FromRequestParts, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;

pub const COOKIE: &str = "pgw_session";
const HTML: &str = include_str!("../../web/console.html");
const JS: &str = include_str!("../../web/console.js");
const CSS: &str = include_str!("../../web/console.css");

pub fn router() -> Router<AppState> {
    let api = Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/signup", post(signup))
        .route("/invite/{token}", get(invite_info))
        .route("/me", get(me))
        .route("/password", post(change_password))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/{id}", get(get_project))
        .route("/projects/{id}/settings", post(update_settings))
        .route("/projects/{id}/rotate-key", post(rotate_key))
        .route(
            "/projects/{id}/rotate-webhook-secret",
            post(rotate_webhook_secret),
        )
        .route(
            "/projects/{id}/providers",
            get(list_providers).post(create_provider),
        )
        .route(
            "/projects/{id}/providers/{pid}",
            post(update_provider).delete(remove_provider),
        )
        .route(
            "/projects/{id}/providers/{pid}/enabled",
            post(set_provider_enabled),
        )
        .route(
            "/projects/{id}/payments",
            get(list_payments).post(create_payment),
        )
        .route("/projects/{id}/payments/{payment}", get(get_payment))
        .route(
            "/projects/{id}/payments/{payment}/attempts",
            post(create_attempt),
        )
        .route(
            "/projects/{id}/payments/{payment}/cancel",
            post(cancel_payment),
        )
        .route(
            "/projects/{id}/payments/{payment}/refund",
            post(refund_payment),
        )
        .route("/projects/{id}/stats", get(stats))
        .route("/projects/{id}/deliveries", get(deliveries))
        .route("/projects/{id}/deliveries/{delivery}/replay", post(replay))
        .route("/projects/{id}/members", get(members))
        .route("/projects/{id}/invites", post(invite_member))
        .route("/projects/{id}/members/{account}", delete(remove_member))
        .route("/admin/accounts", get(admin_accounts))
        .route("/admin/accounts/{id}/disabled", post(admin_disable))
        .route("/admin/invites", post(admin_invite))
        .route("/admin/projects/{id}/trusted", post(admin_trusted));
    Router::new()
        .route("/console", get(page))
        .route("/console/{*rest}", get(page_or_asset))
        .nest("/console/api", api)
}

fn security_headers(h: &mut HeaderMap) {
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: https:; connect-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'",
        ),
    );
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
}

async fn page() -> Response {
    let mut r = (StatusCode::OK, HTML).into_response();
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    security_headers(r.headers_mut());
    r
}

async fn page_or_asset(Path(rest): Path<String>) -> Response {
    match rest.as_str() {
        "app.js" => asset(JS, "text/javascript"),
        "app.css" => asset(CSS, "text/css"),
        _ => page().await,
    }
}

fn asset(body: &'static str, content_type: &'static str) -> Response {
    let mut r = (StatusCode::OK, body).into_response();
    r.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    r.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    r.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    r
}

pub struct Session {
    pub session: SessionRow,
    pub account: AccountRow,
}

fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|p| p.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v)
}

impl FromRequestParts<AppState> for Session {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = cookie_value(&parts.headers, COOKIE).ok_or_else(ApiError::unauthorized)?;
        let (session, account) = accounts::session_by_token(state.pool(), token)
            .await?
            .ok_or_else(ApiError::unauthorized)?;
        if parts.method != Method::GET && parts.method != Method::HEAD {
            let sent = parts
                .headers
                .get("x-csrf-token")
                .and_then(|v| v.to_str().ok());
            if sent != Some(session.csrf.as_str()) {
                return Err(ApiError(StatusCode::FORBIDDEN, "bad csrf token".into()));
            }
        }
        Ok(Session { session, account })
    }
}

impl Session {
    async fn project(&self, s: &AppState, id: u64, min: Role) -> Result<(Tenant, Role), ApiError> {
        let tenant = db::get_tenant(s.pool(), id)
            .await?
            .ok_or_else(ApiError::not_found)?;
        let role = if self.account.is_operator {
            Role::Owner
        } else {
            accounts::member_role(s.pool(), id, self.account.id)
                .await?
                .ok_or_else(ApiError::not_found)?
        };
        if role < min {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                format!("needs {} access", min.as_str()),
            ));
        }
        Ok((tenant, role))
    }

    fn operator(&self) -> Result<(), ApiError> {
        if self.account.is_operator {
            Ok(())
        } else {
            Err(ApiError(StatusCode::FORBIDDEN, "operators only".into()))
        }
    }
}

fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next_back())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "local".into())
}

fn session_cookie(s: &AppState, token: &str, max_age: i64) -> HeaderValue {
    let secure = if s.service.public_url.starts_with("https://") {
        "; Secure"
    } else {
        ""
    };
    HeaderValue::from_str(&format!(
        "{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/console; Max-Age={max_age}{secure}"
    ))
    .expect("cookie")
}

#[derive(Deserialize)]
struct Credentials {
    email: String,
    password: String,
}

#[derive(Serialize)]
struct Me {
    id: u64,
    email: String,
    is_operator: bool,
    csrf: String,
    projects: Vec<accounts::ProjectRef>,
}

async fn me_for(s: &AppState, sess: &Session) -> Result<Me, ApiError> {
    Ok(Me {
        id: sess.account.id,
        email: sess.account.email.clone(),
        is_operator: sess.account.is_operator,
        csrf: sess.session.csrf.clone(),
        projects: accounts::projects_for_account(s.pool(), &sess.account).await?,
    })
}

async fn login(
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
        return Err(ApiError::unauthorized_with("wrong email or password"));
    }
    let account = account.unwrap();
    s.login_limiter.clear(&ek);
    accounts::touch_login(s.pool(), account.id).await?;
    let (token, csrf) = accounts::create_session(s.pool(), account.id).await?;
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

async fn logout(sess: Session, State(s): State<AppState>) -> Result<Response, ApiError> {
    accounts::delete_session(s.pool(), sess.session.id).await?;
    let mut r = Json(json!({"ok": true})).into_response();
    r.headers_mut()
        .insert(header::SET_COOKIE, session_cookie(&s, "", 0));
    Ok(r)
}

async fn me(sess: Session, State(s): State<AppState>) -> ApiResult<Me> {
    Ok(Json(me_for(&s, &sess).await?))
}

#[derive(Deserialize)]
struct PasswordChange {
    current: String,
    new: String,
}

async fn change_password(
    sess: Session,
    State(s): State<AppState>,
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
    Ok(Json(json!({"ok": true})))
}

#[derive(Serialize)]
struct InviteInfo {
    email: String,
    project: Option<String>,
    operator: bool,
}

async fn invite_info(
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
struct Signup {
    token: String,
    password: String,
}

async fn signup(
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
    let account_id = match accounts::account_by_email(s.pool(), &invite.email).await? {
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
        accounts::add_member(&mut *tx, tenant, account_id, role).await?;
    }
    tx.commit().await?;
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

#[derive(Serialize)]
struct ProjectView {
    id: u64,
    name: String,
    role: Role,
    trusted: bool,
    webhook_url: Option<String>,
    display_name: Option<String>,
    logo_url: Option<String>,
    accent_color: Option<String>,
    allowed_origins: Vec<String>,
    default_success_url: Option<String>,
    default_cancel_url: Option<String>,
    enabled: bool,
}

fn project_view(t: Tenant, role: Role) -> ProjectView {
    let allowed_origins = t.origins();
    ProjectView {
        id: t.id,
        name: t.name,
        role,
        trusted: t.trusted,
        webhook_url: t.webhook_url,
        display_name: t.display_name,
        logo_url: t.logo_url,
        accent_color: t.accent_color,
        allowed_origins,
        default_success_url: t.default_success_url,
        default_cancel_url: t.default_cancel_url,
        enabled: t.enabled,
    }
}

async fn list_projects(
    sess: Session,
    State(s): State<AppState>,
) -> ApiResult<Vec<accounts::ProjectRef>> {
    Ok(Json(
        accounts::projects_for_account(s.pool(), &sess.account).await?,
    ))
}

#[derive(Deserialize)]
struct NewProject {
    name: String,
}

async fn create_project(
    sess: Session,
    State(s): State<AppState>,
    Json(p): Json<NewProject>,
) -> ApiResult<Value> {
    let name = p.name.trim().to_string();
    if name.is_empty() || name.len() > 64 {
        return Err(ApiError::bad_request("name must be 1-64 characters"));
    }
    let owned = accounts::projects_for_account(s.pool(), &sess.account).await?;
    if !sess.account.is_operator && owned.len() >= s.max_projects {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            format!("project limit of {} reached", s.max_projects),
        ));
    }
    let key = new_api_key();
    let secret = new_webhook_secret();
    let sealed = s.service.sealer.seal(secret.as_bytes())?;
    let mut tx = s.pool().begin().await?;
    let id = accounts::insert_project(&mut *tx, &name, &hash_api_key(&key), &sealed).await?;
    accounts::add_member(&mut *tx, id, sess.account.id, Role::Owner).await?;
    tx.commit().await?;
    Ok(Json(
        json!({"id": id, "name": name, "api_key": key, "webhook_secret": secret}),
    ))
}

async fn get_project(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<ProjectView> {
    let (t, role) = sess.project(&s, id, Role::Viewer).await?;
    Ok(Json(project_view(t, role)))
}

async fn update_settings(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<SettingsInput>,
) -> ApiResult<ProjectView> {
    let (t, role) = sess.project(&s, id, Role::Admin).await?;
    let settings = validate::settings(t.trusted, req)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    db::update_tenant_settings(s.pool(), id, &settings).await?;
    let t = db::get_tenant(s.pool(), id)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(project_view(t, role)))
}

async fn rotate_key(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    let key = new_api_key();
    db::rotate_tenant_key(s.pool(), id, &hash_api_key(&key)).await?;
    Ok(Json(json!({"api_key": key})))
}

async fn rotate_webhook_secret(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    let secret = new_webhook_secret();
    accounts::set_webhook_secret(s.pool(), id, &s.service.sealer.seal(secret.as_bytes())?).await?;
    Ok(Json(json!({"webhook_secret": secret})))
}

#[derive(Serialize)]
struct ProviderView {
    id: u64,
    method: String,
    provider_type: String,
    name: String,
    enabled: bool,
    fee_rate: Option<f64>,
    fee_base: Option<u64>,
    fee_currency: Option<String>,
    config: Value,
    hook_url: String,
}

fn provider_view(s: &AppState, tenant: u64, p: &ProviderConfigRow) -> ProviderView {
    let config = s
        .service
        .registry
        .open_config(p)
        .map(|c| validate::redacted(&c))
        .unwrap_or(Value::Null);
    ProviderView {
        id: p.id,
        method: p.method.clone(),
        provider_type: p.provider_type.clone(),
        name: p.name.clone(),
        enabled: p.enabled,
        fee_rate: p.fee_rate,
        fee_base: p.fee_base,
        fee_currency: p.fee_currency.clone(),
        config,
        hook_url: s.service.hook_url(tenant, p.id),
    }
}

async fn list_providers(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<ProviderView>> {
    sess.project(&s, id, Role::Viewer).await?;
    Ok(Json(
        db::list_provider_configs(s.pool(), id, false)
            .await?
            .iter()
            .map(|p| provider_view(&s, id, p))
            .collect(),
    ))
}

#[derive(Deserialize)]
struct ProviderInput {
    method: String,
    provider_type: String,
    name: String,
    config: Value,
    fee_rate: Option<f64>,
    fee_base: Option<u64>,
    fee_currency: Option<String>,
}

fn check_fees(
    rate: Option<f64>,
    currency: &Option<String>,
    base: Option<u64>,
) -> Result<(), ApiError> {
    if rate.is_some_and(|r| !(0.0..=100.0).contains(&r) || r.is_nan()) {
        return Err(ApiError::bad_request("fee_rate must be 0-100"));
    }
    if base.is_some()
        && currency
            .as_deref()
            .map(|c| crate::rates::unit(c).is_err())
            .unwrap_or(true)
    {
        return Err(ApiError::bad_request(
            "fee_currency must be a supported fiat or BTC currency",
        ));
    }
    Ok(())
}

async fn create_provider(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(req): Json<ProviderInput>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    if !METHODS.contains(&req.method.as_str()) {
        return Err(ApiError::bad_request("unknown method"));
    }
    let name = req.name.trim();
    if name.is_empty() || name.len() > 128 {
        return Err(ApiError::bad_request("name must be 1-128 characters"));
    }
    check_fees(req.fee_rate, &req.fee_currency, req.fee_base)?;
    validate::provider_config(
        t.trusted,
        s.allow_mock,
        &req.method,
        &req.provider_type,
        &req.config,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    let sealed = s
        .service
        .sealer
        .seal(&serde_json::to_vec(&req.config).map_err(anyhow::Error::from)?)?;
    let pid = db::insert_provider_config(
        s.pool(),
        NewProviderConfig {
            tenant_id: id,
            method: &req.method,
            provider_type: &req.provider_type,
            name,
            sealed_config: &sealed,
            fee_rate: req.fee_rate,
            fee_base: req.fee_base,
            fee_currency: req.fee_currency.as_deref(),
        },
    )
    .await?;
    Ok(Json(
        json!({"id": pid, "hook_url": s.service.hook_url(id, pid)}),
    ))
}

async fn update_provider(
    sess: Session,
    State(s): State<AppState>,
    Path((id, pid)): Path<(u64, u64)>,
    Json(req): Json<ProviderInput>,
) -> ApiResult<ProviderView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let row = db::get_tenant_provider(s.pool(), id, pid)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if req.method != row.method || req.provider_type != row.provider_type {
        return Err(ApiError::bad_request(
            "method and provider_type cannot change",
        ));
    }
    check_fees(req.fee_rate, &req.fee_currency, req.fee_base)?;
    let old = s.service.registry.open_config(&row)?;
    let merged = validate::merge_update(&old, &req.config);
    validate::provider_config(
        t.trusted,
        s.allow_mock,
        &row.method,
        &row.provider_type,
        &merged,
    )
    .await
    .map_err(|e| ApiError::bad_request(e.to_string()))?;
    s.service.registry.persist_config(pid, &merged).await?;
    db::update_provider_fees(
        s.pool(),
        pid,
        req.name.trim(),
        req.fee_rate,
        req.fee_base,
        req.fee_currency.as_deref(),
    )
    .await?;
    s.service.registry.invalidate(pid);
    let row = db::get_tenant_provider(s.pool(), id, pid)
        .await?
        .ok_or_else(ApiError::not_found)?;
    Ok(Json(provider_view(&s, id, &row)))
}

#[derive(Deserialize)]
struct Enabled {
    enabled: bool,
}

async fn set_provider_enabled(
    sess: Session,
    State(s): State<AppState>,
    Path((id, pid)): Path<(u64, u64)>,
    Json(e): Json<Enabled>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    if !db::set_provider_config_enabled(s.pool(), id, pid, e.enabled).await? {
        return Err(ApiError::not_found());
    }
    s.service.registry.invalidate(pid);
    Ok(Json(json!({"enabled": e.enabled})))
}

async fn remove_provider(
    sess: Session,
    State(s): State<AppState>,
    Path((id, pid)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Owner).await?;
    db::get_tenant_provider(s.pool(), id, pid)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if db::provider_attempt_count(s.pool(), pid).await? > 0 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "this provider has payment history, disable it instead".into(),
        ));
    }
    db::delete_provider(s.pool(), id, pid).await?;
    s.service.registry.invalidate(pid);
    Ok(Json(json!({"deleted": true})))
}

async fn list_payments(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let status = q.get("status").filter(|v| !v.is_empty());
    if status.is_some_and(|v| {
        ![
            "pending",
            "paid",
            "expired",
            "cancelled",
            "refunded",
            "partially_refunded",
        ]
        .contains(&v.as_str())
    }) {
        return Err(ApiError::bad_request("unknown status"));
    }
    let before = q
        .get("before")
        .filter(|v| !v.is_empty())
        .map(|v| parse_cursor(v))
        .transpose()
        .map_err(|_| ApiError::bad_request("before must be a cursor from a previous page"))?;
    let search = q.get("q").map(|v| v.trim()).filter(|v| !v.is_empty());
    let rows =
        db::list_payments(s.pool(), id, status.map(String::as_str), search, before, 51).await?;
    let more = rows.len() > 50;
    let mut out = vec![];
    for row in rows.iter().take(50) {
        out.push(s.service.view(row).await?);
    }
    let next = if more {
        out.last()
            .map(|p: &PaymentView| format!("{}_{}", p.created.timestamp(), p.id))
    } else {
        None
    };
    Ok(Json(json!({"payments": out, "next": next})))
}

fn parse_cursor(v: &str) -> Result<(DateTime<Utc>, Vec<u8>), ()> {
    let (secs, id) = v.split_once('_').ok_or(())?;
    let at = DateTime::from_timestamp(secs.parse().map_err(|_| ())?, 0).ok_or(())?;
    Ok((at, hex::decode(id).map_err(|_| ())?))
}

async fn get_payment(
    sess: Session,
    State(s): State<AppState>,
    Path((id, payment)): Path<(u64, String)>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Viewer).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    let row = s.service.payment_for_tenant(&t, &pid).await?;
    let attempts = db::attempts_for_payment(s.pool(), &pid).await?;
    let mut receipts = vec![];
    for a in &attempts {
        for r in db::receipts_for_attempt(s.pool(), a.id).await? {
            receipts.push(json!({"attempt_id": a.id, "ref": r.r#ref, "amount": r.amount, "confirmed": r.confirmed}));
        }
    }
    let deliveries: Vec<Value> = db::deliveries_for_payment(s.pool(), &pid)
        .await?
        .into_iter()
        .map(|d| json!({"id": d.id, "event_type": d.event_type, "attempts": d.attempts, "delivered_at": d.delivered_at, "abandoned_at": d.abandoned_at, "last_error": d.last_error}))
        .collect();
    Ok(Json(json!({
        "payment": s.service.view(&row).await?,
        "receipts": receipts,
        "deliveries": deliveries,
    })))
}

async fn create_payment(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(input): Json<crate::model::CreatePayment>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let (row, _) = s.service.create_payment(&t, input, None).await?;
    Ok(Json(
        serde_json::to_value(s.service.view(&row).await?).map_err(anyhow::Error::from)?,
    ))
}

#[derive(Deserialize)]
struct AttemptInput {
    method: String,
    provider_id: Option<u64>,
    option: Option<String>,
}

async fn create_attempt(
    sess: Session,
    State(s): State<AppState>,
    Path((id, payment)): Path<(u64, String)>,
    Json(a): Json<AttemptInput>,
) -> ApiResult<Value> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    s.service
        .create_attempt(
            &t,
            &pid,
            CreateAttempt {
                method: a.method,
                provider_id: a.provider_id,
                option: a.option,
                ..Default::default()
            },
        )
        .await?;
    let row = s.service.payment_for_tenant(&t, &pid).await?;
    Ok(Json(
        serde_json::to_value(s.service.view(&row).await?).map_err(anyhow::Error::from)?,
    ))
}

async fn cancel_payment(
    sess: Session,
    State(s): State<AppState>,
    Path((id, payment)): Path<(u64, String)>,
) -> ApiResult<PaymentView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    let row = s.service.cancel_payment(&t, &pid).await?;
    Ok(Json(s.service.view(&row).await?))
}

async fn refund_payment(
    sess: Session,
    State(s): State<AppState>,
    Path((id, payment)): Path<(u64, String)>,
    Json(input): Json<RefundInput>,
) -> ApiResult<RefundView> {
    let (t, _) = sess.project(&s, id, Role::Admin).await?;
    let pid = parse_payment_id(&payment).ok_or_else(ApiError::not_found)?;
    Ok(Json(s.service.refund(&t, &pid, input).await?))
}

async fn stats(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Viewer).await?;
    let days = q
        .get("days")
        .and_then(|d| d.parse::<i64>().ok())
        .unwrap_or(30)
        .clamp(1, 365);
    let since = Utc::now() - Duration::days(days);
    Ok(Json(json!({
        "days": days,
        "by_status": db::payment_stats(s.pool(), id, since).await?,
        "paid_by_day": db::paid_by_day(s.pool(), id, since).await?,
        "by_method": db::method_counts(s.pool(), id, since).await?.into_iter().map(|(m, c)| json!({"method": m, "count": c})).collect::<Vec<_>>(),
        "pending_deliveries": db::pending_delivery_count(s.pool(), id).await?,
    })))
}

async fn deliveries(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Query(q): Query<HashMap<String, String>>,
) -> ApiResult<Vec<db::DeliveryDetail>> {
    sess.project(&s, id, Role::Viewer).await?;
    let only = q
        .get("undelivered")
        .is_some_and(|v| v == "1" || v == "true");
    Ok(Json(db::deliveries_for_tenant(s.pool(), id, only).await?))
}

async fn replay(
    sess: Session,
    State(s): State<AppState>,
    Path((id, delivery)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    sess.project(&s, id, Role::Admin).await?;
    if !db::replay_tenant_delivery(s.pool(), id, delivery, Utc::now()).await? {
        return Err(ApiError::not_found());
    }
    Ok(Json(json!({"queued": true})))
}

async fn members(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
) -> ApiResult<Vec<accounts::MemberRow>> {
    sess.project(&s, id, Role::Viewer).await?;
    Ok(Json(accounts::list_members(s.pool(), id).await?))
}

#[derive(Deserialize)]
struct MemberInvite {
    email: String,
    role: String,
}

async fn invite_member(
    sess: Session,
    State(s): State<AppState>,
    Path(id): Path<u64>,
    Json(i): Json<MemberInvite>,
) -> ApiResult<Value> {
    let (_, my_role) = sess.project(&s, id, Role::Admin).await?;
    let role = Role::parse(&i.role)
        .ok_or_else(|| ApiError::bad_request("role must be viewer, admin or owner"))?;
    if role > my_role {
        return Err(ApiError(
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
    Ok(Json(
        json!({"invite_url": format!("{}/console/invite/{token}", s.service.public_url), "email": email, "expires_in_days": accounts::INVITE_DAYS}),
    ))
}

async fn remove_member(
    sess: Session,
    State(s): State<AppState>,
    Path((id, account)): Path<(u64, u64)>,
) -> ApiResult<Value> {
    let (_, my_role) = sess.project(&s, id, Role::Admin).await?;
    let target = accounts::member_role(s.pool(), id, account)
        .await?
        .ok_or_else(ApiError::not_found)?;
    if target > my_role {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "you cannot remove someone above your role".into(),
        ));
    }
    if target == Role::Owner && accounts::owner_count(s.pool(), id).await? <= 1 {
        return Err(ApiError(
            StatusCode::CONFLICT,
            "a project needs at least one owner".into(),
        ));
    }
    accounts::remove_member(s.pool(), id, account).await?;
    Ok(Json(json!({"removed": true})))
}

#[derive(Serialize)]
struct AccountView {
    id: u64,
    email: String,
    is_operator: bool,
    disabled: bool,
    created: DateTime<Utc>,
    last_login: Option<DateTime<Utc>>,
}

async fn admin_accounts(sess: Session, State(s): State<AppState>) -> ApiResult<Vec<AccountView>> {
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

async fn admin_disable(
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
struct OperatorInvite {
    email: String,
    #[serde(default)]
    operator: bool,
}

async fn admin_invite(
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
struct Trusted {
    trusted: bool,
}

async fn admin_trusted(
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
