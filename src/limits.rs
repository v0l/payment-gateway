use crate::db::Tenant;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, FromRow, PartialEq)]
pub struct LimitOverrides {
    pub limit_payments_per_day: Option<u64>,
    pub limit_pending_payments: Option<u64>,
    pub limit_amount_per_payment: Option<u64>,
    pub limit_volume_per_day: Option<u64>,
    pub limit_providers: Option<u64>,
    pub limit_webhook_endpoints: Option<u64>,
    pub limit_attempts_per_payment: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct Limits {
    pub payments_per_day: u64,
    pub pending_payments: u64,
    pub amount_per_payment: u64,
    pub volume_per_day: u64,
    pub providers: u64,
    pub webhook_endpoints: u64,
    pub attempts_per_payment: u64,
}

pub const NAMES: [&str; 7] = [
    "payments_per_day",
    "pending_payments",
    "amount_per_payment",
    "volume_per_day",
    "providers",
    "webhook_endpoints",
    "attempts_per_payment",
];

pub const LIMIT_CURRENCY: &str = "EUR";

impl Limits {
    pub const UNTRUSTED: Limits = Limits {
        payments_per_day: 100,
        pending_payments: 50,
        amount_per_payment: 100_000,
        volume_per_day: 500_000,
        providers: 5,
        webhook_endpoints: 1,
        attempts_per_payment: 10,
    };

    pub const TRUSTED: Limits = Limits {
        payments_per_day: 100_000,
        pending_payments: 10_000,
        amount_per_payment: 100_000_000,
        volume_per_day: 10_000_000_000,
        providers: 50,
        webhook_endpoints: 10,
        attempts_per_payment: 50,
    };

    pub fn defaults(trusted: bool) -> Limits {
        if trusted {
            Self::TRUSTED
        } else {
            Self::UNTRUSTED
        }
    }

    pub fn for_tenant(t: &Tenant) -> Limits {
        let d = Self::defaults(t.trusted);
        let o = &t.limits;
        Limits {
            payments_per_day: o.limit_payments_per_day.unwrap_or(d.payments_per_day),
            pending_payments: o.limit_pending_payments.unwrap_or(d.pending_payments),
            amount_per_payment: o.limit_amount_per_payment.unwrap_or(d.amount_per_payment),
            volume_per_day: o.limit_volume_per_day.unwrap_or(d.volume_per_day),
            providers: o.limit_providers.unwrap_or(d.providers),
            webhook_endpoints: o.limit_webhook_endpoints.unwrap_or(d.webhook_endpoints),
            attempts_per_payment: o
                .limit_attempts_per_payment
                .unwrap_or(d.attempts_per_payment),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq)]
pub struct Usage {
    pub payments_per_day: u64,
    pub pending_payments: u64,
    pub volume_per_day: u64,
    pub providers: u64,
    pub webhook_endpoints: u64,
}

#[derive(Debug, Clone)]
pub struct LimitHit {
    pub name: &'static str,
    pub current: u64,
    pub max: u64,
}

impl LimitHit {
    pub fn new(name: &'static str, current: u64, max: u64) -> Self {
        Self { name, current, max }
    }

    pub fn message(&self) -> String {
        format!(
            "limit_exceeded: {} is {} of at most {}",
            self.name, self.current, self.max
        )
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct LimitsInput {
    #[serde(default, deserialize_with = "double_option")]
    pub payments_per_day: Option<Option<u64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub pending_payments: Option<Option<u64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub amount_per_payment: Option<Option<u64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub volume_per_day: Option<Option<u64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub providers: Option<Option<u64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub webhook_endpoints: Option<Option<u64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub attempts_per_payment: Option<Option<u64>>,
}

fn double_option<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<u64>>, D::Error> {
    Option::<u64>::deserialize(d).map(Some)
}

impl LimitsInput {
    pub fn apply(self, mut o: LimitOverrides) -> LimitOverrides {
        fn set(slot: &mut Option<u64>, v: Option<Option<u64>>) {
            if let Some(v) = v {
                *slot = v;
            }
        }
        set(&mut o.limit_payments_per_day, self.payments_per_day);
        set(&mut o.limit_pending_payments, self.pending_payments);
        set(&mut o.limit_amount_per_payment, self.amount_per_payment);
        set(&mut o.limit_volume_per_day, self.volume_per_day);
        set(&mut o.limit_providers, self.providers);
        set(&mut o.limit_webhook_endpoints, self.webhook_endpoints);
        set(&mut o.limit_attempts_per_payment, self.attempts_per_payment);
        o
    }
}

pub fn diff(before: &LimitOverrides, after: &LimitOverrides) -> [(&'static str, bool); 7] {
    [
        (
            "payments_per_day",
            before.limit_payments_per_day != after.limit_payments_per_day,
        ),
        (
            "pending_payments",
            before.limit_pending_payments != after.limit_pending_payments,
        ),
        (
            "amount_per_payment",
            before.limit_amount_per_payment != after.limit_amount_per_payment,
        ),
        (
            "volume_per_day",
            before.limit_volume_per_day != after.limit_volume_per_day,
        ),
        ("providers", before.limit_providers != after.limit_providers),
        (
            "webhook_endpoints",
            before.limit_webhook_endpoints != after.limit_webhook_endpoints,
        ),
        (
            "attempts_per_payment",
            before.limit_attempts_per_payment != after.limit_attempts_per_payment,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn untrusted_defaults_are_below_trusted() {
        let (u, t) = (Limits::UNTRUSTED, Limits::TRUSTED);
        assert!(u.payments_per_day < t.payments_per_day);
        assert!(u.pending_payments < t.pending_payments);
        assert!(u.amount_per_payment < t.amount_per_payment);
        assert!(u.volume_per_day < t.volume_per_day);
        assert!(u.providers < t.providers);
        assert!(u.webhook_endpoints < t.webhook_endpoints);
        assert!(u.attempts_per_payment < t.attempts_per_payment);
    }

    #[test]
    fn input_sets_and_clears_overrides() {
        let o = LimitOverrides {
            limit_providers: Some(3),
            ..Default::default()
        };
        let input: LimitsInput =
            serde_json::from_str(r#"{"providers": null, "pending_payments": 7}"#).unwrap();
        let o = input.apply(o);
        assert_eq!(o.limit_providers, None);
        assert_eq!(o.limit_pending_payments, Some(7));
    }
}
