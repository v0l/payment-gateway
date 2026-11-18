use super::{ApiError, ApiResult, AppState, client_ip};
use crate::accounts::{self, AccountRow, Role, SessionRow};
use crate::audit::{self, Action, Actor, Event, Origin};
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

mod activity;
mod balance;
mod operator;
mod payments;
mod projects;
mod providers;
mod recovery;
mod session;
mod team;
mod twofactor;

use activity::*;
use balance::*;
use operator::*;
use payments::*;
pub(crate) use projects::settings_diff;
use projects::*;
use providers::*;
use recovery::*;
use session::*;
use team::*;
use twofactor::*;

pub const COOKIE: &str = "pgw_session";
const HTML: &str = include_str!("../../../web/console.html");
const JS_CORE: &str = include_str!("../../../web/console/core.js");
const JS_AUTH: &str = include_str!("../../../web/console/auth.js");
const JS_PROJECTS: &str = include_str!("../../../web/console/projects.js");
const JS_PAYMENTS: &str = include_str!("../../../web/console/payments.js");
const JS_PROVIDERS: &str = include_str!("../../../web/console/providers.js");
const JS_WEBHOOKS: &str = include_str!("../../../web/console/webhooks.js");
const JS_TEAM: &str = include_str!("../../../web/console/team.js");
const JS_SETTINGS: &str = include_str!("../../../web/console/settings.js");
const JS_ACCOUNT: &str = include_str!("../../../web/console/account.js");
const JS_ADMIN: &str = include_str!("../../../web/console/admin.js");
const JS_ACTIVITY: &str = include_str!("../../../web/console/activity.js");
const JS_BALANCE: &str = include_str!("../../../web/console/balance.js");
const JS_APP: &str = include_str!("../../../web/console/app.js");
const CSS: &str = include_str!("../../../web/console.css");
const TOKENS_CSS: &str = include_str!("../../../web/tokens.css");
const COMPONENTS_CSS: &str = include_str!("../../../web/components.css");
const STYLEGUIDE_HTML: &str = include_str!("../../../web/styleguide.html");
const STYLEGUIDE_JS: &str = include_str!("../../../web/styleguide.js");

pub fn router() -> Router<AppState> {
    let api = Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/signup", post(signup))
        .route("/reset", post(request_reset))
        .route("/reset/confirm", post(confirm_reset))
        .route("/email", post(request_email_change))
        .route("/email/confirm", post(confirm_email_change))
        .route("/notifications", post(set_notifications))
        .route("/invite/{token}", get(invite_info))
        .route("/me", get(me))
        .route("/login/2fa", post(login_second_step))
        .route("/password", post(change_password))
        .route("/2fa", get(two_factor_status))
        .route("/2fa/setup", post(two_factor_setup))
        .route("/2fa/activate", post(two_factor_activate))
        .route("/2fa/disable", post(two_factor_disable))
        .route("/2fa/recovery-codes", post(two_factor_recovery_codes))
        .route("/projects/{id}/require-2fa", post(set_require_2fa))
        .route("/projects", get(list_projects).post(create_project))
        .route("/projects/{id}", get(get_project))
        .route("/projects/{id}/settings", post(update_settings))
        .route("/projects/{id}/limits", get(get_limits))
        .route("/admin/projects/{id}/limits", post(set_limits))
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
        .route("/projects/{id}/activity", get(project_activity))
        .route("/projects/{id}/balance", get(project_balance))
        .route("/projects/{id}/ledger", get(project_ledger))
        .route("/admin/audit", get(admin_audit))
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
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
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
        "core.js" => asset(JS_CORE, "text/javascript"),
        "auth.js" => asset(JS_AUTH, "text/javascript"),
        "projects.js" => asset(JS_PROJECTS, "text/javascript"),
        "payments.js" => asset(JS_PAYMENTS, "text/javascript"),
        "providers.js" => asset(JS_PROVIDERS, "text/javascript"),
        "webhooks.js" => asset(JS_WEBHOOKS, "text/javascript"),
        "team.js" => asset(JS_TEAM, "text/javascript"),
        "settings.js" => asset(JS_SETTINGS, "text/javascript"),
        "account.js" => asset(JS_ACCOUNT, "text/javascript"),
        "admin.js" => asset(JS_ADMIN, "text/javascript"),
        "balance.js" => asset(JS_BALANCE, "text/javascript"),
        "activity.js" => asset(JS_ACTIVITY, "text/javascript"),
        "app.js" => asset(JS_APP, "text/javascript"),
        "app.css" => asset(CSS, "text/css"),
        "tokens.css" => asset(TOKENS_CSS, "text/css"),
        "components.css" => asset(COMPONENTS_CSS, "text/css"),
        "styleguide.js" => asset(STYLEGUIDE_JS, "text/javascript"),
        "styleguide" => styleguide(),
        _ => page().await,
    }
}

fn styleguide() -> Response {
    let mut r = (StatusCode::OK, STYLEGUIDE_HTML).into_response();
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    security_headers(r.headers_mut());
    r
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
            let matches = sent.is_some_and(|s| {
                subtle::ConstantTimeEq::ct_eq(s.as_bytes(), session.csrf.as_bytes()).into()
            });
            if !matches {
                return Err(ApiError::new(
                    StatusCode::FORBIDDEN,
                    "bad csrf token".into(),
                ));
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
        if self.second_factor_missing(s, &tenant) {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "two factor authentication required: enable it in Account settings".into(),
            ));
        }
        let role = if self.account.is_operator {
            Role::Owner
        } else {
            accounts::member_role(s.pool(), id, self.account.id)
                .await?
                .ok_or_else(ApiError::not_found)?
        };
        if role < min {
            return Err(ApiError::new(
                StatusCode::FORBIDDEN,
                format!("needs {} access", min.as_str()),
            ));
        }
        crate::logging::set_tenant(tenant.id);
        Ok((tenant, role))
    }

    fn event(&self, action: Action) -> Event {
        Event::new(action, Actor::Account(self.account.id))
    }

    fn operator_needs_2fa(&self, s: &AppState) -> bool {
        self.account.is_operator && !self.account.totp_enabled && !s.operator_2fa_grace
    }

    fn second_factor_missing(&self, s: &AppState, tenant: &Tenant) -> bool {
        !self.account.totp_enabled && (tenant.require_2fa || self.operator_needs_2fa(s))
    }

    fn operator(&self, s: &AppState) -> Result<(), ApiError> {
        if self.operator_needs_2fa(s) {
            Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "two factor authentication required: enable it in Account settings".into(),
            ))
        } else if self.account.is_operator {
            Ok(())
        } else {
            Err(ApiError::new(
                StatusCode::FORBIDDEN,
                "operators only".into(),
            ))
        }
    }
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
