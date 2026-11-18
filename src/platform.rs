use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

pub const CURRENCY: &str = "EUR";
pub const INVOICE_DUE_DAYS: i64 = 3;
pub const PAYMENT_EXPIRY_DAYS: i64 = 30;
pub const RESTRICT_AFTER_DAYS: i64 = 7;
pub const DISABLE_AFTER_DAYS: i64 = 21;
pub const BPS: u128 = 10_000;

pub const STANDING_CURRENT: &str = "current";
pub const STANDING_WARNED: &str = "warned";
pub const STANDING_RESTRICTED: &str = "restricted";
pub const STANDING_DISABLED: &str = "disabled";

pub const INVOICE_OPEN: &str = "open";
pub const INVOICE_PAID: &str = "paid";
pub const INVOICE_WAIVED: &str = "waived";

pub const KIND_PERIOD: &str = "period";
pub const KIND_PLAN: &str = "plan";
pub const KIND_UPGRADE: &str = "upgrade";
pub const KIND_USAGE: &str = "usage";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Standing {
    Current,
    Warned,
    Restricted,
    Disabled,
}

impl Standing {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Current => STANDING_CURRENT,
            Self::Warned => STANDING_WARNED,
            Self::Restricted => STANDING_RESTRICTED,
            Self::Disabled => STANDING_DISABLED,
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            STANDING_WARNED => Self::Warned,
            STANDING_RESTRICTED => Self::Restricted,
            STANDING_DISABLED => Self::Disabled,
            _ => Self::Current,
        }
    }

    pub fn blocks_new_payments(self) -> bool {
        self >= Self::Restricted
    }

    pub fn blocks_new_attempts(self) -> bool {
        self >= Self::Disabled
    }
}

pub fn standing_for(oldest_due: Option<DateTime<Utc>>, now: DateTime<Utc>) -> Standing {
    let Some(due) = oldest_due else {
        return Standing::Current;
    };
    if due > now {
        return Standing::Current;
    }
    let overdue = now - due;
    if overdue >= Duration::days(DISABLE_AFTER_DAYS) {
        Standing::Disabled
    } else if overdue >= Duration::days(RESTRICT_AFTER_DAYS) {
        Standing::Restricted
    } else {
        Standing::Warned
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fees {
    pub monthly_price: u64,
    pub included_volume: u64,
    pub fee_bps: u32,
    pub fee_base: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Metered {
    pub count: u64,
    pub volume: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsageFee {
    pub per_transaction: u64,
    pub billable_volume: u64,
    pub percent: u64,
}

impl UsageFee {
    pub fn total(&self) -> u64 {
        self.per_transaction.saturating_add(self.percent)
    }
}

pub fn usage_fee(fees: &Fees, used: Metered) -> UsageFee {
    let per_transaction = used.count.saturating_mul(fees.fee_base);
    let billable_volume = used.volume.saturating_sub(fees.included_volume);
    let percent = (billable_volume as u128 * fees.fee_bps as u128 / BPS) as u64;
    UsageFee {
        per_transaction,
        billable_volume,
        percent,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct InvoiceLine {
    pub name: String,
    pub quantity: u64,
    pub unit_amount: u64,
}

impl InvoiceLine {
    pub fn new(name: impl Into<String>, quantity: u64, unit_amount: u64) -> Self {
        Self {
            name: name.into(),
            quantity,
            unit_amount,
        }
    }

    pub fn total(&self) -> u64 {
        self.quantity.saturating_mul(self.unit_amount)
    }
}

pub fn subtotal(lines: &[InvoiceLine]) -> u64 {
    lines.iter().map(InvoiceLine::total).sum()
}

pub fn usage_lines(fees: &Fees, used: Metered) -> Vec<InvoiceLine> {
    let fee = usage_fee(fees, used);
    let mut lines = vec![];
    if fee.per_transaction > 0 {
        lines.push(InvoiceLine::new(
            format!("Transaction fee ({} settled payments)", used.count),
            used.count,
            fees.fee_base,
        ));
    }
    if fee.percent > 0 {
        lines.push(InvoiceLine::new(
            format!(
                "Volume fee {}.{:02}% on {} above the included volume",
                fees.fee_bps / 100,
                fees.fee_bps % 100,
                fee.billable_volume
            ),
            1,
            fee.percent,
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2030, 1, d, 12, 0, 0).unwrap()
    }

    #[test]
    fn standing_follows_days_overdue() {
        assert_eq!(standing_for(None, at(1)), Standing::Current);
        assert_eq!(standing_for(Some(at(5)), at(1)), Standing::Current);
        assert_eq!(standing_for(Some(at(1)), at(1)), Standing::Warned);
        assert_eq!(standing_for(Some(at(1)), at(7)), Standing::Warned);
        assert_eq!(standing_for(Some(at(1)), at(8)), Standing::Restricted);
        assert_eq!(standing_for(Some(at(1)), at(22)), Standing::Disabled);
    }

    #[test]
    fn usage_fee_charges_percent_only_above_included_volume() {
        let fees = Fees {
            monthly_price: 0,
            included_volume: 1_000,
            fee_bps: 100,
            fee_base: 5,
        };
        let fee = usage_fee(
            &fees,
            Metered {
                count: 10,
                volume: 3_000,
            },
        );
        assert_eq!(fee.per_transaction, 50);
        assert_eq!(fee.billable_volume, 2_000);
        assert_eq!(fee.percent, 20);
        let under = usage_fee(
            &fees,
            Metered {
                count: 1,
                volume: 900,
            },
        );
        assert_eq!(under.percent, 0);
    }

    #[test]
    fn zero_prices_give_no_lines() {
        let fees = Fees {
            monthly_price: 0,
            included_volume: 0,
            fee_bps: 0,
            fee_base: 0,
        };
        let used = Metered {
            count: 5,
            volume: 99_999,
        };
        assert!(usage_lines(&fees, used).is_empty());
    }
}
