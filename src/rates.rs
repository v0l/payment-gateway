use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use payments_rs::currency::Currency;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const MSATS_PER_BTC: f64 = 1.0e11;

#[async_trait]
pub trait RateService: Send + Sync {
    async fn btc_price(&self, currency: &str) -> Result<f64>;
    async fn all(&self) -> Result<HashMap<String, f64>>;
}

pub struct StaticRates(pub HashMap<String, f64>);

impl StaticRates {
    pub fn new(rates: &[(&str, f64)]) -> Self {
        Self(rates.iter().map(|(k, v)| (k.to_string(), *v)).collect())
    }
}

#[async_trait]
impl RateService for StaticRates {
    async fn btc_price(&self, currency: &str) -> Result<f64> {
        self.0
            .get(&currency.to_uppercase())
            .copied()
            .ok_or_else(|| anyhow!("no rate for {currency}"))
    }

    async fn all(&self) -> Result<HashMap<String, f64>> {
        Ok(self.0.clone())
    }
}

pub struct HttpRates {
    client: reqwest::Client,
    url: String,
    ttl: Duration,
    cache: Mutex<Option<(Instant, HashMap<String, f64>)>>,
}

impl HttpRates {
    pub fn new(url: impl Into<String>, ttl: Duration) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .connect_timeout(Duration::from_secs(5))
                .build()
                .expect("http client"),
            url: url.into(),
            ttl,
            cache: Mutex::new(None),
        }
    }

    pub fn mempool() -> Self {
        Self::new(
            "https://mempool.space/api/v1/prices",
            Duration::from_secs(60),
        )
    }

    async fn fetch(&self) -> Result<HashMap<String, f64>> {
        let body: serde_json::Value = self
            .client
            .get(&self.url)
            .timeout(Duration::from_secs(10))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let rates: HashMap<String, f64> = body
            .as_object()
            .ok_or_else(|| anyhow!("unexpected rates response"))?
            .iter()
            .filter(|(k, _)| k.len() == 3 && k.chars().all(|c| c.is_ascii_uppercase()))
            .filter_map(|(k, v)| v.as_f64().filter(|p| *p > 0.0).map(|p| (k.clone(), p)))
            .collect();
        if rates.is_empty() {
            bail!("rates response had no prices");
        }
        Ok(rates)
    }
}

#[async_trait]
impl RateService for HttpRates {
    async fn all(&self) -> Result<HashMap<String, f64>> {
        {
            let cache = self.cache.lock().unwrap();
            if let Some((at, rates)) = cache.as_ref()
                && at.elapsed() < self.ttl
            {
                return Ok(rates.clone());
            }
        }
        match self.fetch().await {
            Ok(rates) => {
                *self.cache.lock().unwrap() = Some((Instant::now(), rates.clone()));
                Ok(rates)
            }
            Err(e) => match self.cache.lock().unwrap().as_ref() {
                Some((_, stale)) => {
                    log::warn!("rate fetch failed, using stale rates: {e:#}");
                    Ok(stale.clone())
                }
                None => Err(e),
            },
        }
    }

    async fn btc_price(&self, currency: &str) -> Result<f64> {
        self.all()
            .await?
            .get(&currency.to_uppercase())
            .copied()
            .ok_or_else(|| anyhow!("no rate for {currency}"))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Unit {
    Btc,
    Fiat { base: String, scale: f64 },
}

pub fn unit(code: &str) -> Result<Unit> {
    let cur = Currency::from_str(code).map_err(|_| anyhow!("unsupported currency {code}"))?;
    if cur == Currency::BTC {
        return Ok(Unit::Btc);
    }
    if cur.is_crypto() {
        bail!("{code} cannot be used as a payment currency");
    }
    Ok(Unit::Fiat {
        base: cur.base().to_string().to_uppercase(),
        scale: cur.scale(),
    })
}

pub fn is_card_currency(code: &str) -> bool {
    matches!(
        code.to_uppercase().as_str(),
        "EUR" | "USD" | "GBP" | "CAD" | "CHF" | "AUD"
    )
}

pub const MAX_AMOUNT: u64 = 1_000_000_000_000_000;

async fn checked_price(rates: &dyn RateService, base: &str) -> Result<f64> {
    let price = rates.btc_price(base).await?;
    if !price.is_finite() || price <= 0.0 {
        bail!("no usable rate for {base}");
    }
    Ok(price)
}

fn to_minor(value: f64) -> Result<u64> {
    if !value.is_finite() || value < 0.0 || value > MAX_AMOUNT as f64 {
        bail!("converted amount out of range");
    }
    Ok(value.ceil() as u64)
}

pub async fn convert(rates: &dyn RateService, amount: u64, from: &str, to: &str) -> Result<u64> {
    let (from, to) = (from.to_uppercase(), to.to_uppercase());
    if from == to {
        return Ok(amount);
    }
    if amount > MAX_AMOUNT {
        bail!("amount out of range");
    }
    let btc = match unit(&from)? {
        Unit::Btc => amount as f64 / MSATS_PER_BTC,
        Unit::Fiat { base, scale } => amount as f64 / scale / checked_price(rates, &base).await?,
    };
    match unit(&to)? {
        Unit::Btc => to_minor(btc * MSATS_PER_BTC),
        Unit::Fiat { base, scale } => to_minor(btc * checked_price(rates, &base).await? * scale),
    }
}

pub async fn processing_fee(
    rates: &dyn RateService,
    fee_rate: Option<f64>,
    fee_base: Option<u64>,
    fee_currency: Option<&str>,
    amount: u64,
    currency: &str,
) -> Result<u64> {
    let percent = match fee_rate {
        Some(r) if r.is_finite() && (0.0..=100.0).contains(&r) => {
            ((amount as u128 * (r * 1_000_000.0).round() as u128).div_ceil(100_000_000)) as u64
        }
        Some(_) => bail!("fee_rate must be 0-100"),
        None => 0,
    };
    let base = match (fee_base, fee_currency) {
        (Some(b), Some(c)) if !c.eq_ignore_ascii_case(currency) => {
            convert(rates, b, c, currency).await?
        }
        (Some(b), _) => b,
        _ => 0,
    };
    Ok(percent.saturating_add(base))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rates() -> StaticRates {
        StaticRates::new(&[("EUR", 50_000.0), ("USD", 60_000.0)])
    }

    #[tokio::test]
    async fn eur_to_msats_and_back() {
        let r = rates();
        let msats = convert(&r, 5_000, "EUR", "BTC").await.unwrap();
        assert_eq!(msats, 100_000_000);
        assert_eq!(convert(&r, msats, "BTC", "EUR").await.unwrap(), 5_000);
    }

    #[tokio::test]
    async fn fiat_cross_rate_goes_through_btc() {
        let r = rates();
        assert_eq!(convert(&r, 5_000, "EUR", "USD").await.unwrap(), 6_000);
    }

    #[tokio::test]
    async fn same_currency_is_identity_and_milli_fiat_scales() {
        let r = rates();
        assert_eq!(convert(&r, 123, "EUR", "EUR").await.unwrap(), 123);
        assert_eq!(convert(&r, 100, "EUR", "MEUR").await.unwrap(), 100_000);
    }

    #[tokio::test]
    async fn crypto_payment_currency_is_rejected() {
        assert!(convert(&rates(), 1, "XMR", "EUR").await.is_err());
        assert!(convert(&rates(), 1, "NOPE", "EUR").await.is_err());
    }

    #[tokio::test]
    async fn fee_is_percent_plus_base_in_payment_currency() {
        let r = rates();
        let fee = processing_fee(&r, Some(1.5), Some(25), Some("EUR"), 1_000, "EUR")
            .await
            .unwrap();
        assert_eq!(fee, 15 + 25);
        let fee = processing_fee(&r, None, Some(120), Some("USD"), 1_000, "EUR")
            .await
            .unwrap();
        assert_eq!(fee, 100);
        assert_eq!(
            processing_fee(&r, None, None, None, 1_000, "EUR")
                .await
                .unwrap(),
            0
        );
    }

    #[test]
    fn card_currencies() {
        assert!(is_card_currency("eur"));
        assert!(!is_card_currency("BTC"));
        assert!(!is_card_currency("JPY"));
    }
}
