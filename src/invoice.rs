use crate::rates::MAX_AMOUNT;
use serde::{Deserialize, Serialize};

pub const MAX_LINES: usize = 100;
pub const MAX_NAME: usize = 127;
pub const MAX_LINE_DESCRIPTION: usize = 255;
pub const MAX_SKU: usize = 64;
pub const MAX_CODE: usize = 64;
pub const MAX_QUANTITY: u64 = 1_000_000;
const BPS: u128 = 10_000;

#[derive(Debug, Clone, Deserialize)]
pub struct LineItemInput {
    pub name: String,
    pub description: Option<String>,
    pub quantity: Option<u64>,
    pub unit_amount: u64,
    pub tax_rate: Option<f64>,
    pub tax_amount: Option<u64>,
    pub sku: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DiscountInput {
    pub amount: Option<u64>,
    pub percent: Option<f64>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Line {
    pub name: String,
    pub description: Option<String>,
    pub sku: Option<String>,
    pub quantity: u64,
    pub unit_amount: u64,
    pub net: u64,
    pub discount: u64,
    pub tax_rate_bps: Option<u32>,
    pub tax: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppliedDiscount {
    pub amount: u64,
    pub percent_bps: Option<u32>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Breakdown {
    pub lines: Vec<Line>,
    pub subtotal: u64,
    pub discount: Option<AppliedDiscount>,
    pub tax: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LineView {
    pub name: String,
    pub description: Option<String>,
    pub sku: Option<String>,
    pub quantity: u64,
    pub unit_amount: u64,
    pub amount: u64,
    pub discount: u64,
    pub tax_rate: Option<f64>,
    pub tax: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscountView {
    pub amount: u64,
    pub percent: Option<f64>,
    pub code: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChargeLine {
    pub name: String,
    pub description: Option<String>,
    pub quantity: u64,
    pub unit_amount: u64,
}

fn bps_of_percent(p: f64, what: &str) -> Result<u32, String> {
    if !p.is_finite() || !(0.0..=100.0).contains(&p) {
        return Err(format!("{what} must be between 0 and 100"));
    }
    let scaled = p * 100.0;
    let rounded = scaled.round();
    if (scaled - rounded).abs() > 1e-6 {
        return Err(format!("{what} supports at most 2 decimal places"));
    }
    Ok(rounded as u32)
}

fn round_div(n: u128, d: u128) -> u128 {
    (n + d / 2) / d
}

fn text_ok(v: &str, max: usize, what: &str) -> Result<(), String> {
    if v.chars().count() > max {
        return Err(format!("{what} too long"));
    }
    if v.chars().any(char::is_control) {
        return Err(format!("{what} must not contain control characters"));
    }
    Ok(())
}

fn clean(v: &Option<String>, max: usize, what: &str) -> Result<Option<String>, String> {
    match v.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => {
            text_ok(s, max, what)?;
            Ok(Some(s.to_string()))
        }
    }
}

fn allocate_discount(nets: &[u64], subtotal: u64, discount: u64) -> Vec<u64> {
    let mut shares: Vec<u64> = Vec::with_capacity(nets.len());
    let mut remainders: Vec<(u128, usize)> = Vec::with_capacity(nets.len());
    for (i, &n) in nets.iter().enumerate() {
        let scaled = discount as u128 * n as u128;
        shares.push((scaled / subtotal as u128) as u64);
        remainders.push((scaled % subtotal as u128, i));
    }
    let given: u64 = shares.iter().sum();
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    for (_, i) in remainders.into_iter().take((discount - given) as usize) {
        shares[i] += 1;
    }
    shares
}

pub fn compute(
    items: &[LineItemInput],
    discount: Option<&DiscountInput>,
) -> Result<Breakdown, String> {
    if items.is_empty() {
        return Err("line_items must not be empty".into());
    }
    if items.len() > MAX_LINES {
        return Err(format!("line_items cannot exceed {MAX_LINES} lines"));
    }
    struct Raw {
        name: String,
        description: Option<String>,
        sku: Option<String>,
        quantity: u64,
        unit_amount: u64,
        net: u64,
        rate_bps: Option<u32>,
        fixed_tax: Option<u64>,
    }
    let mut raws = Vec::with_capacity(items.len());
    let mut subtotal: u128 = 0;
    for it in items {
        let name = it.name.trim();
        if name.is_empty() {
            return Err("line item name is required".into());
        }
        text_ok(name, MAX_NAME, "line item name")?;
        let quantity = it.quantity.unwrap_or(1);
        if quantity == 0 || quantity > MAX_QUANTITY {
            return Err(format!("line item quantity must be 1-{MAX_QUANTITY}"));
        }
        if it.tax_rate.is_some() && it.tax_amount.is_some() {
            return Err("a line item takes tax_rate or tax_amount, not both".into());
        }
        let rate_bps = it
            .tax_rate
            .map(|r| bps_of_percent(r, "tax_rate"))
            .transpose()?;
        let net = quantity as u128 * it.unit_amount as u128;
        if net > MAX_AMOUNT as u128 {
            return Err(format!("line item total cannot exceed {MAX_AMOUNT}"));
        }
        subtotal += net;
        raws.push(Raw {
            name: name.to_string(),
            description: clean(
                &it.description,
                MAX_LINE_DESCRIPTION,
                "line item description",
            )?,
            sku: clean(&it.sku, MAX_SKU, "line item sku")?,
            quantity,
            unit_amount: it.unit_amount,
            net: net as u64,
            rate_bps,
            fixed_tax: it.tax_amount,
        });
    }
    if subtotal == 0 {
        return Err("line items must have a positive subtotal".into());
    }
    if subtotal > MAX_AMOUNT as u128 {
        return Err(format!("amount cannot exceed {MAX_AMOUNT}"));
    }
    let subtotal = subtotal as u64;

    let applied = match discount {
        None => None,
        Some(d) => {
            let code = clean(&d.code, MAX_CODE, "discount code")?;
            let (amount, percent_bps) = match (d.amount, d.percent) {
                (Some(_), Some(_)) | (None, None) => {
                    return Err("discount takes exactly one of amount or percent".into());
                }
                (Some(a), None) => (a, None),
                (None, Some(p)) => {
                    let bps = bps_of_percent(p, "discount percent")?;
                    (
                        round_div(subtotal as u128 * bps as u128, BPS) as u64,
                        Some(bps),
                    )
                }
            };
            if amount > subtotal {
                return Err("discount cannot exceed the subtotal".into());
            }
            Some(AppliedDiscount {
                amount,
                percent_bps,
                code,
            })
        }
    };
    let discount_amount = applied.as_ref().map_or(0, |d| d.amount);
    let nets: Vec<u64> = raws.iter().map(|r| r.net).collect();
    let shares = allocate_discount(&nets, subtotal, discount_amount);

    let mut lines = Vec::with_capacity(raws.len());
    let mut tax_total: u128 = 0;
    for (r, share) in raws.into_iter().zip(shares) {
        let taxable = (r.net - share) as u128;
        let tax = match (r.rate_bps, r.fixed_tax) {
            (Some(bps), _) => round_div(taxable * bps as u128, BPS) as u64,
            (None, Some(t)) => t,
            (None, None) => 0,
        };
        tax_total += tax as u128;
        lines.push(Line {
            name: r.name,
            description: r.description,
            sku: r.sku,
            quantity: r.quantity,
            unit_amount: r.unit_amount,
            net: r.net,
            discount: share,
            tax_rate_bps: r.rate_bps,
            tax,
            total: r.net - share + tax,
        });
    }
    let total = (subtotal - discount_amount) as u128 + tax_total;
    if total == 0 {
        return Err("total must be positive".into());
    }
    if total > MAX_AMOUNT as u128 {
        return Err(format!("amount cannot exceed {MAX_AMOUNT}"));
    }
    Ok(Breakdown {
        lines,
        subtotal,
        discount: applied,
        tax: tax_total as u64,
        total: total as u64,
    })
}

impl Line {
    pub fn view(&self) -> LineView {
        LineView {
            name: self.name.clone(),
            description: self.description.clone(),
            sku: self.sku.clone(),
            quantity: self.quantity,
            unit_amount: self.unit_amount,
            amount: self.net,
            discount: self.discount,
            tax_rate: self.tax_rate_bps.map(|b| b as f64 / 100.0),
            tax: self.tax,
            total: self.total,
        }
    }
}

impl AppliedDiscount {
    pub fn view(&self) -> DiscountView {
        DiscountView {
            amount: self.amount,
            percent: self.percent_bps.map(|b| b as f64 / 100.0),
            code: self.code.clone(),
        }
    }
}

impl Breakdown {
    pub fn charge_lines(&self, fee: u64) -> Vec<ChargeLine> {
        let mut out = vec![];
        for l in self.lines.iter().filter(|l| l.total > 0) {
            if l.total % l.quantity == 0 {
                out.push(ChargeLine {
                    name: l.name.clone(),
                    description: l.description.clone(),
                    quantity: l.quantity,
                    unit_amount: l.total / l.quantity,
                });
            } else {
                out.push(ChargeLine {
                    name: format!("{} x{}", l.name, l.quantity),
                    description: l.description.clone(),
                    quantity: 1,
                    unit_amount: l.total,
                });
            }
        }
        if fee > 0 {
            out.push(ChargeLine {
                name: "Processing fee".into(),
                description: None,
                quantity: 1,
                unit_amount: fee,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(unit: u64, qty: u64, rate: Option<f64>) -> LineItemInput {
        LineItemInput {
            name: "Thing".into(),
            description: None,
            quantity: Some(qty),
            unit_amount: unit,
            tax_rate: rate,
            tax_amount: None,
            sku: None,
        }
    }

    fn pct(p: f64) -> DiscountInput {
        DiscountInput {
            amount: None,
            percent: Some(p),
            code: None,
        }
    }

    #[test]
    fn tax_rounds_half_up_per_line() {
        let b = compute(&[item(1050, 1, Some(5.0))], None).unwrap();
        assert_eq!(b.tax, 53);
        assert_eq!(b.total, 1103);
        let b = compute(&[item(1049, 1, Some(5.0))], None).unwrap();
        assert_eq!(b.tax, 52);
        let b = compute(&[item(333, 3, Some(8.25))], None).unwrap();
        assert_eq!(b.subtotal, 999);
        assert_eq!(b.tax, 82);
        assert_eq!(b.total, 1081);
    }

    #[test]
    fn tax_is_rounded_per_line_not_on_the_sum() {
        let b = compute(&[item(10, 1, Some(5.0)), item(10, 1, Some(5.0))], None).unwrap();
        assert_eq!(b.tax, 2);
        assert_eq!(b.total, 22);
    }

    #[test]
    fn percent_discount_rounds_half_up_and_splits_exactly() {
        let items = [item(100, 1, None), item(100, 1, None), item(100, 1, None)];
        let b = compute(&items, Some(&pct(10.0))).unwrap();
        assert_eq!(b.discount.as_ref().unwrap().amount, 30);
        assert_eq!(b.lines.iter().map(|l| l.discount).sum::<u64>(), 30);
        let b = compute(&[item(5, 1, None)], Some(&pct(10.0))).unwrap();
        assert_eq!(b.discount.unwrap().amount, 1);
    }

    #[test]
    fn discount_remainder_goes_to_the_largest_fraction_then_first_line() {
        let items = [item(1, 1, None), item(1, 1, None), item(1, 1, None)];
        let d = DiscountInput {
            amount: Some(1),
            percent: None,
            code: None,
        };
        let b = compute(&items, Some(&d)).unwrap();
        let shares: Vec<u64> = b.lines.iter().map(|l| l.discount).collect();
        assert_eq!(shares, vec![1, 0, 0]);
    }

    #[test]
    fn tax_is_charged_on_the_discounted_line() {
        let b = compute(&[item(1000, 1, Some(20.0))], Some(&pct(50.0))).unwrap();
        assert_eq!(b.subtotal, 1000);
        assert_eq!(b.discount.as_ref().unwrap().amount, 500);
        assert_eq!(b.tax, 100);
        assert_eq!(b.total, 600);
    }

    #[test]
    fn fixed_tax_amount_is_taken_as_given() {
        let mut i = item(1000, 2, None);
        i.tax_amount = Some(123);
        let b = compute(&[i], None).unwrap();
        assert_eq!(b.tax, 123);
        assert_eq!(b.total, 2123);
    }

    #[test]
    fn invalid_inputs_are_rejected() {
        assert!(compute(&[], None).is_err());
        let many: Vec<_> = (0..101).map(|_| item(1, 1, None)).collect();
        assert!(compute(&many, None).is_err());
        let hundred: Vec<_> = (0..100).map(|_| item(1, 1, None)).collect();
        assert!(compute(&hundred, None).is_ok());
        let mut long = item(1, 1, None);
        long.name = "x".repeat(128);
        assert!(compute(&[long], None).is_err());
        let mut both = item(1, 1, Some(5.0));
        both.tax_amount = Some(1);
        assert!(compute(&[both], None).is_err());
        assert!(compute(&[item(1, 0, None)], None).is_err());
        assert!(compute(&[item(0, 1, None)], None).is_err());
        assert!(compute(&[item(1, 1, Some(100.5))], None).is_err());
        assert!(compute(&[item(1, 1, Some(5.005))], None).is_err());
        assert!(compute(&[item(100, 1, None)], Some(&pct(101.0))).is_err());
        let over = DiscountInput {
            amount: Some(101),
            percent: None,
            code: None,
        };
        assert!(compute(&[item(100, 1, None)], Some(&over)).is_err());
        let both = DiscountInput {
            amount: Some(1),
            percent: Some(1.0),
            code: None,
        };
        assert!(compute(&[item(100, 1, None)], Some(&both)).is_err());
        assert!(compute(&[item(100, 1, None)], Some(&pct(100.0))).is_err());
        assert!(compute(&[item(u64::MAX, 2, None)], None).is_err());
    }

    #[test]
    fn charge_lines_sum_to_the_total_plus_fee() {
        let items = [item(333, 3, Some(8.25)), item(1000, 7, None)];
        let b = compute(&items, Some(&pct(13.0))).unwrap();
        let lines = b.charge_lines(17);
        let sum: u64 = lines.iter().map(|l| l.quantity * l.unit_amount).sum();
        assert_eq!(sum, b.total + 17);
    }
}
