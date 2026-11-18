use super::*;

pub(super) fn refund_charge(
    attempt: &AttemptRow,
    amount: u64,
    remaining: u64,
    charge_committed: u64,
) -> Result<u64> {
    let gross = (attempt.base_amount + attempt.processing_fee).max(1) as u128;
    let mut charge = (attempt.charge_amount as u128 * amount as u128 / gross) as u64;
    if amount == remaining {
        charge = charge
            .max(
                ((attempt.charge_amount as u128 * attempt.base_amount as u128 / gross) as u64)
                    .saturating_sub(charge_committed),
            )
            .min(attempt.charge_amount.saturating_sub(charge_committed));
    }
    if charge == 0 {
        return Err(Error::Invalid("refund is too small".into()));
    }
    if attempt.charge_currency == "BTC" {
        charge = (charge / 1000) * 1000;
        if charge == 0 {
            return Err(Error::Invalid("refund is below 1 sat".into()));
        }
    }
    Ok(charge)
}

#[derive(Debug, Serialize)]
pub struct RefundPreview {
    pub attempt_id: u64,
    pub method: String,
    pub currency: String,
    pub refundable: u64,
    pub amount: u64,
    pub full: bool,
    pub charge: u64,
    pub charge_currency: String,
    pub processing_fee: u64,
    pub fee_kept: u64,
    pub remaining_after: u64,
    pub destination_required: bool,
}

pub fn validate_destination(method: &str, charge: u64, destination: &str) -> Result<()> {
    match method {
        "lightning" => {
            let msat = crate::providers::lightning::invoice_amount_msat(destination)
                .map_err(|_| Error::Invalid("destination is not a valid bolt11 invoice".into()))?;
            if let Some(msat) = msat
                && msat > charge
            {
                return Err(Error::Invalid(format!(
                    "invoice asks for {msat} msat but the refund is {charge} msat"
                )));
            }
            Ok(())
        }
        "onchain" => {
            use std::str::FromStr;
            bitcoin::Address::<bitcoin::address::NetworkUnchecked>::from_str(destination)
                .map(|_| ())
                .map_err(|_| Error::Invalid("destination is not a valid bitcoin address".into()))
        }
        _ => Ok(()),
    }
}

impl Service {
    pub async fn refund_preview(
        &self,
        tenant: &Tenant,
        id: &PaymentId,
        amount: Option<u64>,
        attempt_id: Option<u64>,
        destination: Option<&str>,
    ) -> Result<RefundPreview> {
        let p = self.payment_for_tenant(tenant, id).await?;
        if !matches!(p.status.as_str(), "paid" | "partially_refunded") {
            return Err(Error::Conflict(format!("payment is {}", p.status)));
        }
        let attempts = db::attempts_for_payment(&self.pool, id).await?;
        let attempt = match attempt_id {
            Some(aid) => attempts.iter().find(|a| a.id == aid && a.status == "paid"),
            None => attempts
                .iter()
                .filter(|a| a.status == "paid")
                .min_by_key(|a| a.paid_at),
        }
        .ok_or_else(|| Error::Invalid("no paid attempt to refund".into()))?;
        let refunds = db::refunds_for_payment(&self.pool, id).await?;
        let live = refunds
            .iter()
            .filter(|r| r.attempt_id == attempt.id && r.status != "failed");
        let committed: u64 = live.clone().map(|r| r.amount).sum();
        let charge_committed: u64 = live.map(|r| r.charge_amount).sum();
        let refundable = p.amount.saturating_sub(committed);
        let amount = amount.unwrap_or(refundable);
        if amount == 0 || amount > refundable {
            return Err(Error::Invalid(format!("refundable amount is {refundable}")));
        }
        let charge = refund_charge(attempt, amount, refundable, charge_committed)?;
        let full = amount == refundable;
        let fee_kept = if full {
            attempt
                .charge_amount
                .saturating_sub(charge_committed)
                .saturating_sub(charge)
        } else {
            0
        };
        if let Some(d) = destination.filter(|d| !d.trim().is_empty()) {
            validate_destination(&attempt.method, charge, d.trim())?;
        }
        Ok(RefundPreview {
            attempt_id: attempt.id,
            method: attempt.method.clone(),
            currency: p.currency.clone(),
            refundable,
            amount,
            full,
            charge,
            charge_currency: attempt.charge_currency.clone(),
            processing_fee: attempt.processing_fee,
            fee_kept,
            remaining_after: refundable - amount,
            destination_required: matches!(attempt.method.as_str(), "lightning" | "onchain"),
        })
    }
}
