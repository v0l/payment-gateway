use crate::db::{PeriodRow, PlanRow, SubscriptionRow};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct CreatePlan {
    pub name: String,
    pub description: Option<String>,
    pub amount: u64,
    pub currency: String,
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub interval_count: Option<u32>,
    #[serde(default)]
    pub trial_days: Option<u32>,
    #[serde(default)]
    pub grace_days: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct CreateSubscription {
    pub customer_ref: String,
    pub plan_id: u64,
    #[serde(default)]
    pub saved_method_id: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct CancelSubscription {
    #[serde(default)]
    pub at_period_end: bool,
}

#[derive(Debug, Deserialize)]
pub struct ChangePlan {
    pub plan_id: u64,
    #[serde(default)]
    pub proration: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanView {
    pub id: u64,
    pub name: String,
    pub description: Option<String>,
    pub amount: u64,
    pub currency: String,
    pub interval: String,
    pub interval_count: u32,
    pub trial_days: u32,
    pub grace_days: u32,
    pub active: bool,
    pub created: DateTime<Utc>,
}

impl From<&PlanRow> for PlanView {
    fn from(p: &PlanRow) -> Self {
        Self {
            id: p.id,
            name: p.name.clone(),
            description: p.description.clone(),
            amount: p.amount,
            currency: p.currency.clone(),
            interval: p.interval_unit.clone(),
            interval_count: p.interval_count,
            trial_days: p.trial_days,
            grace_days: p.grace_days,
            active: p.active,
            created: p.created,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PeriodView {
    pub index: i32,
    pub period_start: DateTime<Utc>,
    pub period_end: DateTime<Utc>,
    pub amount: u64,
    pub currency: String,
    pub status: String,
    pub failures: u32,
    pub payment_id: Option<String>,
    pub paid_at: Option<DateTime<Utc>>,
}

impl From<&PeriodRow> for PeriodView {
    fn from(p: &PeriodRow) -> Self {
        Self {
            index: p.period_index,
            period_start: p.period_start,
            period_end: p.period_end,
            amount: p.amount,
            currency: p.currency.clone(),
            status: p.status.clone(),
            failures: p.failures,
            payment_id: p.payment_id.as_ref().map(hex::encode),
            paid_at: p.paid_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SubscriptionView {
    pub id: u64,
    pub customer_ref: String,
    pub plan_id: u64,
    pub saved_method_id: Option<u64>,
    pub status: String,
    pub current_period_start: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub trial_end: Option<DateTime<Utc>>,
    pub next_charge: Option<DateTime<Utc>>,
    pub cancel_at_period_end: bool,
    pub credit: i64,
    pub paused_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub cancel_reason: Option<String>,
    pub created: DateTime<Utc>,
    pub payment_id: Option<String>,
    pub pay_url: Option<String>,
    pub periods: Vec<PeriodView>,
}

impl SubscriptionView {
    pub fn build(s: &SubscriptionRow, periods: &[PeriodRow], public_url: &str) -> Self {
        let open = periods
            .iter()
            .find(|p| p.status == crate::billing::PERIOD_OPEN && p.payment_id.is_some());
        let payment_id = open.and_then(|p| p.payment_id.as_ref()).map(hex::encode);
        Self {
            id: s.id,
            customer_ref: s.customer_ref.clone(),
            plan_id: s.plan_id,
            saved_method_id: s.saved_method_id,
            status: s.status.clone(),
            current_period_start: s.current_period_start,
            current_period_end: s.current_period_end,
            trial_end: s.trial_end,
            next_charge: s.next_charge,
            cancel_at_period_end: s.cancel_at_period_end,
            credit: -s.adjustment,
            paused_at: s.paused_at,
            cancelled_at: s.cancelled_at,
            cancel_reason: s.cancel_reason.clone(),
            created: s.created,
            pay_url: payment_id
                .as_ref()
                .filter(|_| s.saved_method_id.is_none())
                .map(|id| format!("{public_url}/pay/{id}")),
            payment_id,
            periods: periods.iter().map(PeriodView::from).collect(),
        }
    }
}
