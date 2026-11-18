use super::*;
use crate::db::{DestinationRow, NewDestination, NewWithdrawal, WithdrawalRow};
use crate::ledger::{self, AccountKind, AccountRef, Entry, LedgerError, Posting, TxKind};
use crate::payouts::{self, CURRENCY, DestinationKind, PayoutPolicy};
use sqlx::{MySql, Transaction};

pub const MAX_DESTINATIONS: usize = 20;
pub const EVENT_WITHDRAWAL_FAILED: &str = "withdrawal_failed";

#[derive(Debug, Clone, Default)]
pub struct WithdrawalRequest {
    pub amount: u64,
    pub destination_id: Option<u64>,
    pub invoice: Option<String>,
    pub requested_by: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PayoutLimits {
    pub min_amount: u64,
    pub daily_max: u64,
    pub auto_threshold: u64,
    pub used_24h: u64,
    pub destination_delay_hours: i64,
    pub currency: &'static str,
}

fn owed(tenant_id: u64) -> AccountRef {
    AccountRef::tenant(tenant_id, AccountKind::TenantBalance, CURRENCY)
}

fn pending(tenant_id: u64) -> AccountRef {
    AccountRef::tenant(tenant_id, AccountKind::PendingPayout, CURRENCY)
}

pub fn verify_challenge(d: &DestinationRow) -> String {
    format!(
        "payment-gateway payout destination {} for project {} created {}",
        d.id,
        d.tenant_id,
        d.created.timestamp()
    )
}

impl Service {
    pub fn payout_policy(&self) -> PayoutPolicy {
        self.payouts.policy()
    }

    pub async fn payout_limits(&self, tenant: &Tenant) -> Result<PayoutLimits> {
        let policy = self.payout_policy();
        let overrides = db::get_payout_limits(&self.pool, tenant.id).await?;
        let used = db::sum_recent_withdrawals(
            &self.pool,
            tenant.id,
            CURRENCY,
            Utc::now() - Duration::hours(24),
        )
        .await?;
        Ok(PayoutLimits {
            min_amount: policy.min_amount,
            daily_max: overrides
                .daily_max
                .unwrap_or_else(|| policy.daily_max(tenant.trusted)),
            auto_threshold: overrides.auto_threshold.unwrap_or(policy.auto_threshold),
            used_24h: used,
            destination_delay_hours: policy.destination_delay.num_hours(),
            currency: CURRENCY,
        })
    }

    pub async fn add_destination(
        &self,
        tenant: &Tenant,
        kind: DestinationKind,
        label: &str,
        raw: &str,
        created_by: Option<u64>,
    ) -> Result<DestinationRow> {
        let policy = self.payout_policy();
        if !kind.storable() {
            return invalid("a bolt11 invoice is given per withdrawal, not saved as a destination");
        }
        let label = label.trim();
        if label.is_empty() || label.chars().count() > 100 {
            return invalid("label must be 1 to 100 characters");
        }
        let dest = payouts::normalize(kind, raw, policy.network)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        if db::list_destinations(&self.pool, tenant.id).await?.len() >= MAX_DESTINATIONS {
            return Err(Error::LimitExceeded(crate::limits::LimitHit::new(
                "payout_destinations",
                MAX_DESTINATIONS as u64,
                MAX_DESTINATIONS as u64,
            )));
        }
        let sealed = self.sealer.seal(dest.value.as_bytes())?;
        let id = db::insert_destination(
            &self.pool,
            &NewDestination {
                tenant_id: tenant.id,
                kind: kind.as_str(),
                label,
                sealed_value: &sealed,
                value_hash: &dest.hash(),
                verified: false,
                created_by,
                usable_from: Utc::now() + policy.destination_delay,
            },
        )
        .await?;
        db::get_destination(&self.pool, tenant.id, id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub async fn remove_destination(&self, tenant: &Tenant, id: u64) -> Result<()> {
        if db::disable_destination(&self.pool, tenant.id, id).await? {
            Ok(())
        } else {
            Err(Error::NotFound)
        }
    }

    pub async fn verify_destination_signature(
        &self,
        tenant: &Tenant,
        id: u64,
        signature: &str,
    ) -> Result<DestinationRow> {
        let d = db::get_destination(&self.pool, tenant.id, id)
            .await?
            .ok_or(Error::NotFound)?;
        if d.verified {
            return Err(Error::Conflict("destination is already verified".into()));
        }
        if d.kind != DestinationKind::Onchain.as_str() {
            return invalid("signed message verification works for on-chain addresses only");
        }
        let address = String::from_utf8(self.sealer.open(&d.value)?)
            .map_err(|e| Error::Internal(e.into()))?;
        if !payouts::signed_by_address(&address, &verify_challenge(&d), signature) {
            return invalid("signature does not match the address and challenge");
        }
        db::mark_destination_verified(&self.pool, id, Utc::now()).await?;
        db::get_destination(&self.pool, tenant.id, id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub async fn verify_destination_payout(
        &self,
        tenant: &Tenant,
        id: u64,
        requested_by: Option<u64>,
    ) -> Result<WithdrawalRow> {
        let policy = self.payout_policy();
        let mut tx = self.pool.begin().await?;
        lock_tenant(&mut tx, tenant.id).await?;
        let d = db::get_destination(&mut *tx, tenant.id, id)
            .await?
            .ok_or(Error::NotFound)?;
        if d.verified {
            return Err(Error::Conflict("destination is already verified".into()));
        }
        let open: i64 = sqlx::query_scalar(
            "select count(*) from withdrawal where destination_id = ? and purpose = 'verify' and status in ('requested', 'approved', 'sending')",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        if open > 0 {
            return Err(Error::Conflict(
                "a test payout is already in progress".into(),
            ));
        }
        let kind = DestinationKind::parse(&d.kind)
            .ok_or_else(|| Error::Invalid("unknown destination kind".into()))?;
        let amount = if kind == DestinationKind::Onchain {
            policy.verify_amount_onchain
        } else {
            policy.verify_amount_lightning
        };
        let w = self
            .insert_reserved(
                &mut tx,
                tenant,
                &policy,
                ReservedWithdrawal {
                    amount,
                    kind,
                    destination_id: Some(d.id),
                    sealed_destination: d.value.clone(),
                    label: d.label.clone(),
                    sealed_invoice: None,
                    payment_hash: None,
                    purpose: "verify",
                    status: "approved",
                    requested_by,
                    auto: true,
                },
            )
            .await?;
        tx.commit().await?;
        Ok(w)
    }

    pub async fn request_withdrawal(
        &self,
        tenant: &Tenant,
        req: WithdrawalRequest,
    ) -> Result<WithdrawalRow> {
        let policy = self.payout_policy();
        if !tenant.enabled {
            return Err(Error::Conflict("project is disabled".into()));
        }
        if req.amount == 0 || !req.amount.is_multiple_of(1000) {
            return invalid("amount must be a whole number of sats, given in msats");
        }
        if req.amount < policy.min_amount {
            return invalid(format!(
                "amount is below the minimum of {} msats",
                policy.min_amount
            ));
        }
        let mut tx = self.pool.begin().await?;
        lock_tenant(&mut tx, tenant.id).await?;
        let overrides = db::lock_payout_limits(&mut *tx, tenant.id).await?;
        let daily_max = overrides
            .daily_max
            .unwrap_or_else(|| policy.daily_max(tenant.trusted));
        let threshold = overrides.auto_threshold.unwrap_or(policy.auto_threshold);
        let used = db::sum_recent_withdrawals(
            &mut *tx,
            tenant.id,
            CURRENCY,
            Utc::now() - Duration::hours(24),
        )
        .await?;
        if used + req.amount > daily_max {
            return Err(Error::LimitExceeded(crate::limits::LimitHit::new(
                "payout_daily_max",
                used + req.amount,
                daily_max,
            )));
        }
        let resolved = match (req.destination_id, req.invoice.as_deref()) {
            (Some(_), Some(_)) | (None, None) => {
                return invalid("give either destination_id or invoice");
            }
            (Some(did), None) => {
                let d = db::get_destination(&mut *tx, tenant.id, did)
                    .await?
                    .ok_or(Error::NotFound)?;
                if !d.verified {
                    return Err(Error::Conflict("destination is not verified yet".into()));
                }
                if d.usable_from > Utc::now() {
                    return Err(Error::Conflict(format!(
                        "destination can be used from {}",
                        d.usable_from.to_rfc3339()
                    )));
                }
                let kind = DestinationKind::parse(&d.kind)
                    .ok_or_else(|| Error::Invalid("unknown destination kind".into()))?;
                (kind, Some(d.id), d.value, d.label, None, None)
            }
            (None, Some(inv)) => {
                let invoice = inv.trim().trim_start_matches("lightning:").to_string();
                let parsed = crate::providers::lightning::parse_invoice(&invoice)
                    .map_err(|e| Error::Invalid(format!("invalid invoice: {e}")))?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                if parsed.parsed_invoice.expires_at().is_none_or(|e| e <= now) {
                    return invalid("invoice has expired");
                }
                payouts::check_invoice_amount(&invoice, req.amount)
                    .map_err(|e| Error::Invalid(e.to_string()))?;
                let sealed = self.sealer.seal(invoice.as_bytes())?;
                (
                    DestinationKind::Bolt11,
                    None,
                    sealed.clone(),
                    payouts::mask(DestinationKind::Bolt11, &invoice),
                    Some(sealed),
                    Some(parsed.payment_hash()),
                )
            }
        };
        let (kind, destination_id, sealed_destination, label, sealed_invoice, payment_hash) =
            resolved;
        let auto = tenant.trusted && req.amount <= threshold;
        let w = self
            .insert_reserved(
                &mut tx,
                tenant,
                &policy,
                ReservedWithdrawal {
                    amount: req.amount,
                    kind,
                    destination_id,
                    sealed_destination,
                    label,
                    sealed_invoice,
                    payment_hash,
                    purpose: "payout",
                    status: if auto { "approved" } else { "requested" },
                    requested_by: req.requested_by,
                    auto,
                },
            )
            .await?;
        self.emit_withdrawal(&mut tx, tenant, "withdrawal.requested", w.id)
            .await?;
        tx.commit().await?;
        Ok(w)
    }

    async fn insert_reserved(
        &self,
        tx: &mut Transaction<'_, MySql>,
        tenant: &Tenant,
        policy: &PayoutPolicy,
        r: ReservedWithdrawal,
    ) -> Result<WithdrawalRow> {
        let max_fee = match r.kind {
            DestinationKind::Onchain => policy.onchain_fee_reserve,
            _ => policy.lightning_max_fee(r.amount),
        };
        let id = db::insert_withdrawal(
            &mut **tx,
            &NewWithdrawal {
                tenant_id: tenant.id,
                amount: r.amount,
                currency: CURRENCY,
                purpose: r.purpose,
                max_fee,
                destination_kind: r.kind.as_str(),
                destination_id: r.destination_id,
                sealed_destination: &r.sealed_destination,
                destination_label: &r.label,
                sealed_invoice: r.sealed_invoice.as_deref(),
                payment_hash: r.payment_hash.as_deref(),
                status: r.status,
                requested_by: r.requested_by,
                approved_by: None,
                auto_approved: r.auto,
            },
        )
        .await?;
        let reserve = (r.amount + max_fee) as i64;
        let posting = Posting::new(
            TxKind::PayoutRequested,
            vec![
                Entry::new(owed(tenant.id), -reserve),
                Entry::new(pending(tenant.id), reserve),
            ],
        )
        .reference("withdrawal", id);
        match ledger::post(tx, &posting, &format!("withdrawal:{id}:requested")).await {
            Ok(_) => {}
            Err(LedgerError::Overdraw { .. }) => {
                return Err(Error::Conflict("insufficient balance".into()));
            }
            Err(e) => return Err(e.into()),
        }
        db::lock_withdrawal(&mut **tx, id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub async fn withdrawal_for_tenant(&self, tenant: &Tenant, id: u64) -> Result<WithdrawalRow> {
        db::get_withdrawal(&self.pool, id)
            .await?
            .filter(|w| w.tenant_id == tenant.id)
            .ok_or(Error::NotFound)
    }

    pub async fn cancel_withdrawal(&self, tenant: &Tenant, id: u64) -> Result<WithdrawalRow> {
        self.withdrawal_for_tenant(tenant, id).await?;
        self.release(
            tenant,
            id,
            &["requested"],
            "cancelled",
            "cancelled by the project",
            false,
        )
        .await
    }

    pub async fn approve_withdrawal(
        &self,
        id: u64,
        approver: Option<u64>,
    ) -> Result<WithdrawalRow> {
        let mut tx = self.pool.begin().await?;
        let w = db::lock_withdrawal(&mut *tx, id)
            .await?
            .ok_or(Error::NotFound)?;
        if w.status != "requested" {
            return Err(Error::Conflict(format!("withdrawal is {}", w.status)));
        }
        db::approve_withdrawal_row(&mut *tx, id, approver, false, Utc::now()).await?;
        tx.commit().await?;
        db::get_withdrawal(&self.pool, id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub async fn reject_withdrawal(&self, id: u64, reason: &str) -> Result<WithdrawalRow> {
        let w = db::get_withdrawal(&self.pool, id)
            .await?
            .ok_or(Error::NotFound)?;
        let tenant = self.tenant(w.tenant_id).await?;
        let reason = format!("rejected: {}", reason.trim());
        self.release(&tenant, id, &["requested"], "rejected", &reason, true)
            .await
    }

    pub(super) async fn release(
        &self,
        tenant: &Tenant,
        id: u64,
        from: &[&str],
        status: &str,
        error: &str,
        announce: bool,
    ) -> Result<WithdrawalRow> {
        let mut tx = self.pool.begin().await?;
        let w = db::lock_withdrawal(&mut *tx, id)
            .await?
            .filter(|w| w.tenant_id == tenant.id)
            .ok_or(Error::NotFound)?;
        if !from.contains(&w.status.as_str()) {
            return Err(Error::Conflict(format!("withdrawal is {}", w.status)));
        }
        let reserve = (w.amount + w.max_fee) as i64;
        let posting = Posting::new(
            TxKind::PayoutCancelled,
            vec![
                Entry::new(pending(w.tenant_id), -reserve),
                Entry::new(owed(w.tenant_id), reserve),
            ],
        )
        .reference("withdrawal", id);
        ledger::post(&mut tx, &posting, &format!("withdrawal:{id}:released")).await?;
        db::finish_withdrawal(&mut *tx, id, status, None, None, Some(error), Utc::now()).await?;
        if announce && w.purpose == "payout" {
            self.emit_withdrawal(&mut tx, tenant, "withdrawal.failed", id)
                .await?;
        }
        tx.commit().await?;
        db::get_withdrawal(&self.pool, id)
            .await?
            .ok_or(Error::NotFound)
    }

    pub(super) async fn emit_withdrawal(
        &self,
        tx: &mut Transaction<'_, MySql>,
        tenant: &Tenant,
        event_type: &str,
        id: u64,
    ) -> Result<()> {
        let w = db::lock_withdrawal(&mut **tx, id)
            .await?
            .ok_or(Error::NotFound)?;
        let now = Utc::now();
        let event_id = random_hex(16);
        let body = json!({
            "id": event_id,
            "type": event_type,
            "created": now.timestamp(),
            "data": { "withdrawal": payouts::WithdrawalView::from(&w) },
        });
        let body = body.to_string();
        let event = super::settlement::OutgoingEvent {
            payment_id: None,
            event_id: &event_id,
            event_type,
            body: &body,
            now,
        };
        self.fan_out(tx, tenant, &event).await
    }
}

struct ReservedWithdrawal {
    amount: u64,
    kind: DestinationKind,
    destination_id: Option<u64>,
    sealed_destination: Vec<u8>,
    label: String,
    sealed_invoice: Option<Vec<u8>>,
    payment_hash: Option<String>,
    purpose: &'static str,
    status: &'static str,
    requested_by: Option<u64>,
    auto: bool,
}

async fn lock_tenant(tx: &mut Transaction<'_, MySql>, id: u64) -> Result<()> {
    let found: Option<u64> = sqlx::query_scalar("select id from tenant where id = ? for update")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    found.map(|_| ()).ok_or(Error::NotFound)
}
