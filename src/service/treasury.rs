use super::*;
use crate::ledger;
use crate::mailer::templates;
use crate::treasury::{self, Details, RunSummary, WalletLine};

impl Service {
    async fn collect_wallets(&self) -> Result<Vec<WalletLine>> {
        let mut lines: Vec<WalletLine> = vec![];
        for sp in db::list_shared_providers(&self.pool, false).await? {
            let key = crate::shared::shared_key(sp.id);
            let outcome = match self.registry.get(key).await {
                Ok(p) => p.wallet_balances().await,
                Err(e) => Err(e),
            };
            match outcome {
                Ok(found) => {
                    for w in found {
                        if lines.iter().any(|l| l.identity == w.identity) {
                            continue;
                        }
                        lines.push(WalletLine {
                            identity: w.identity,
                            kind: w.kind,
                            msat: w.msat,
                            error: None,
                        });
                    }
                }
                Err(e) => lines.push(WalletLine {
                    identity: format!("shared:{}", sp.id),
                    kind: sp.method.clone(),
                    msat: 0,
                    error: Some(crate::logsafe::clean(&format!("{e:#}"))),
                }),
            }
        }
        Ok(lines)
    }

    pub async fn run_reconciliation(&self, trigger: &str) -> Result<RunSummary> {
        let now = self.clock.now();
        let totals = treasury::totals(&self.pool).await?;
        let wallets = self.collect_wallets().await?;
        let wallet_errors = wallets.iter().filter(|w| w.error.is_some()).count();
        let assets: i64 = wallets
            .iter()
            .filter(|w| w.error.is_none())
            .map(|w| w.msat as i64)
            .sum();
        let mismatches = ledger::reconcile(&self.pool).await?;
        let drift = mismatches.iter().filter(|m| m.account_id.is_some()).count();
        let unbalanced = mismatches.len() - drift;
        let stuck = treasury::find_stuck(&self.pool, now, UNCONFIRMED_GRACE_SECS).await?;
        let alerts = treasury::assess(
            totals.liabilities,
            assets,
            wallet_errors,
            drift,
            unbalanced,
            self.treasury.tolerance_msat(),
        );
        let checked = wallets.len() - wallet_errors;
        let stuck_count = stuck.len();
        let id = treasury::insert_run(
            &self.pool,
            now,
            trigger,
            &totals,
            assets,
            checked,
            wallet_errors,
            drift,
            unbalanced,
            stuck_count,
            &alerts,
            &Details {
                wallets,
                mismatches,
                stuck,
            },
        )
        .await?;
        let run = treasury::latest_run(&self.pool)
            .await?
            .filter(|r| r.id == id)
            .ok_or(Error::NotFound)?;
        if !alerts.is_empty() {
            log::error!("treasury alert: {}", alerts.join("; "));
            self.alert_treasury(&alerts).await;
        }
        Ok(run)
    }

    pub async fn reconcile_if_due(&self) -> Result<Option<RunSummary>> {
        let now = self.clock.now();
        if let Some(last) = treasury::latest_run(&self.pool).await?
            && now - last.created < Duration::seconds(treasury::RUN_INTERVAL_SECS)
        {
            return Ok(None);
        }
        self.run_reconciliation("scheduled").await.map(Some)
    }

    async fn alert_treasury(&self, alerts: &[String]) {
        if !self.mailer.enabled() {
            return;
        }
        let now = self.clock.now();
        let cutoff = now - Duration::seconds(super::notify::NOTIFY_INTERVAL_SECS);
        match db::claim_notification(&self.pool, 0, treasury::ALERT_KIND, now, cutoff).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(e) => {
                log::warn!("treasury alert throttle failed: {e:#}");
                return;
            }
        }
        let detail: String = alerts.join(". ");
        match db::operator_emails(&self.pool).await {
            Ok(list) => {
                for to in list {
                    self.send_mail(templates::event(&to, "operator", "Treasury alert", &detail));
                }
            }
            Err(e) => log::warn!("treasury alert recipients failed: {e:#}"),
        }
    }
}
