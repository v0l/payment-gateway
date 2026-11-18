use super::*;
use crate::db::{SharedAttempt, SharedProviderRow};
use crate::ledger::{self, AccountKind, AccountRef, Entry, Posting, TxKind};
use crate::shared::{self, FeeTerms, Split};

pub const LEDGER_CURRENCY: &str = "BTC";

impl Service {
    pub(super) async fn usable_configs(&self, tenant: &Tenant) -> Result<Vec<ProviderConfigRow>> {
        let configs = db::list_provider_configs(&self.pool, tenant.id, true).await?;
        let mut out = Vec::with_capacity(configs.len());
        for cfg in configs {
            match cfg.shared_provider_id {
                None => out.push(cfg),
                Some(sid) => {
                    let eligible = db::get_shared_provider(&self.pool, sid)
                        .await?
                        .is_some_and(|sp| sp.enabled && Self::tenant_may_use(tenant, &sp));
                    if eligible {
                        out.push(cfg);
                    }
                }
            }
        }
        Ok(out)
    }

    pub fn tenant_may_use(tenant: &Tenant, sp: &SharedProviderRow) -> bool {
        shared::may_use(
            tenant.trusted,
            &sp.network,
            sp.allow_trusted,
            sp.allow_untrusted,
        )
    }

    pub(super) async fn shared_attempt_terms(
        &self,
        tenant: &Tenant,
        cfg: &ProviderConfigRow,
    ) -> Result<Option<SharedAttempt>> {
        let Some(sid) = cfg.shared_provider_id else {
            return Ok(None);
        };
        let sp = db::get_shared_provider(&self.pool, sid)
            .await?
            .ok_or(Error::NotFound)?;
        let terms = db::effective_terms(&self.pool, tenant.id, &sp).await?;
        Ok(Some(SharedAttempt {
            shared_provider_id: sid,
            terms,
        }))
    }

    pub async fn shared_choices(&self, tenant: &Tenant) -> Result<Vec<SharedChoice>> {
        let enabled: std::collections::HashSet<u64> =
            db::list_provider_configs(&self.pool, tenant.id, true)
                .await?
                .iter()
                .filter_map(|c| c.shared_provider_id)
                .collect();
        let mut out = vec![];
        for sp in db::list_shared_providers(&self.pool, true).await? {
            if !Self::tenant_may_use(tenant, &sp) {
                continue;
            }
            let terms = db::effective_terms(&self.pool, tenant.id, &sp).await?;
            out.push(SharedChoice {
                id: sp.id,
                name: sp.name.clone(),
                method: sp.method.clone(),
                network: sp.network.clone(),
                fee_bps: terms.bps,
                fee_base_msat: terms.base_msat,
                enabled: enabled.contains(&sp.id),
            });
        }
        Ok(out)
    }

    pub async fn enable_shared(&self, tenant: &Tenant, shared_id: u64) -> Result<u64> {
        let sp = db::get_shared_provider(&self.pool, shared_id)
            .await?
            .filter(|sp| sp.enabled)
            .ok_or(Error::NotFound)?;
        if !Self::tenant_may_use(tenant, &sp) {
            return Err(Error::Invalid(if tenant.trusted {
                "this shared provider is not available to this project".into()
            } else {
                "test mode projects can only use shared regtest or signet providers".into()
            }));
        }
        let sealed = self.sealer.seal(b"{}").map_err(Error::Internal)?;
        let id = db::enable_shared_for_tenant(&self.pool, tenant.id, &sp, &sealed).await?;
        Ok(id)
    }

    pub async fn disable_shared(&self, tenant: &Tenant, shared_id: u64) -> Result<()> {
        if !db::disable_shared_for_tenant(&self.pool, tenant.id, shared_id).await? {
            return Err(Error::NotFound);
        }
        Ok(())
    }

    pub(super) async fn post_settlement(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
        tenant_id: u64,
        attempt: &AttemptRow,
        received: u64,
    ) -> Result<Option<Split>> {
        if attempt.shared_provider_id.is_none() {
            return Ok(None);
        }
        let terms = FeeTerms {
            bps: attempt.platform_fee_bps.unwrap_or(0),
            base_msat: attempt.platform_fee_base.unwrap_or(0),
        };
        let received_msat = if attempt.charge_currency == "BTC" {
            received
        } else {
            return Ok(None);
        };
        let split = shared::split(received_msat, terms);
        let mut entries = vec![
            Entry::new(
                AccountRef::platform(AccountKind::PlatformWallet, LEDGER_CURRENCY),
                -(received_msat as i64),
            ),
            Entry::new(
                AccountRef::tenant(tenant_id, AccountKind::TenantBalance, LEDGER_CURRENCY),
                split.net as i64,
            ),
        ];
        if split.fee > 0 {
            entries.push(Entry::new(
                AccountRef::platform(AccountKind::PlatformFees, LEDGER_CURRENCY),
                split.fee as i64,
            ));
        }
        if split.net == 0 {
            entries.remove(1);
        }
        let posting =
            Posting::new(TxKind::PaymentReceived, entries).reference("attempt", attempt.id);
        ledger::post(tx, &posting, &format!("settle:{}", attempt.id)).await?;
        db::set_attempt_platform_fee(&mut **tx, attempt.id, split.fee, split.net).await?;
        Ok(Some(split))
    }

    pub(super) async fn post_refund_debit(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
        tenant_id: u64,
        attempt: &AttemptRow,
        refund_id: u64,
        charge_msat: u64,
    ) -> Result<()> {
        if attempt.shared_provider_id.is_none() || attempt.charge_currency != "BTC" {
            return Ok(());
        }
        let posting = Posting::new(
            TxKind::Refund,
            vec![
                Entry::new(
                    AccountRef::tenant(tenant_id, AccountKind::TenantBalance, LEDGER_CURRENCY),
                    -(charge_msat as i64),
                ),
                Entry::new(
                    AccountRef::platform(AccountKind::PlatformWallet, LEDGER_CURRENCY),
                    charge_msat as i64,
                ),
            ],
        )
        .reference("refund", refund_id);
        ledger::post(tx, &posting, &format!("refund:{refund_id}")).await?;
        Ok(())
    }

    pub(super) async fn post_refund_reversal(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::MySql>,
        tenant_id: u64,
        attempt: &AttemptRow,
        refund_id: u64,
        charge_msat: u64,
    ) -> Result<()> {
        if attempt.shared_provider_id.is_none() || attempt.charge_currency != "BTC" {
            return Ok(());
        }
        let posting = Posting::new(
            TxKind::Adjustment,
            vec![
                Entry::new(
                    AccountRef::tenant(tenant_id, AccountKind::TenantBalance, LEDGER_CURRENCY),
                    charge_msat as i64,
                ),
                Entry::new(
                    AccountRef::platform(AccountKind::PlatformWallet, LEDGER_CURRENCY),
                    -(charge_msat as i64),
                ),
            ],
        )
        .reference("refund", refund_id);
        ledger::post(tx, &posting, &format!("refund_reversal:{refund_id}")).await?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SharedChoice {
    pub id: u64,
    pub name: String,
    pub method: String,
    pub network: String,
    pub fee_bps: u32,
    pub fee_base_msat: u64,
    pub enabled: bool,
}
