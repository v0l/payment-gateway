use super::{DestinationKind, mask};
use crate::crypto::Sealer;
use crate::db::{DestinationRow, WithdrawalRow};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WithdrawalView {
    pub id: u64,
    pub tenant_id: u64,
    pub amount: u64,
    pub currency: String,
    pub purpose: String,
    pub status: String,
    pub destination_kind: String,
    pub destination_label: String,
    pub destination_id: Option<u64>,
    pub fee: Option<u64>,
    pub max_fee: u64,
    pub external_id: Option<String>,
    pub error: Option<String>,
    pub requested_by: Option<u64>,
    pub approved_by: Option<u64>,
    pub auto_approved: bool,
    pub created: DateTime<Utc>,
    pub approved_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl From<&WithdrawalRow> for WithdrawalView {
    fn from(w: &WithdrawalRow) -> Self {
        Self {
            id: w.id,
            tenant_id: w.tenant_id,
            amount: w.amount,
            currency: w.currency.clone(),
            purpose: w.purpose.clone(),
            status: w.status.clone(),
            destination_kind: w.destination_kind.clone(),
            destination_label: w.destination_label.clone(),
            destination_id: w.destination_id,
            fee: w.fee,
            max_fee: w.max_fee,
            external_id: w.external_id.clone(),
            error: w.error.clone(),
            requested_by: w.requested_by,
            approved_by: w.approved_by,
            auto_approved: w.auto_approved,
            created: w.created,
            approved_at: w.approved_at,
            finished_at: w.finished_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DestinationView {
    pub id: u64,
    pub kind: String,
    pub label: String,
    pub masked: String,
    pub verified: bool,
    pub usable_from: DateTime<Utc>,
    pub usable: bool,
    pub created: DateTime<Utc>,
    pub verified_at: Option<DateTime<Utc>>,
}

pub fn destination_view(
    d: &DestinationRow,
    sealer: &Sealer,
    now: DateTime<Utc>,
) -> DestinationView {
    let kind = DestinationKind::parse(&d.kind);
    let masked = match (kind, sealer.open(&d.value)) {
        (Some(k), Ok(v)) => mask(k, &String::from_utf8_lossy(&v)),
        _ => "***".into(),
    };
    DestinationView {
        id: d.id,
        kind: d.kind.clone(),
        label: d.label.clone(),
        masked,
        verified: d.verified,
        usable_from: d.usable_from,
        usable: d.verified && d.usable_from <= now,
        created: d.created,
        verified_at: d.verified_at,
    }
}
