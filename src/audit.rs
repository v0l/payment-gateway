use axum::http::HeaderMap;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::{FromRow, MySqlPool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    LoginOk,
    LoginFailed,
    Logout,
    PasswordChanged,
    PasswordResetRequested,
    PasswordReset,
    EmailChangeRequested,
    EmailChanged,
    NotificationsChanged,
    InviteCreated,
    InviteUsed,
    MemberAdded,
    MemberRemoved,
    MemberRoleChanged,
    ProjectCreated,
    KeyRotated,
    WebhookSecretRotated,
    ProviderCreated,
    ProviderUpdated,
    ProviderEnabled,
    ProviderDisabled,
    ProviderDeleted,
    SettingsChanged,
    RefundRequested,
    RefundResult,
    PaymentCancelled,
    ProjectTrustChanged,
    ProjectEnabledChanged,
    AccountDisabled,
    AccountEnabled,
    DeliveryReplayed,
    LimitsChanged,
    KeysRotated,
    RetentionPurged,
    PrivacyExported,
    PrivacyDeleted,
}

impl Action {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LoginOk => "login.ok",
            Self::LoginFailed => "login.failed",
            Self::Logout => "logout",
            Self::PasswordChanged => "password.changed",
            Self::PasswordResetRequested => "password.reset_requested",
            Self::PasswordReset => "password.reset",
            Self::EmailChangeRequested => "email.change_requested",
            Self::EmailChanged => "email.changed",
            Self::NotificationsChanged => "notifications.changed",
            Self::InviteCreated => "invite.created",
            Self::InviteUsed => "invite.used",
            Self::MemberAdded => "member.added",
            Self::MemberRemoved => "member.removed",
            Self::MemberRoleChanged => "member.role_changed",
            Self::ProjectCreated => "project.created",
            Self::KeyRotated => "key.rotated",
            Self::WebhookSecretRotated => "webhook_secret.rotated",
            Self::ProviderCreated => "provider.created",
            Self::ProviderUpdated => "provider.updated",
            Self::ProviderEnabled => "provider.enabled",
            Self::ProviderDisabled => "provider.disabled",
            Self::ProviderDeleted => "provider.deleted",
            Self::SettingsChanged => "settings.changed",
            Self::RefundRequested => "refund.requested",
            Self::RefundResult => "refund.result",
            Self::PaymentCancelled => "payment.cancelled",
            Self::ProjectTrustChanged => "project.trust_changed",
            Self::ProjectEnabledChanged => "project.enabled_changed",
            Self::AccountDisabled => "account.disabled",
            Self::AccountEnabled => "account.enabled",
            Self::DeliveryReplayed => "delivery.replayed",
            Self::LimitsChanged => "limits.changed",
            Self::KeysRotated => "keys.rotated",
            Self::RetentionPurged => "retention.purged",
            Self::PrivacyExported => "privacy.exported",
            Self::PrivacyDeleted => "privacy.deleted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    Account(u64),
    ApiKey(u64),
    AdminToken,
    System,
}

impl Actor {
    fn parts(&self) -> (&'static str, Option<String>) {
        match self {
            Self::Account(id) => ("account", Some(id.to_string())),
            Self::ApiKey(tenant) => ("api_key", Some(tenant.to_string())),
            Self::AdminToken => ("admin_token", None),
            Self::System => ("system", None),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Origin {
    pub ip: Option<String>,
    pub user_agent: Option<String>,
}

impl Origin {
    pub fn from_headers(headers: &HeaderMap) -> Self {
        let ip = headers
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next_back())
            .map(|s| s.trim().to_string());
        let user_agent = headers
            .get(axum::http::header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        Self { ip, user_agent }
    }
}

pub struct Event {
    action: Action,
    actor: Actor,
    tenant_id: Option<u64>,
    target: Option<(&'static str, String)>,
    details: Value,
}

impl Event {
    pub fn new(action: Action, actor: Actor) -> Self {
        Self {
            action,
            actor,
            tenant_id: None,
            target: None,
            details: Value::Null,
        }
    }

    pub fn tenant(mut self, id: u64) -> Self {
        self.tenant_id = Some(id);
        self
    }

    pub fn target(mut self, kind: &'static str, id: impl ToString) -> Self {
        self.target = Some((kind, id.to_string()));
        self
    }

    pub fn details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub async fn record(self, pool: &MySqlPool, origin: &Origin) {
        let action = self.action;
        if let Err(e) = self.insert(pool, origin).await {
            log::error!("audit write failed for {}: {e}", action.as_str());
        }
    }

    async fn insert(self, pool: &MySqlPool, origin: &Origin) -> Result<(), sqlx::Error> {
        let (kind, actor_id) = self.actor.parts();
        let (target_kind, target_id) = match self.target {
            Some((k, i)) => (Some(k), Some(i)),
            None => (None, None),
        };
        let details = (!self.details.is_null()).then(|| self.details.to_string());
        sqlx::query(
            "insert into audit_event (tenant_id, actor_kind, actor_id, action, target_kind, target_id, ip, user_agent, details, created) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(self.tenant_id)
        .bind(kind)
        .bind(actor_id)
        .bind(self.action.as_str())
        .bind(target_kind)
        .bind(target_id)
        .bind(origin.ip.as_deref().map(|s| clip(s, 64)))
        .bind(origin.user_agent.as_deref().map(|s| clip(s, 255)))
        .bind(details)
        .bind(Utc::now())
        .execute(pool)
        .await?;
        Ok(())
    }
}

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub fn changed(fields: &[(&str, bool)]) -> Value {
    let names: Vec<&str> = fields
        .iter()
        .filter(|(_, changed)| *changed)
        .map(|(name, _)| *name)
        .collect();
    json!({ "changed": names })
}

#[derive(Debug, Default, Clone)]
pub struct Filter {
    pub tenant_id: Option<u64>,
    pub action: Option<String>,
    pub actor: Option<String>,
    pub actor_kind: Option<String>,
    pub before: Option<u64>,
    pub limit: Option<u32>,
}

#[derive(FromRow)]
struct Row {
    id: u64,
    tenant_id: Option<u64>,
    actor_kind: String,
    actor_id: Option<String>,
    actor_email: Option<String>,
    action: String,
    target_kind: Option<String>,
    target_id: Option<String>,
    ip: Option<String>,
    user_agent: Option<String>,
    details: Option<String>,
    created: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct AuditEvent {
    pub id: u64,
    pub tenant_id: Option<u64>,
    pub actor_kind: String,
    pub actor_id: Option<String>,
    pub actor_email: Option<String>,
    pub action: String,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    pub ip: Option<String>,
    pub user_agent: Option<String>,
    pub details: Value,
    pub created: DateTime<Utc>,
}

impl From<Row> for AuditEvent {
    fn from(r: Row) -> Self {
        Self {
            id: r.id,
            tenant_id: r.tenant_id,
            actor_kind: r.actor_kind,
            actor_id: r.actor_id,
            actor_email: r.actor_email,
            action: r.action,
            target_kind: r.target_kind,
            target_id: r.target_id,
            ip: r.ip,
            user_agent: r.user_agent,
            details: r
                .details
                .and_then(|d| serde_json::from_str(&d).ok())
                .unwrap_or(Value::Null),
            created: r.created,
        }
    }
}

pub async fn list(pool: &MySqlPool, f: &Filter) -> anyhow::Result<Vec<AuditEvent>> {
    let mut sql = String::from(
        "select e.id, e.tenant_id, e.actor_kind, e.actor_id, a.email as actor_email, e.action, e.target_kind, e.target_id, e.ip, e.user_agent, e.details, e.created from audit_event e left join account a on e.actor_kind = 'account' and a.id = cast(e.actor_id as unsigned) where 1 = 1",
    );
    if f.tenant_id.is_some() {
        sql.push_str(" and e.tenant_id = ?");
    }
    if f.action.is_some() {
        sql.push_str(" and (e.action = ? or e.action like ?)");
    }
    if f.actor.is_some() {
        sql.push_str(" and (e.actor_id = ? or a.email = ?)");
    }
    if f.actor_kind.is_some() {
        sql.push_str(" and e.actor_kind = ?");
    }
    if f.before.is_some() {
        sql.push_str(" and e.id < ?");
    }
    sql.push_str(" order by e.id desc limit ?");
    let mut q = sqlx::query_as::<_, Row>(&sql);
    if let Some(t) = f.tenant_id {
        q = q.bind(t);
    }
    if let Some(a) = &f.action {
        q = q
            .bind(a.clone())
            .bind(format!("{}.%", a.replace(['%', '_'], "")));
    }
    if let Some(a) = &f.actor {
        q = q.bind(a.clone()).bind(a.to_lowercase());
    }
    if let Some(k) = &f.actor_kind {
        q = q.bind(k.clone());
    }
    if let Some(b) = f.before {
        q = q.bind(b);
    }
    q = q.bind(f.limit.unwrap_or(50).clamp(1, 200));
    Ok(q.fetch_all(pool)
        .await?
        .into_iter()
        .map(Into::into)
        .collect())
}

impl Filter {
    pub fn from_query(
        tenant_id: Option<u64>,
        q: &std::collections::HashMap<String, String>,
    ) -> Self {
        let text = |k: &str| {
            q.get(k)
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        };
        Self {
            tenant_id,
            action: text("action"),
            actor: text("actor"),
            actor_kind: text("actor_kind"),
            before: q.get("before").and_then(|v| v.parse().ok()),
            limit: q.get("limit").and_then(|v| v.parse().ok()),
        }
    }
}
