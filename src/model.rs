use crate::db::{AttemptRow, PaymentRow, RefundRow};
use crate::invoice::{AppliedDiscount, DiscountView, Line, LineView};
use crate::rates;
use anyhow::{Result, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const METHODS: &[&str] = &[
    "lightning",
    "onchain",
    "stripe",
    "revolut",
    "paypal",
    "fixedfloat",
];

pub fn is_fiat_method(method: &str) -> bool {
    matches!(method, "stripe" | "revolut" | "paypal")
}

pub fn charge_currency(method: &str, payment_currency: &str) -> Result<String> {
    let payment_currency = payment_currency.to_uppercase();
    if is_fiat_method(method) {
        if !rates::is_card_currency(&payment_currency) {
            bail!("{method} cannot charge in {payment_currency}");
        }
        Ok(payment_currency)
    } else {
        Ok("BTC".to_string())
    }
}

pub fn method_supports(method: &str, payment_currency: &str) -> bool {
    charge_currency(method, payment_currency).is_ok()
}

pub fn options_from_config(method: &str, config: &Value) -> Vec<String> {
    if method != "fixedfloat" {
        return vec![];
    }
    config
        .get("coins")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|s| s.to_uppercase())
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
pub struct CreatePayment {
    pub reference: Option<String>,
    #[serde(default)]
    pub amount: u64,
    pub currency: String,
    #[serde(default)]
    pub tax: u64,
    pub line_items: Option<Vec<crate::invoice::LineItemInput>>,
    pub discount: Option<crate::invoice::DiscountInput>,
    pub expires_in: Option<i64>,
    pub description: Option<String>,
    pub customer_ref: Option<String>,
    pub customer_email: Option<String>,
    pub metadata: Option<Value>,
    pub methods: Option<Vec<String>>,
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct CreateAttempt {
    pub method: String,
    pub provider_id: Option<u64>,
    pub option: Option<String>,
    #[serde(default)]
    pub save_method: bool,
    pub saved_method_id: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RefundInput {
    pub amount: Option<u64>,
    pub destination: Option<String>,
    pub attempt_id: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttemptView {
    pub id: u64,
    pub provider_id: u64,
    pub method: String,
    pub option: Option<String>,
    pub status: String,
    pub amount: u64,
    pub currency: String,
    pub base_amount: u64,
    pub processing_fee: u64,
    pub expires: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,
    pub details: Value,
    pub platform_managed: bool,
    pub platform_fee: Option<u64>,
    pub net_credited: Option<u64>,
}

impl From<&AttemptRow> for AttemptView {
    fn from(a: &AttemptRow) -> Self {
        Self {
            id: a.id,
            provider_id: a.provider_config_id,
            method: a.method.clone(),
            option: (!a.option_key.is_empty()).then(|| a.option_key.clone()),
            status: a.status.clone(),
            amount: a.charge_amount,
            currency: a.charge_currency.clone(),
            base_amount: a.base_amount,
            processing_fee: a.processing_fee,
            expires: a.expires,
            paid_at: a.paid_at,
            details: serde_json::from_str(&a.details).unwrap_or(Value::Null),
            platform_managed: a.shared_provider_id.is_some(),
            platform_fee: a.platform_fee,
            net_credited: a.platform_net,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RefundView {
    pub id: u64,
    pub attempt_id: u64,
    pub amount: u64,
    pub currency: String,
    pub status: String,
    pub error: Option<String>,
    pub created: DateTime<Utc>,
}

impl From<&RefundRow> for RefundView {
    fn from(r: &RefundRow) -> Self {
        Self {
            id: r.id,
            attempt_id: r.attempt_id,
            amount: r.amount,
            currency: r.currency.clone(),
            status: r.status.clone(),
            error: r.error.clone(),
            created: r.created,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PaymentView {
    pub id: String,
    pub reference: String,
    pub amount: u64,
    pub currency: String,
    pub tax: u64,
    pub status: String,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,
    pub description: Option<String>,
    pub customer_ref: Option<String>,
    pub metadata: Value,
    pub line_items: Vec<LineView>,
    pub subtotal: Option<u64>,
    pub discount: Option<DiscountView>,
    pub receipt_url: String,
    pub pay_url: String,
    #[serde(default)]
    pub disputed: bool,
    #[serde(default)]
    pub dispute_status: Option<String>,
    pub attempts: Vec<AttemptView>,
    pub refunds: Vec<RefundView>,
}

impl PaymentView {
    pub fn build(
        p: &PaymentRow,
        public_url: &str,
        attempts: &[AttemptRow],
        refunds: &[RefundRow],
    ) -> Self {
        let breakdown = p.breakdown();
        Self {
            id: hex::encode(&p.id),
            reference: p.reference.clone(),
            amount: p.amount,
            currency: p.currency.clone(),
            tax: p.tax,
            status: p.status.clone(),
            created: p.created,
            expires: p.expires,
            paid_at: p.paid_at,
            description: p.description.clone(),
            customer_ref: p.customer_ref.clone(),
            metadata: p
                .metadata
                .as_deref()
                .and_then(|m| serde_json::from_str(m).ok())
                .unwrap_or(Value::Null),
            line_items: breakdown
                .as_ref()
                .map(|b| b.lines.iter().map(Line::view).collect())
                .unwrap_or_default(),
            subtotal: breakdown.as_ref().map(|b| b.subtotal),
            discount: breakdown
                .as_ref()
                .and_then(|b| b.discount.as_ref().map(AppliedDiscount::view)),
            receipt_url: format!(
                "{}/receipt/{}",
                public_url.trim_end_matches('/'),
                hex::encode(&p.id)
            ),
            pay_url: format!(
                "{}/pay/{}",
                public_url.trim_end_matches('/'),
                hex::encode(&p.id)
            ),
            disputed: p.dispute_status.as_deref() == Some("open"),
            dispute_status: p.dispute_status.clone(),
            attempts: attempts.iter().map(Into::into).collect(),
            refunds: refunds.iter().map(Into::into).collect(),
        }
    }
}

pub fn origin_of(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    let origin = u.origin();
    origin.is_tuple().then(|| origin.ascii_serialization())
}

pub fn url_in_origins(url: &str, origins: &[String]) -> bool {
    origin_of(url).is_some_and(|o| origins.contains(&o))
}

pub fn normalize_origins(raw: &str) -> Result<Vec<String>> {
    raw.split_whitespace()
        .map(|o| match origin_of(o) {
            Some(norm) => Ok(norm),
            None => bail!("invalid origin {o}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fiat_methods_need_card_currency_and_btc_methods_take_anything() {
        assert_eq!(charge_currency("stripe", "eur").unwrap(), "EUR");
        assert!(charge_currency("stripe", "BTC").is_err());
        assert!(charge_currency("revolut", "JPY").is_err());
        assert_eq!(charge_currency("lightning", "USD").unwrap(), "BTC");
        assert_eq!(charge_currency("fixedfloat", "EUR").unwrap(), "BTC");
        assert!(method_supports("onchain", "BTC"));
    }

    #[test]
    fn origins_are_normalised_and_validated() {
        assert_eq!(
            origin_of("https://Example.com/a/b?x=1").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            origin_of("http://localhost:3000/x").unwrap(),
            "http://localhost:3000"
        );
        assert!(origin_of("javascript:alert(1)").is_none());
        assert!(origin_of("ftp://x").is_none());
        assert_eq!(
            normalize_origins("https://a.com https://b.com:8443/").unwrap(),
            vec!["https://a.com", "https://b.com:8443"]
        );
        assert!(normalize_origins("nonsense").is_err());
    }

    #[test]
    fn fixedfloat_coins_come_from_config() {
        let cfg = serde_json::json!({"coins": ["xmr", "LTC"]});
        assert_eq!(options_from_config("fixedfloat", &cfg), vec!["XMR", "LTC"]);
        assert!(options_from_config("stripe", &cfg).is_empty());
    }
}
