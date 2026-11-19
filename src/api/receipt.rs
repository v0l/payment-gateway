use super::AppState;
use super::public::{load, page_headers};
use crate::db::{self, PaymentRow, Tenant};
use crate::invoice::Breakdown;
use crate::rates::{self, Unit};
use axum::Router;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/receipt/{id}", get(receipt_page))
        .route("/receipt/{id}/", get(receipt_page))
}

pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

pub fn money(amount: u64, currency: &str) -> String {
    match rates::unit(currency) {
        Ok(Unit::Btc) => format!("{} sats", group(amount.div_ceil(1000))),
        Ok(Unit::Fiat { scale, .. }) => {
            let decimals = scale.log10().round() as u32;
            let div = 10u64.pow(decimals);
            format!(
                "{}.{:0width$} {}",
                group(amount / div),
                amount % div,
                currency,
                width = decimals as usize
            )
        }
        Err(_) => format!("{amount} {currency}"),
    }
}

fn group(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn percent(bps: u32) -> String {
    if bps.is_multiple_of(100) {
        format!("{}%", bps / 100)
    } else {
        format!("{}.{:02}%", bps / 100, bps % 100)
    }
}

fn method_name(method: &str) -> &str {
    match method {
        "lightning" => "Lightning",
        "onchain" => "Bitcoin on-chain",
        "stripe" => "Card (Stripe)",
        "revolut" => "Card (Revolut)",
        "paypal" => "PayPal",
        "fixedfloat" => "Crypto swap",
        other => other,
    }
}

async fn receipt_page(
    State(s): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, super::ApiError> {
    let (p, tenant) = load(&s, &id).await?;
    if !matches!(
        p.status.as_str(),
        "paid" | "refunded" | "partially_refunded"
    ) {
        return Err(super::ApiError::not_found());
    }
    let attempts = db::attempts_for_payment(s.pool(), &p.pid()).await?;
    let method = attempts
        .iter()
        .find(|a| a.status == "paid")
        .map(|a| method_name(&a.method).to_string());
    let html = render(&p, &tenant, method.as_deref());
    let mut headers = page_headers("'none'");
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok((headers, html).into_response())
}

fn row(label: &str, value: &str) -> String {
    format!("<dt>{}</dt><dd>{}</dd>", esc(label), esc(value))
}

fn lines_table(b: &Breakdown, currency: &str) -> String {
    let mut out = String::from(
        "<div class=\"scroll\"><table class=\"lines\"><thead><tr><th>Item</th><th class=\"num\">Qty</th><th class=\"num\">Unit</th><th class=\"num\">Total</th></tr></thead><tbody>",
    );
    for l in &b.lines {
        let mut name = esc(&l.name);
        if let Some(d) = &l.description {
            name.push_str(&format!("<span class=\"sub\">{}</span>", esc(d)));
        }
        if let Some(sku) = &l.sku {
            name.push_str(&format!("<span class=\"sub\">SKU {}</span>", esc(sku)));
        }
        if l.discount > 0 {
            name.push_str(&format!(
                "<span class=\"sub\">Discount -{}</span>",
                esc(&money(l.discount, currency))
            ));
        }
        if l.tax > 0 {
            let tax = match l.tax_rate_bps {
                Some(bps) => format!("Tax {} ({})", money(l.tax, currency), percent(bps)),
                None => format!("Tax {}", money(l.tax, currency)),
            };
            name.push_str(&format!("<span class=\"sub\">{}</span>", esc(&tax)));
        }
        out.push_str(&format!(
            "<tr><td>{}</td><td class=\"num money\">{}</td><td class=\"num money\">{}</td><td class=\"num money\">{}</td></tr>",
            name,
            l.quantity,
            esc(&money(l.unit_amount, currency)),
            esc(&money(l.net, currency)),
        ));
    }
    out.push_str("</tbody><tfoot>");
    let foot = |label: String, value: String| {
        format!(
            "<tr><td colspan=\"3\" class=\"num\">{}</td><td class=\"num money\">{}</td></tr>",
            esc(&label),
            esc(&value)
        )
    };
    out.push_str(&foot("Subtotal".into(), money(b.subtotal, currency)));
    if let Some(d) = &b.discount {
        let mut label = "Discount".to_string();
        if let Some(bps) = d.percent_bps {
            label.push_str(&format!(" ({})", percent(bps)));
        }
        if let Some(code) = &d.code {
            label.push_str(&format!(" {code}"));
        }
        out.push_str(&foot(label, format!("-{}", money(d.amount, currency))));
    }
    out.push_str(&foot("Tax".into(), money(b.tax, currency)));
    out.push_str("</tfoot></table></div>");
    out
}

pub fn render(p: &PaymentRow, tenant: &Tenant, method: Option<&str>) -> String {
    let currency = &p.currency;
    let seller = tenant
        .legal_name
        .clone()
        .or_else(|| tenant.display_name.clone())
        .unwrap_or_else(|| tenant.name.clone());
    let mut seller_html = format!("<strong>{}</strong>", esc(&seller));
    if let Some(a) = &tenant.address {
        seller_html.push_str(&format!("<div class=\"addr\">{}</div>", esc(a)));
    }
    if let Some(t) = &tenant.tax_id {
        seller_html.push_str(&format!("<div>Tax ID: {}</div>", esc(t)));
    }
    if let Some(e) = &tenant.support_email {
        seller_html.push_str(&format!("<div>{}</div>", esc(e)));
    }

    let mut meta = String::new();
    meta.push_str(&row("Receipt for", &p.reference));
    meta.push_str(&format!(
        "<dt>Payment ID</dt><dd class=\"mono\">{}</dd>",
        hex::encode(&p.id)
    ));
    if let Some(at) = p.paid_at {
        meta.push_str(&row("Date", &at.format("%Y-%m-%d %H:%M UTC").to_string()));
    }
    if let Some(m) = method {
        meta.push_str(&row("Payment method", m));
    }
    if let Some(e) = &p.customer_email {
        meta.push_str(&row("Billed to", e));
    }
    if let Some(d) = &p.description {
        meta.push_str(&row("Description", d));
    }
    if p.status != "paid" {
        meta.push_str(&row("Status", &p.status.replace('_', " ")));
    }

    let body = match p.breakdown() {
        Some(b) => {
            let mut t = lines_table(&b, currency);
            t.push_str(&format!(
                "<table><tbody><tr class=\"total\"><td>Total paid</td><td class=\"num money\">{}</td></tr></tbody></table>",
                esc(&money(p.amount, currency))
            ));
            t
        }
        None => {
            let mut t = String::from("<table><tbody>");
            if p.tax > 0 {
                t.push_str(&format!(
                    "<tr><td>Includes tax</td><td class=\"num money\">{}</td></tr>",
                    esc(&money(p.tax, currency))
                ));
            }
            t.push_str(&format!(
                "<tr class=\"total\"><td>Total paid</td><td class=\"num money\">{}</td></tr></tbody></table>",
                esc(&money(p.amount, currency))
            ));
            t
        }
    };

    let assets = crate::web::entry("receipt");
    let head_links: String = assets
        .styles
        .iter()
        .map(|c| format!("<link rel=\"stylesheet\" href=\"{c}\">"))
        .collect();
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><meta name=\"robots\" content=\"noindex\"><title>Receipt {}</title>{}</head><body><main><div class=\"receipt\"><header><div><h1>Receipt</h1></div><div>{}</div></header><dl class=\"meta\">{}</dl>{}<div class=\"actions\"><button type=\"button\" id=\"print\">Print</button></div></div></main><script type=\"module\" src=\"{}\"></script></body></html>",
        esc(&p.reference),
        head_links,
        seller_html,
        meta,
        body,
        assets.script
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_uses_currency_decimals() {
        assert_eq!(money(123456, "EUR"), "1,234.56 EUR");
        assert_eq!(money(5, "USD"), "0.05 USD");
        assert_eq!(money(2_500_000, "BTC"), "2,500 sats");
    }

    #[test]
    fn escapes_html() {
        assert_eq!(
            esc("<a href=\"x\">&'"),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;"
        );
    }
}
