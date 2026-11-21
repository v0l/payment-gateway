use super::*;
use crate::db::WithdrawalRow;
use crate::payouts::DestinationKind;
use sha2::Digest;

#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct CreateWithdrawal {
    pub amount: u64,
    #[serde(default)]
    pub currency: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
    #[serde(default)]
    pub destination_id: Option<u64>,
    #[serde(default)]
    pub provider_id: Option<u64>,
}

pub fn detect_kind(destination: &str) -> Option<DestinationKind> {
    let d = destination.trim().trim_start_matches("lightning:");
    let lower = d.to_ascii_lowercase();
    if lower.contains('@') {
        Some(DestinationKind::LightningAddress)
    } else if lower.starts_with("lnurl") {
        Some(DestinationKind::Lnurl)
    } else if lower.starts_with("ln") {
        Some(DestinationKind::Bolt11)
    } else if !lower.is_empty() {
        Some(DestinationKind::Onchain)
    } else {
        None
    }
}

fn request_hash(input: &CreateWithdrawal) -> String {
    let canonical = json!({
        "amount": input.amount,
        "currency": input.currency.as_deref().map(str::to_ascii_uppercase),
        "destination": input.destination.as_deref().map(str::trim),
        "destination_id": input.destination_id,
        "provider_id": input.provider_id,
    });
    hex::encode(sha2::Sha256::digest(canonical.to_string().as_bytes()))
}

impl Service {
    pub async fn create_withdrawal(
        &self,
        tenant: &Tenant,
        input: CreateWithdrawal,
        idempotency_key: &str,
    ) -> Result<(WithdrawalRow, bool)> {
        if idempotency_key.is_empty() || idempotency_key.len() > 128 {
            return invalid("Idempotency-Key must be 1-128 characters");
        }
        let hash = request_hash(&input);
        if let Some(existing) =
            db::withdrawal_by_idempotency(&self.pool, tenant.id, idempotency_key).await?
        {
            return same_request(existing, &hash);
        }
        if let Some(c) = &input.currency
            && !c.eq_ignore_ascii_case("BTC")
        {
            return invalid("only BTC withdrawals are supported");
        }
        if input.provider_id.is_some() && input.destination_id.is_some() {
            return invalid("destination_id applies to custody withdrawals only");
        }
        let mut req = WithdrawalRequest {
            amount: input.amount,
            idempotency_key: Some(idempotency_key.to_string()),
            request_hash: Some(hash.clone()),
            ..Default::default()
        };
        if let Some(pid) = input.provider_id {
            self.check_wallet_provider(tenant, pid).await?;
            let dest = input
                .destination
                .as_deref()
                .ok_or_else(|| Error::Invalid("destination is required".into()))?;
            let kind = detect_kind(dest)
                .ok_or_else(|| Error::Invalid("destination is required".into()))?;
            req.wallet_provider_id = Some(pid);
            if kind == DestinationKind::Bolt11 {
                req.invoice = Some(dest.to_string());
            } else {
                req.raw_destination = Some((kind, dest.to_string()));
            }
        } else {
            req.destination_id = input.destination_id;
            if let Some(dest) = input.destination.as_deref() {
                match detect_kind(dest) {
                    Some(DestinationKind::Bolt11) => req.invoice = Some(dest.to_string()),
                    _ => {
                        return invalid(
                            "custody withdrawals take a bolt11 invoice or a verified destination_id",
                        );
                    }
                }
            }
        }
        match self.request_withdrawal(tenant, req).await {
            Ok(w) => Ok((w, true)),
            Err(Error::Conflict(_)) => {
                match db::withdrawal_by_idempotency(&self.pool, tenant.id, idempotency_key).await? {
                    Some(existing) => same_request(existing, &hash),
                    None => Err(Error::Conflict("already exists".into())),
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn check_wallet_provider(&self, tenant: &Tenant, id: u64) -> Result<()> {
        let row = db::get_provider_config(&self.pool, id)
            .await?
            .filter(|r| r.tenant_id == tenant.id)
            .ok_or(Error::NotFound)?;
        if row.shared_provider_id.is_some() || !row.enabled || row.provider_type != "lnd" {
            return invalid("provider_id must be one of the project's own enabled LND providers");
        }
        if self.payouts.tenant_nodes().is_none() {
            return Err(Error::Conflict("wallet payouts are not enabled".into()));
        }
        Ok(())
    }

    pub async fn list_withdrawals(
        &self,
        tenant: &Tenant,
        before: Option<u64>,
        status: Option<&str>,
        limit: u32,
    ) -> Result<Vec<WithdrawalRow>> {
        Ok(db::withdrawals_page(&self.pool, tenant.id, before, status, limit).await?)
    }
}

fn same_request(existing: WithdrawalRow, hash: &str) -> Result<(WithdrawalRow, bool)> {
    if existing.request_hash.as_deref() == Some(hash) {
        Ok((existing, false))
    } else {
        Err(Error::Conflict(
            "Idempotency-Key was used with different parameters".into(),
        ))
    }
}
