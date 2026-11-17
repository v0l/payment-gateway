use super::{AttemptRequest, CreatedAttempt, Provider};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;
use std::time::Duration;

const DEFAULT_URL: &str = "https://ff.io/api/v2";

pub struct FixedFloatProvider {
    client: reqwest::Client,
    url: String,
    api_key: String,
    api_secret: String,
    refcode: Option<String>,
    afftax: Option<f64>,
    coins: Vec<String>,
}

#[derive(Deserialize)]
struct Envelope {
    code: i64,
    #[serde(default)]
    msg: String,
    data: Option<Value>,
}

#[derive(Deserialize)]
struct CreatedOrder {
    id: String,
    token: String,
    time: OrderTime,
    from: OrderLeg,
}

#[derive(Deserialize)]
struct OrderTime {
    expiration: i64,
}

#[derive(Deserialize)]
struct OrderLeg {
    address: String,
    amount: String,
}

impl FixedFloatProvider {
    pub fn new(config: &Value) -> Result<Self> {
        let field = |k: &str| {
            config
                .get(k)
                .and_then(Value::as_str)
                .map(str::to_string)
                .with_context(|| format!("fixedfloat config needs {k}"))
        };
        let coins = super::super::model::options_from_config("fixedfloat", config);
        if coins.is_empty() {
            bail!("fixedfloat config needs a non-empty coins list");
        }
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            url: config
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_URL)
                .trim_end_matches('/')
                .to_string(),
            api_key: field("api_key")?,
            api_secret: field("api_secret")?,
            refcode: config
                .get("refcode")
                .and_then(Value::as_str)
                .map(str::to_string),
            afftax: config.get("afftax").and_then(Value::as_f64),
            coins,
        })
    }

    fn sign(&self, body: &str) -> Result<String> {
        let mut mac = Hmac::<Sha256>::new_from_slice(self.api_secret.as_bytes())?;
        mac.update(body.as_bytes());
        Ok(hex::encode(mac.finalize().into_bytes()))
    }

    fn params(&self, coin: &str, receive_sats: u64, address: &str) -> Value {
        let mut params = json!({
            "type": "float",
            "fromCcy": coin,
            "toCcy": "BTC",
            "direction": "to",
            "amount": receive_sats as f64 / 100_000_000.0,
            "toAddress": address,
        });
        if let Some(refcode) = &self.refcode {
            params["refcode"] = json!(refcode);
            if let Some(tax) = self.afftax {
                params["afftax"] = json!(tax);
            }
        }
        params
    }

    async fn call(&self, method: &str, params: Value) -> Result<CreatedOrder> {
        let body = serde_json::to_string(&params)?;
        let res = self
            .client
            .post(format!("{}/{method}", self.url))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json; charset=UTF-8")
            .header("X-API-KEY", &self.api_key)
            .header("X-API-SIGN", self.sign(&body)?)
            .body(body)
            .send()
            .await?
            .error_for_status()?;
        let envelope: Envelope = res.json().await.context("parsing FixedFloat response")?;
        if envelope.code != 0 {
            bail!("FixedFloat {}: {}", envelope.code, envelope.msg);
        }
        let data = envelope.data.context("FixedFloat response had no data")?;
        Ok(serde_json::from_value(data)?)
    }
}

#[async_trait]
impl Provider for FixedFloatProvider {
    fn method(&self) -> &'static str {
        "fixedfloat"
    }

    fn options(&self) -> Vec<String> {
        self.coins.clone()
    }

    async fn create(&self, req: AttemptRequest) -> Result<CreatedAttempt> {
        let coin = req
            .option
            .as_ref()
            .map(|c| c.to_uppercase())
            .filter(|c| self.coins.contains(c))
            .ok_or_else(|| anyhow!("choose one of {:?}", self.coins))?;
        let receiver = req
            .receive
            .as_ref()
            .context("fixedfloat needs an on-chain provider to receive into")?;
        let address = receiver.receive_address(req.amount).await?;
        let order = self
            .call(
                "create",
                self.params(&coin, req.amount.div_ceil(1000), &address),
            )
            .await?;
        let expires = DateTime::<Utc>::from_timestamp(order.time.expiration, 0)
            .context("FixedFloat order has an invalid expiration")?;
        Ok(CreatedAttempt {
            lookup: address.clone(),
            external_id: Some(order.id.clone()),
            details: json!({
                "coin": coin,
                "order_id": order.id,
                "order_token": order.token,
                "deposit_address": order.from.address,
                "deposit_amount": order.from.amount,
                "receive_address": address,
            }),
            expires: Some(expires),
            completed: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(url: String) -> FixedFloatProvider {
        FixedFloatProvider::new(&json!({
            "api_key": "key", "api_secret": "secret", "url": url, "coins": ["XMR", "ltc"]
        }))
        .unwrap()
    }

    #[test]
    fn coins_are_required_and_uppercased() {
        assert_eq!(provider("http://x".into()).options(), vec!["XMR", "LTC"]);
        assert!(FixedFloatProvider::new(&json!({"api_key": "k", "api_secret": "s"})).is_err());
    }

    #[test]
    fn params_ask_for_btc_out_to_our_address() {
        let p = provider("http://x".into());
        let params = p.params("XMR", 150_000, "bc1qdest");
        assert_eq!(params["fromCcy"], "XMR");
        assert_eq!(params["toCcy"], "BTC");
        assert_eq!(params["direction"], "to");
        assert_eq!(params["amount"], 0.0015);
        assert_eq!(params["toAddress"], "bc1qdest");
        assert!(params.get("refcode").is_none());
    }

    #[test]
    fn signature_is_hmac_sha256_hex() {
        let p = provider("http://x".into());
        let sig = p.sign("{}").unwrap();
        assert_eq!(sig.len(), 64);
        let mut mac = Hmac::<Sha256>::new_from_slice(b"secret").unwrap();
        mac.update(b"{}");
        assert_eq!(sig, hex::encode(mac.finalize().into_bytes()));
    }

    #[tokio::test]
    async fn create_signs_and_parses_against_a_server() {
        use wiremock::matchers::{header_exists, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/create"))
            .and(header_exists("X-API-SIGN"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "code": 0, "msg": "OK",
                "data": {
                    "id": "ABC", "token": "tok",
                    "time": {"expiration": 1_900_000_000},
                    "from": {"address": "4Axmr", "amount": "0.0123"}
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        let p = provider(server.uri());
        let order = p
            .call("create", p.params("XMR", 1000, "bc1q"))
            .await
            .unwrap();
        assert_eq!(order.id, "ABC");
        assert_eq!(order.from.address, "4Axmr");
    }

    #[tokio::test]
    async fn api_error_code_is_surfaced() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(
                    json!({"code": 301, "msg": "Amount less than limit", "data": null}),
                ),
            )
            .mount(&server)
            .await;
        let p = provider(server.uri());
        let err = p.call("create", json!({})).await.err().unwrap();
        assert!(err.to_string().contains("Amount less than limit"));
    }
}
