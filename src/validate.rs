use crate::db::TenantSettings;
use crate::model::normalize_origins;
use crate::ssrf;
use anyhow::{Result, bail};
use serde::Deserialize;
use serde_json::Value;

pub const SECRET_KEYS: &[&str] = &[
    "secret",
    "secret_key",
    "webhook_secret",
    "token",
    "api_secret",
    "api_key",
    "client_secret",
    "macaroon",
    "macaroon_path",
    "cert_path",
];

fn str_field<'a>(config: &'a Value, key: &str) -> Option<&'a str> {
    config
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

fn need(config: &Value, key: &str) -> Result<()> {
    if str_field(config, key).is_none() {
        bail!("config needs {key}");
    }
    Ok(())
}

fn forbid(config: &Value, keys: &[&str]) -> Result<()> {
    for k in keys {
        if config.get(*k).is_some() {
            bail!("{k} is not allowed in a customer config");
        }
    }
    Ok(())
}

pub fn supported(method: &str, provider_type: &str) -> bool {
    matches!(
        (method, provider_type),
        ("lightning", "lnd")
            | ("onchain", "lnd")
            | ("stripe", "stripe")
            | ("revolut", "revolut")
            | ("paypal", "paypal")
            | ("fixedfloat", "fixedfloat")
    )
}

pub async fn provider_config(
    trusted: bool,
    allow_mock: bool,
    method: &str,
    provider_type: &str,
    config: &Value,
) -> Result<()> {
    if !config.is_object() {
        bail!("config must be an object");
    }
    if provider_type == "mock" {
        if trusted && allow_mock {
            return Ok(());
        }
        bail!("mock providers are not available");
    }
    if !supported(method, provider_type) {
        bail!("unsupported provider {method}/{provider_type}");
    }
    match (method, provider_type) {
        ("lightning" | "onchain", "lnd") => {
            need(config, "url")?;
            if !trusted {
                forbid(config, &["cert_path", "macaroon_path"])?;
                need(config, "cert")?;
                need(config, "macaroon")?;
                let url = str_field(config, "url").unwrap();
                ssrf::require_https(url)?;
                ssrf::resolve_public(url).await?;
            } else if str_field(config, "cert").is_none()
                && str_field(config, "cert_path").is_none()
            {
                bail!("config needs cert or cert_path");
            }
            if method == "onchain" {
                if let Some(t) = str_field(config, "address_type")
                    && !matches!(t, "p2wkh" | "np2wkh" | "p2tr")
                {
                    bail!("address_type must be p2wkh, np2wkh or p2tr");
                }
                if let Some(c) = config.get("min_confirmations")
                    && !c.as_u64().is_some_and(|c| (1..=100).contains(&c))
                {
                    bail!("min_confirmations must be 1-100");
                }
            }
        }
        ("stripe", _) => {
            need(config, "secret_key")?;
            if !trusted {
                forbid(config, &["url"])?;
            }
        }
        ("revolut", _) => {
            need(config, "token")?;
            need(config, "api_version")?;
            if !trusted {
                forbid(config, &["url"])?;
            }
        }
        ("paypal", _) => {
            need(config, "client_id")?;
            need(config, "client_secret")?;
            if let Some(m) = str_field(config, "mode")
                && !matches!(m, "sandbox" | "live")
            {
                bail!("mode must be sandbox or live");
            }
            if !trusted {
                forbid(config, &["url"])?;
            }
        }
        ("fixedfloat", _) => {
            need(config, "api_key")?;
            need(config, "api_secret")?;
            if !trusted {
                forbid(config, &["url"])?;
            }
            if crate::model::options_from_config("fixedfloat", config).is_empty() {
                bail!("coins must list at least one coin");
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn redacted(config: &Value) -> Value {
    let Some(obj) = config.as_object() else {
        return Value::Null;
    };
    Value::Object(
        obj.iter()
            .map(|(k, v)| {
                if SECRET_KEYS.contains(&k.as_str()) || k == "cert" {
                    (k.clone(), Value::String("set".into()))
                } else {
                    (k.clone(), v.clone())
                }
            })
            .collect(),
    )
}

pub fn changed_config_keys(old: &Value, new: &Value) -> Vec<String> {
    let empty = serde_json::Map::new();
    let (old, new) = (
        old.as_object().unwrap_or(&empty),
        new.as_object().unwrap_or(&empty),
    );
    let mut keys: Vec<String> = old
        .keys()
        .chain(new.keys())
        .filter(|k| old.get(*k) != new.get(*k))
        .map(|k| format!("config.{k}"))
        .collect();
    keys.sort();
    keys.dedup();
    keys
}

pub fn merge_update(old: &Value, new: &Value) -> Value {
    let mut merged = old.as_object().cloned().unwrap_or_default();
    if let Some(n) = new.as_object() {
        for (k, v) in n {
            if v.as_str() == Some("set") && SECRET_KEYS.contains(&k.as_str()) {
                continue;
            }
            merged.insert(k.clone(), v.clone());
        }
    }
    Value::Object(merged)
}

#[derive(Debug, Deserialize, Default)]
pub struct SettingsInput {
    pub webhook_url: Option<String>,
    pub display_name: Option<String>,
    pub logo_url: Option<String>,
    pub accent_color: Option<String>,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    pub default_success_url: Option<String>,
    pub default_cancel_url: Option<String>,
    pub legal_name: Option<String>,
    pub address: Option<String>,
    pub tax_id: Option<String>,
    pub support_email: Option<String>,
}

pub fn valid_url(url: &str) -> bool {
    url::Url::parse(url)
        .map(|u| matches!(u.scheme(), "http" | "https") && u.host().is_some())
        .unwrap_or(false)
}

pub fn valid_color(c: &str) -> bool {
    let c = c.strip_prefix('#').unwrap_or("");
    matches!(c.len(), 3 | 6) && c.chars().all(|ch| ch.is_ascii_hexdigit())
}

pub async fn settings(trusted: bool, input: SettingsInput) -> Result<TenantSettings> {
    for u in [
        &input.webhook_url,
        &input.logo_url,
        &input.default_success_url,
        &input.default_cancel_url,
    ]
    .into_iter()
    .flatten()
    {
        if !valid_url(u) {
            bail!("invalid url {u}");
        }
    }
    if let Some(hook) = &input.webhook_url
        && !trusted
    {
        ssrf::require_https(hook)?;
        ssrf::resolve_public(hook).await?;
    }
    if input
        .accent_color
        .as_deref()
        .is_some_and(|c| !valid_color(c))
    {
        bail!("accent_color must be #rgb or #rrggbb");
    }
    if input.display_name.as_deref().is_some_and(|n| n.len() > 128) {
        bail!("display_name too long");
    }
    let legal_name = seller_field(input.legal_name, 255, "legal_name", true)?;
    let address = seller_field(input.address, 1024, "address", false)?;
    let tax_id = seller_field(input.tax_id, 64, "tax_id", true)?;
    let support_email = seller_field(input.support_email, 255, "support_email", true)?;
    if support_email
        .as_deref()
        .is_some_and(|e| !e.contains('@') || e.contains(char::is_whitespace))
    {
        bail!("support_email must be an email address");
    }
    let origins = normalize_origins(&input.allowed_origins.join(" "))?;
    for u in [&input.default_success_url, &input.default_cancel_url]
        .into_iter()
        .flatten()
    {
        if !crate::model::url_in_origins(u, &origins) {
            bail!("{u} is not on one of the allowed_origins");
        }
    }
    Ok(TenantSettings {
        webhook_url: input.webhook_url,
        display_name: input.display_name,
        logo_url: input.logo_url,
        accent_color: input.accent_color,
        allowed_origins: (!origins.is_empty()).then(|| origins.join(" ")),
        default_success_url: input.default_success_url,
        default_cancel_url: input.default_cancel_url,
        legal_name,
        address,
        tax_id,
        support_email,
    })
}

fn seller_field(
    v: Option<String>,
    max: usize,
    name: &str,
    single_line: bool,
) -> Result<Option<String>> {
    let Some(v) = v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if v.chars().count() > max {
        bail!("{name} too long");
    }
    if v.chars()
        .any(|c| c.is_control() && !(!single_line && c == '\n'))
    {
        bail!("{name} must not contain control characters");
    }
    Ok(Some(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const CERT: &str = "-----BEGIN CERTIFICATE-----\nx\n-----END CERTIFICATE-----";

    #[tokio::test]
    async fn customers_cannot_use_paths_mocks_or_internal_hosts() {
        let ok = json!({"url": "https://1.1.1.1:10009", "cert": CERT, "macaroon": "0a"});
        assert!(
            provider_config(false, false, "lightning", "lnd", &ok)
                .await
                .is_ok()
        );
        let mut with_path = ok.clone();
        with_path["macaroon_path"] = json!("/etc/passwd");
        assert!(
            provider_config(false, false, "lightning", "lnd", &with_path)
                .await
                .is_err()
        );
        for url in [
            "https://10.245.31.103:10009",
            "https://localhost:10009",
            "http://1.1.1.1:10009",
            "https://127.0.0.1",
        ] {
            let cfg = json!({"url": url, "cert": CERT, "macaroon": "0a"});
            assert!(
                provider_config(false, false, "lightning", "lnd", &cfg)
                    .await
                    .is_err(),
                "{url}"
            );
        }
        assert!(
            provider_config(false, true, "stripe", "mock", &json!({}))
                .await
                .is_err()
        );
        assert!(
            provider_config(true, true, "stripe", "mock", &json!({}))
                .await
                .is_ok()
        );
        assert!(
            provider_config(
                false,
                false,
                "stripe",
                "stripe",
                &json!({"secret_key": "sk", "url": "http://10.0.0.1"})
            )
            .await
            .is_err()
        );
        assert!(
            provider_config(
                false,
                false,
                "stripe",
                "stripe",
                &json!({"secret_key": "sk"})
            )
            .await
            .is_ok()
        );
        assert!(
            provider_config(false, false, "revolut", "revolut", &json!({"token": "t"}))
                .await
                .is_err()
        );
        assert!(
            provider_config(
                false,
                false,
                "fixedfloat",
                "fixedfloat",
                &json!({"api_key": "k", "api_secret": "s", "coins": []})
            )
            .await
            .is_err()
        );
        assert!(
            provider_config(true, false, "bogus", "x", &json!({}))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn operators_may_still_use_files_and_internal_hosts() {
        let cfg = json!({"url": "https://lnd.bitcoin:10009", "cert_path": "/lnd/tls.cert", "macaroon_path": "/lnd/admin.macaroon"});
        assert!(
            provider_config(true, false, "lightning", "lnd", &cfg)
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn onchain_options_are_checked() {
        let base = json!({"url": "https://1.1.1.1:10009", "cert": CERT, "macaroon": "0a"});
        let mut bad = base.clone();
        bad["address_type"] = json!("p2sh");
        assert!(
            provider_config(false, false, "onchain", "lnd", &bad)
                .await
                .is_err()
        );
        let mut bad = base.clone();
        bad["min_confirmations"] = json!(0);
        assert!(
            provider_config(false, false, "onchain", "lnd", &bad)
                .await
                .is_err()
        );
    }

    #[test]
    fn secrets_are_redacted_and_untouched_on_update() {
        let cfg = json!({"url": "https://x", "macaroon": "0a0b", "secret_key": "sk_live", "cert": CERT, "min_confirmations": 2});
        let r = redacted(&cfg);
        assert_eq!(r["macaroon"], "set");
        assert_eq!(r["secret_key"], "set");
        assert_eq!(r["cert"], "set");
        assert_eq!(r["url"], "https://x");
        assert!(!r.to_string().contains("sk_live"));
        let merged = merge_update(
            &cfg,
            &json!({"url": "https://y", "macaroon": "set", "secret_key": "sk_new"}),
        );
        assert_eq!(merged["macaroon"], "0a0b");
        assert_eq!(merged["secret_key"], "sk_new");
        assert_eq!(merged["url"], "https://y");
        assert_eq!(merged["min_confirmations"], 2);
    }
}

pub fn sanitize_test_error(error: &str, config: Option<&Value>) -> String {
    let mut text = crate::logsafe::clean(error);
    if let Some(obj) = config.and_then(Value::as_object) {
        for (k, v) in obj {
            if let Some(secret) = v.as_str()
                && secret.len() >= 4
                && (SECRET_KEYS.contains(&k.as_str()) || k == "cert")
            {
                text = text.replace(secret, "[redacted]");
            }
        }
    }
    text.chars().take(200).collect()
}
