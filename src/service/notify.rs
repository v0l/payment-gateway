use super::*;
use crate::mailer::{Message, templates};

pub const NOTIFY_INTERVAL_SECS: i64 = 3600;
pub const PROVIDER_FAILURES_BEFORE_NOTICE: u32 = 3;

pub const EVENT_WEBHOOK_ABANDONED: &str = "webhook_abandoned";
pub const EVENT_PROVIDER_FAILING: &str = "provider_failing";
pub const EVENT_REFUND_FAILED: &str = "refund_failed";

impl Service {
    pub fn send_mail(&self, message: Message) {
        if !self.mailer.enabled() {
            return;
        }
        let mailer = self.mailer.clone();
        tokio::spawn(async move {
            if let Err(e) = mailer.send(message).await {
                log::warn!("sending email failed: {e:#}");
            }
        });
    }

    pub fn send_receipt_mail(&self, tenant: &Tenant, p: &PaymentRow) {
        let Some(to) = p.customer_email.as_deref() else {
            return;
        };
        let seller = tenant.legal_name.as_deref().unwrap_or(&tenant.name);
        let url = format!("{}/receipt/{}", self.public_url, hex::encode(&p.id));
        self.send_mail(templates::receipt(to, seller, &url));
    }

    pub async fn notify_owners(
        &self,
        tenant_id: u64,
        kind: &str,
        subject: &str,
        detail: &str,
    ) -> Result<()> {
        if !self.mailer.enabled() {
            return Ok(());
        }
        let recipients = db::notification_recipients(&self.pool, tenant_id).await?;
        if recipients.is_empty() {
            return Ok(());
        }
        let now = Utc::now();
        let cutoff = now - Duration::seconds(NOTIFY_INTERVAL_SECS);
        if !db::claim_notification(&self.pool, tenant_id, kind, now, cutoff).await? {
            return Ok(());
        }
        let tenant = self.tenant(tenant_id).await?;
        for to in recipients {
            self.send_mail(templates::event(&to, &tenant.name, subject, detail));
        }
        Ok(())
    }

    pub async fn notify_webhook_abandoned(&self, tenant_id: u64, error: &str) {
        let detail = format!(
            "A webhook delivery was abandoned after repeated failures. Last error: {error}. You can replay it from the webhooks tab."
        );
        self.notify_quietly(
            tenant_id,
            EVENT_WEBHOOK_ABANDONED,
            "Webhook delivery abandoned",
            &detail,
        )
        .await;
    }

    pub async fn notify_refund_failed(&self, tenant_id: u64, error: &str) {
        let detail = format!("A refund failed. Error: {error}.");
        self.notify_quietly(tenant_id, EVENT_REFUND_FAILED, "Refund failed", &detail)
            .await;
    }

    pub async fn provider_failed(&self, provider_id: u64, error: &str) {
        let count = {
            let mut failures = self.provider_failures.lock().unwrap();
            let n = failures.entry(provider_id).or_insert(0);
            *n += 1;
            *n
        };
        if count < PROVIDER_FAILURES_BEFORE_NOTICE {
            return;
        }
        let Ok(Some(cfg)) = db::get_provider_config(&self.pool, provider_id).await else {
            return;
        };
        let detail = format!(
            "Provider {} ({}) has failed {count} times in a row. Last error: {error}.",
            cfg.name, cfg.method
        );
        self.notify_quietly(
            cfg.tenant_id,
            EVENT_PROVIDER_FAILING,
            "Provider failing",
            &detail,
        )
        .await;
    }

    pub fn provider_recovered(&self, provider_id: u64) {
        self.provider_failures.lock().unwrap().remove(&provider_id);
    }

    pub(super) async fn notify_quietly(
        &self,
        tenant_id: u64,
        kind: &str,
        subject: &str,
        detail: &str,
    ) {
        let detail: String = detail.chars().take(500).collect();
        if let Err(e) = self.notify_owners(tenant_id, kind, subject, &detail).await {
            log::warn!("notification {kind} for project {tenant_id} failed: {e:?}");
        }
    }
}
