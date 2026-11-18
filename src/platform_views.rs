use crate::db::{PlatformAccountRow, PlatformInvoiceRow, PlatformPlanRow};
use crate::platform::{InvoiceLine, Standing};
use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct InvoiceUsage {
    pub count: u64,
    pub volume: u64,
}

#[derive(Debug, Serialize)]
pub struct PlatformInvoiceView {
    pub id: u64,
    pub kind: String,
    pub status: String,
    pub past_due: bool,
    pub currency: String,
    pub period_start: i64,
    pub period_end: i64,
    pub lines: Vec<InvoiceLine>,
    pub usage: InvoiceUsage,
    pub subtotal: u64,
    pub credit_applied: u64,
    pub total: u64,
    pub due_at: i64,
    pub paid_at: Option<i64>,
    pub reason: Option<String>,
    pub payment_method: Option<String>,
    pub pay_url: Option<String>,
    pub receipt_url: Option<String>,
}

impl PlatformInvoiceView {
    pub fn build(
        row: &PlatformInvoiceRow,
        public_url: &str,
        payment_method: Option<String>,
        now: DateTime<Utc>,
    ) -> Self {
        let payment = row.payment_id.as_ref().map(hex::encode);
        let open = row.status == crate::platform::INVOICE_OPEN;
        let paid = row.status == crate::platform::INVOICE_PAID;
        Self {
            id: row.id,
            kind: row.kind.clone(),
            status: row.status.clone(),
            past_due: open && row.due_at <= now,
            currency: row.currency.clone(),
            period_start: row.period_start.timestamp(),
            period_end: row.period_end.timestamp(),
            lines: serde_json::from_str(&row.line_items).unwrap_or_default(),
            usage: InvoiceUsage {
                count: row.usage_count,
                volume: row.usage_volume,
            },
            subtotal: row.subtotal,
            credit_applied: row.credit_applied,
            total: row.total,
            due_at: row.due_at.timestamp(),
            paid_at: row.paid_at.map(|t| t.timestamp()),
            reason: row.reason.clone(),
            payment_method,
            pay_url: payment
                .as_ref()
                .filter(|_| open)
                .map(|p| format!("{public_url}/pay/{p}")),
            receipt_url: payment
                .filter(|_| paid)
                .map(|p| format!("{public_url}/receipt/{p}")),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlatformPlanView {
    pub id: u64,
    pub name: String,
    pub live: bool,
    pub monthly_price: u64,
    pub currency: String,
    pub included_volume: u64,
    pub fee_bps: u32,
    pub fee_base: u64,
    pub limits: crate::limits::LimitOverrides,
    pub active: bool,
}

impl From<&PlatformPlanRow> for PlatformPlanView {
    fn from(p: &PlatformPlanRow) -> Self {
        Self {
            id: p.id,
            name: p.name.clone(),
            live: p.live,
            monthly_price: p.monthly_price,
            currency: p.currency.clone(),
            included_volume: p.included_volume,
            fee_bps: p.fee_bps,
            fee_base: p.fee_base,
            limits: p.limits,
            active: p.active,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct PlatformAccountView {
    pub plan: PlatformPlanView,
    pub standing: &'static str,
    pub standing_since: Option<i64>,
    pub period_start: i64,
    pub period_end: i64,
    pub credit: u64,
    pub usage_count: u64,
    pub usage_volume: u64,
    pub estimated_usage_fee: u64,
    pub restricted: bool,
}

impl PlatformAccountView {
    pub fn build(
        account: &PlatformAccountRow,
        plan: &PlatformPlanRow,
        usage: crate::platform::Metered,
    ) -> Self {
        let standing = Standing::parse(&account.status);
        Self {
            plan: PlatformPlanView::from(plan),
            standing: standing.as_str(),
            standing_since: account.status_since.map(|t| t.timestamp()),
            period_start: account.period_start.timestamp(),
            period_end: account.period_end.timestamp(),
            credit: account.credit,
            usage_count: usage.count,
            usage_volume: usage.volume,
            estimated_usage_fee: crate::platform::usage_fee(&plan.fees(), usage).total(),
            restricted: standing.blocks_new_payments(),
        }
    }
}
