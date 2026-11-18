use super::*;
use crate::audit::{Action, Actor, Event};
use crate::mailer::templates;
use crate::providers::HealthRecorder;

pub const AUTO_DISABLE_AFTER: u32 = 10;
pub const OPERATOR_ALERT_KIND: &str = "provider_operator_alert";

#[async_trait]
impl HealthRecorder for Service {
    async fn provider_call(&self, provider_id: u64, outcome: std::result::Result<(), String>) {
        match outcome {
            Ok(()) => {
                if let Err(e) = db::record_provider_success(&self.pool, provider_id).await {
                    log::warn!("recording provider health failed: {e:#}");
                }
            }
            Err(error) => self.record_hard_failure(provider_id, &error).await,
        }
    }
}

impl Service {
    async fn record_hard_failure(&self, provider_id: u64, error: &str) {
        let error = crate::logsafe::clean(error);
        let consecutive = match db::record_provider_failure(&self.pool, provider_id, &error).await {
            Ok(n) => n,
            Err(e) => {
                log::warn!("recording provider health failed: {e:#}");
                return;
            }
        };
        if consecutive < AUTO_DISABLE_AFTER {
            return;
        }
        if let Err(e) = self.auto_disable(provider_id, consecutive, &error).await {
            log::warn!("auto disable of provider {provider_id} failed: {e:?}");
        }
    }

    async fn auto_disable(&self, provider_id: u64, failures: u32, error: &str) -> Result<()> {
        let Some(cfg) = db::get_provider_config(&self.pool, provider_id).await? else {
            return Ok(());
        };
        if !cfg.enabled {
            return Ok(());
        }
        let tenant = self.tenant(cfg.tenant_id).await?;
        if tenant.trusted {
            self.alert_operators(&cfg, failures, error).await?;
            return Ok(());
        }
        if !db::auto_disable_provider(&self.pool, provider_id).await? {
            return Ok(());
        }
        self.registry.invalidate(provider_id);
        Event::new(Action::ProviderAutoDisabled, Actor::System)
            .tenant(cfg.tenant_id)
            .target("provider", provider_id)
            .details(json!({"consecutive_failures": failures}))
            .record(&self.pool, &Default::default())
            .await;
        let detail = format!(
            "Provider {} ({}) was disabled after {failures} failures in a row. Last error: {error}. Fix the credentials and enable it again from the providers tab.",
            cfg.name, cfg.method
        );
        let detail: String = detail.chars().take(500).collect();
        if let Err(e) = self
            .notify_owners(
                cfg.tenant_id,
                "provider_auto_disabled",
                "Provider disabled",
                &detail,
            )
            .await
        {
            log::warn!("notification for provider {provider_id} failed: {e:?}");
        }
        Ok(())
    }

    async fn alert_operators(
        &self,
        cfg: &ProviderConfigRow,
        failures: u32,
        error: &str,
    ) -> Result<()> {
        if !self.mailer.enabled() {
            return Ok(());
        }
        let now = Utc::now();
        let cutoff = now - Duration::seconds(super::notify::NOTIFY_INTERVAL_SECS);
        if !db::claim_notification(&self.pool, cfg.tenant_id, OPERATOR_ALERT_KIND, now, cutoff)
            .await?
        {
            return Ok(());
        }
        let detail: String = format!(
            "Shared provider {} ({}, id {}) has failed {failures} times in a row and was left enabled. Last error: {error}.",
            cfg.name, cfg.method, cfg.id
        )
        .chars()
        .take(500)
        .collect();
        for to in db::operator_emails(&self.pool).await? {
            self.send_mail(templates::event(
                &to,
                "operator",
                "Shared provider failing",
                &detail,
            ));
        }
        Ok(())
    }
}
