use super::DestinationKind;
use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use std::time::Duration;

pub fn decode_lnurl(s: &str) -> Result<String> {
    let (hrp, data) = bitcoin::bech32::decode(s).map_err(|e| anyhow!("invalid lnurl: {e}"))?;
    if hrp.as_str() != "lnurl" {
        bail!("not an lnurl");
    }
    let url = String::from_utf8(data).context("lnurl is not utf-8")?;
    url::Url::parse(&url).context("lnurl does not hold a url")?;
    Ok(url)
}

fn endpoint(kind: DestinationKind, value: &str, allow_private: bool) -> Result<String> {
    let scheme = if allow_private { "http" } else { "https" };
    match kind {
        DestinationKind::LightningAddress => {
            let (user, domain) = value
                .split_once('@')
                .ok_or_else(|| anyhow!("bad lightning address"))?;
            Ok(format!("{scheme}://{domain}/.well-known/lnurlp/{user}"))
        }
        DestinationKind::Lnurl => decode_lnurl(value),
        _ => bail!("not an lnurl destination"),
    }
}

async fn client_for(url: &str, allow_private: bool) -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none());
    if !allow_private {
        crate::ssrf::require_https(url)?;
        let target = crate::ssrf::resolve_public(url).await?;
        b = b.resolve(&target.host, target.addr);
    }
    Ok(b.build()?)
}

async fn get_json(url: &str, allow_private: bool) -> Result<Value> {
    let client = client_for(url, allow_private).await?;
    let rsp = client.get(url).send().await?;
    if !rsp.status().is_success() {
        bail!("{} returned {}", host_of(url), rsp.status());
    }
    let body = rsp.bytes().await?;
    if body.len() > 64 * 1024 {
        bail!("lnurl response too large");
    }
    let v: Value = serde_json::from_slice(&body)?;
    if v["status"].as_str() == Some("ERROR") {
        bail!(
            "lnurl service said: {}",
            v["reason"].as_str().unwrap_or("error")
        );
    }
    Ok(v)
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_default()
}

pub async fn resolve_invoice(
    kind: DestinationKind,
    value: &str,
    amount_msat: u64,
    allow_private: bool,
) -> Result<String> {
    let first = endpoint(kind, value, allow_private)?;
    let params = get_json(&first, allow_private).await?;
    if params["tag"].as_str() != Some("payRequest") {
        bail!("destination is not an lnurl-pay endpoint");
    }
    let min = params["minSendable"].as_u64().unwrap_or(0);
    let max = params["maxSendable"].as_u64().unwrap_or(u64::MAX);
    if amount_msat < min || amount_msat > max {
        bail!("amount is outside what the destination accepts ({min} to {max} msat)");
    }
    let callback = params["callback"]
        .as_str()
        .ok_or_else(|| anyhow!("lnurl-pay response has no callback"))?;
    let mut url = url::Url::parse(callback)?;
    if !allow_private && url.scheme() != "https" {
        bail!("lnurl callback must be https");
    }
    url.query_pairs_mut()
        .append_pair("amount", &amount_msat.to_string());
    let reply = get_json(url.as_str(), allow_private).await?;
    let pr = reply["pr"]
        .as_str()
        .ok_or_else(|| anyhow!("lnurl callback returned no invoice"))?
        .trim()
        .to_string();
    check_invoice_amount(&pr, amount_msat)?;
    Ok(pr)
}

pub fn check_invoice_amount(invoice: &str, amount_msat: u64) -> Result<()> {
    match crate::providers::lightning::invoice_amount_msat(invoice)? {
        Some(a) if a == amount_msat => Ok(()),
        Some(a) => bail!("invoice is for {a} msat, expected {amount_msat}"),
        None => bail!("invoice must carry an amount"),
    }
}
