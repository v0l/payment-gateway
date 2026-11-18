use chrono::{DateTime, Duration, Months, Utc};

pub const RETRY_DAYS: [i64; 3] = [1, 3, 5];
pub const DEFAULT_GRACE_DAYS: u32 = 3;
pub const MAX_GRACE_DAYS: u32 = 30;
pub const MAX_INTERVAL_COUNT: u32 = 365;
pub const MAX_TRIAL_DAYS: u32 = 365;
pub const CLAIM_SECS: i64 = 600;
pub const IN_FLIGHT_RECHECK_SECS: i64 = 300;

pub const STATUS_INCOMPLETE: &str = "incomplete";
pub const STATUS_TRIALING: &str = "trialing";
pub const STATUS_ACTIVE: &str = "active";
pub const STATUS_PAST_DUE: &str = "past_due";
pub const STATUS_PAUSED: &str = "paused";
pub const STATUS_CANCELLED: &str = "cancelled";

pub const PERIOD_OPEN: &str = "open";
pub const PERIOD_PAID: &str = "paid";
pub const PERIOD_VOID: &str = "void";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    Day,
    Week,
    Month,
    Year,
}

impl Unit {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "day" => Some(Self::Day),
            "week" => Some(Self::Week),
            "month" => Some(Self::Month),
            "year" => Some(Self::Year),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
            Self::Year => "year",
        }
    }
}

pub fn add_intervals(
    anchor: DateTime<Utc>,
    unit: Unit,
    count: u32,
    steps: i64,
) -> DateTime<Utc> {
    let total = count as i64 * steps;
    match unit {
        Unit::Day => anchor + Duration::days(total),
        Unit::Week => anchor + Duration::days(total * 7),
        Unit::Month => shift_months(anchor, total),
        Unit::Year => shift_months(anchor, total * 12),
    }
}

fn shift_months(anchor: DateTime<Utc>, months: i64) -> DateTime<Utc> {
    let m = Months::new(months.unsigned_abs() as u32);
    let shifted = if months >= 0 {
        anchor.checked_add_months(m)
    } else {
        anchor.checked_sub_months(m)
    };
    shifted.unwrap_or(anchor)
}

pub fn period_bounds(
    anchor: DateTime<Utc>,
    anchor_index: i32,
    unit: Unit,
    count: u32,
    index: i32,
) -> (DateTime<Utc>, DateTime<Utc>) {
    let from_anchor = (index - anchor_index) as i64;
    (
        add_intervals(anchor, unit, count, from_anchor),
        add_intervals(anchor, unit, count, from_anchor + 1),
    )
}

pub fn retry_at(period_start: DateTime<Utc>, failed_attempts: u32) -> Option<DateTime<Utc>> {
    RETRY_DAYS
        .get(failed_attempts.checked_sub(1)? as usize)
        .map(|d| period_start + Duration::days(*d))
}

pub fn remaining_days(end: DateTime<Utc>, now: DateTime<Utc>, total_days: i64) -> i64 {
    let secs = (end - now).num_seconds().max(0);
    ((secs + 86_399) / 86_400).min(total_days)
}

pub fn period_days(start: DateTime<Utc>, end: DateTime<Utc>) -> i64 {
    ((end - start).num_seconds() / 86_400).max(1)
}

pub fn prorated_difference(
    old_amount: u64,
    new_amount: u64,
    period_start: DateTime<Utc>,
    period_end: DateTime<Utc>,
    now: DateTime<Utc>,
) -> i64 {
    let total = period_days(period_start, period_end);
    let remaining = remaining_days(period_end, now, total);
    let diff = new_amount as i128 - old_amount as i128;
    (diff * remaining as i128 / total as i128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).unwrap()
    }

    #[test]
    fn monthly_periods_clamp_to_month_end_without_drifting() {
        let anchor = at(2027, 1, 31);
        let ends: Vec<_> = (0..4)
            .map(|i| period_bounds(anchor, 0, Unit::Month, 1, i).1)
            .collect();
        assert_eq!(ends[0], at(2027, 2, 28));
        assert_eq!(ends[1], at(2027, 3, 31));
        assert_eq!(ends[2], at(2027, 4, 30));
        assert_eq!(ends[3], at(2027, 5, 31));
    }

    #[test]
    fn leap_day_anchors_clamp_on_non_leap_years() {
        let anchor = at(2028, 2, 29);
        assert_eq!(
            add_intervals(anchor, Unit::Year, 1, 1),
            at(2029, 2, 28)
        );
        assert_eq!(add_intervals(anchor, Unit::Year, 1, 4), at(2032, 2, 29));
        assert_eq!(add_intervals(at(2028, 1, 31), Unit::Month, 1, 1), at(2028, 2, 29));
    }

    #[test]
    fn day_and_week_intervals_are_fixed_lengths() {
        let anchor = at(2027, 12, 30);
        assert_eq!(add_intervals(anchor, Unit::Day, 3, 1), at(2028, 1, 2));
        assert_eq!(add_intervals(anchor, Unit::Week, 2, 1), at(2028, 1, 13));
    }

    #[test]
    fn periods_after_a_re_anchor_start_at_the_anchor() {
        let anchor = at(2027, 3, 15);
        let (s, e) = period_bounds(anchor, 5, Unit::Month, 1, 5);
        assert_eq!((s, e), (anchor, at(2027, 4, 15)));
        let (s, _) = period_bounds(anchor, 5, Unit::Month, 1, 6);
        assert_eq!(s, at(2027, 4, 15));
    }

    #[test]
    fn retry_schedule_is_one_three_five_days_from_the_period_start() {
        let start = at(2027, 1, 1);
        assert_eq!(retry_at(start, 1), Some(at(2027, 1, 2)));
        assert_eq!(retry_at(start, 2), Some(at(2027, 1, 4)));
        assert_eq!(retry_at(start, 3), Some(at(2027, 1, 6)));
        assert_eq!(retry_at(start, 4), None);
        assert_eq!(retry_at(start, 0), None);
    }

    #[test]
    fn proration_is_the_price_difference_times_remaining_days_over_period_days() {
        let start = at(2027, 1, 1);
        let end = at(2027, 1, 31);
        assert_eq!(prorated_difference(1000, 4000, start, end, start), 3000);
        assert_eq!(prorated_difference(1000, 4000, start, end, at(2027, 1, 16)), 1500);
        assert_eq!(prorated_difference(4000, 1000, start, end, at(2027, 1, 16)), -1500);
        assert_eq!(prorated_difference(1000, 4000, start, end, end), 0);
        assert_eq!(prorated_difference(1000, 1000, start, end, start), 0);
    }
}
