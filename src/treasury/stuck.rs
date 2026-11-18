use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::MySqlPool;

pub const WITHDRAWAL_SENDING_LIMIT_SECS: i64 = 3600;
pub const WEBHOOK_GRACE_SECS: i64 = 3600;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StuckKind {
    SettledWithoutCredit,
    WithdrawalSending,
    UnconfirmedReceipt,
    PaidWithoutWebhook,
}

impl StuckKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SettledWithoutCredit => "settled_without_credit",
            Self::WithdrawalSending => "withdrawal_sending",
            Self::UnconfirmedReceipt => "unconfirmed_receipt",
            Self::PaidWithoutWebhook => "paid_without_webhook",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct StuckItem {
    pub kind: StuckKind,
    pub tenant_id: u64,
    pub reference: String,
    pub since: DateTime<Utc>,
    pub link: String,
}

type Row = (u64, String, DateTime<Utc>);

fn payment_link(tenant_id: u64, payment_hex: &str) -> String {
    format!("#/p/{tenant_id}/payments/{payment_hex}")
}

pub async fn find_stuck(
    pool: &MySqlPool,
    now: DateTime<Utc>,
    receipt_grace_secs: i64,
) -> Result<Vec<StuckItem>> {
    let mut out = vec![];

    let rows: Vec<(u64, String, DateTime<Utc>)> = sqlx::query_as(
        "select p.tenant_id, hex(a.payment_id), coalesce(a.paid_at, a.created) from payment_attempt a join payment p on p.id = a.payment_id where a.status = 'paid' and a.shared_provider_id is not null and a.charge_currency = 'BTC' and not exists (select 1 from ledger_transaction t where t.idempotency_key = concat('settle:', a.id)) order by a.id limit 500",
    )
    .fetch_all(pool)
    .await?;
    out.extend(rows.into_iter().map(|(tenant_id, payment, since)| {
        let payment = payment.to_lowercase();
        StuckItem {
            kind: StuckKind::SettledWithoutCredit,
            tenant_id,
            link: payment_link(tenant_id, &payment),
            reference: payment,
            since,
        }
    }));

    let rows: Vec<Row> = sqlx::query_as(
        "select tenant_id, cast(id as char), sending_since from withdrawal where status = 'sending' and sending_since < ? order by id limit 500",
    )
    .bind(now - Duration::seconds(WITHDRAWAL_SENDING_LIMIT_SECS))
    .fetch_all(pool)
    .await?;
    out.extend(rows.into_iter().map(|(tenant_id, id, since)| StuckItem {
        kind: StuckKind::WithdrawalSending,
        tenant_id,
        reference: id,
        since,
        link: "#/payout-queue".into(),
    }));

    let rows: Vec<(u64, String, DateTime<Utc>)> = sqlx::query_as(
        "select p.tenant_id, hex(a.payment_id), min(r.created) from attempt_receipt r join payment_attempt a on a.id = r.attempt_id join payment p on p.id = a.payment_id where r.confirmed = 0 and a.status = 'pending' and r.created < ? group by p.tenant_id, a.payment_id order by min(r.created) limit 500",
    )
    .bind(now - Duration::seconds(receipt_grace_secs))
    .fetch_all(pool)
    .await?;
    out.extend(rows.into_iter().map(|(tenant_id, payment, since)| {
        let payment = payment.to_lowercase();
        StuckItem {
            kind: StuckKind::UnconfirmedReceipt,
            tenant_id,
            link: payment_link(tenant_id, &payment),
            reference: payment,
            since,
        }
    }));

    let rows: Vec<(u64, String, DateTime<Utc>)> = sqlx::query_as(
        "select p.tenant_id, hex(a.payment_id), a.paid_at from payment_attempt a join payment p on p.id = a.payment_id where a.status = 'paid' and a.paid_at < ? and exists (select 1 from webhook_delivery d where d.payment_id = a.payment_id) and not exists (select 1 from webhook_delivery d where d.payment_id = a.payment_id and d.delivered_at is not null) order by a.id limit 500",
    )
    .bind(now - Duration::seconds(WEBHOOK_GRACE_SECS))
    .fetch_all(pool)
    .await?;
    out.extend(rows.into_iter().map(|(tenant_id, payment, since)| {
        let payment = payment.to_lowercase();
        StuckItem {
            kind: StuckKind::PaidWithoutWebhook,
            tenant_id,
            link: format!("#/p/{tenant_id}/webhooks"),
            reference: payment,
            since,
        }
    }));
    Ok(out)
}
