use super::{
    AttemptRequest, CreatedAttempt, DisputeState, Provider, ProviderEvent, RefundOutcome,
    RefundRequest,
};
use crate::db::AttemptRow;
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

const SANDBOX_BASE: &str = "https://api-m.sandbox.paypal.com";
const LIVE_BASE: &str = "https://api-m.paypal.com";
const ZERO_DECIMAL: &[&str] = &["JPY", "HUF", "TWD"];

pub struct PayPalProvider {
    client: reqwest::Client,
    base: String,
    client_id: String,
    client_secret: String,
    webhook_id: Option<String>,
}

pub fn to_decimal(amount: u64, currency: &str) -> String {
    if ZERO_DECIMAL.contains(&currency.to_uppercase().as_str()) {
        amount.to_string()
    } else {
        format!("{}.{:02}", amount / 100, amount % 100)
    }
}

pub fn from_decimal(value: &str, currency: &str) -> Option<u64> {
    let zero = ZERO_DECIMAL.contains(&currency.to_uppercase().as_str());
    let (whole, frac) = value.split_once('.').unwrap_or((value, ""));
    let whole: u64 = whole.parse().ok()?;
    if zero {
        return Some(whole);
    }
    let frac = format!("{frac:0<2}");
    Some(whole * 100 + frac.get(..2)?.parse::<u64>().ok()?)
}

impl PayPalProvider {
    pub fn new(config: &Value) -> Result<Self> {
        let field = |k: &str| {
            config
                .get(k)
                .and_then(Value::as_str)
                .map(str::to_string)
                .with_context(|| format!("paypal config needs {k}"))
        };
        let base = match config.get("url").and_then(Value::as_str) {
            Some(u) if !u.is_empty() => u.trim_end_matches('/').to_string(),
            _ => match config.get("mode").and_then(Value::as_str) {
                Some("live") => LIVE_BASE.to_string(),
                _ => SANDBOX_BASE.to_string(),
            },
        };
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            base,
            client_id: field("client_id")?,
            client_secret: field("client_secret")?,
            webhook_id: config
                .get("webhook_id")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
        })
    }

    fn failure(status: reqwest::StatusCode, body: &Value) -> anyhow::Error {
        let msg = body["error_description"]
            .as_str()
            .or_else(|| body["message"].as_str())
            .unwrap_or("request failed");
        anyhow!("paypal {status}: {msg}")
    }

    async fn token(&self) -> Result<String> {
        let rsp = self
            .client
            .post(format!("{}/v1/oauth2/token", self.base))
            .basic_auth(&self.client_id, Some(&self.client_secret))
            .form(&[("grant_type", "client_credentials")])
            .send()
            .await?;
        let status = rsp.status();
        let body: Value = rsp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(Self::failure(status, &body));
        }
        body["access_token"]
            .as_str()
            .map(str::to_string)
            .context("paypal returned no access token")
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
        request_id: Option<&str>,
    ) -> Result<Value> {
        let token = self.token().await?;
        let mut req = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(token)
            .header("Content-Type", "application/json");
        if let Some(id) = request_id {
            req = req.header("PayPal-Request-Id", id);
        }
        req = req.body(body.map(|b| b.to_string()).unwrap_or_default());
        let rsp = req.send().await?;
        let status = rsp.status();
        let body: Value = rsp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            return Err(Self::failure(status, &body));
        }
        Ok(body)
    }

    async fn capture_order(&self, order_id: &str) -> Result<Option<ProviderEvent>> {
        let order = self
            .call(
                reqwest::Method::POST,
                &format!("/v2/checkout/orders/{order_id}/capture"),
                None,
                Some(&format!("capture-{order_id}")),
            )
            .await?;
        Ok(Self::paid_from_order(order_id, &order))
    }

    fn paid_from_order(order_id: &str, order: &Value) -> Option<ProviderEvent> {
        let capture = &order["purchase_units"][0]["payments"]["captures"][0];
        if order["status"].as_str() != Some("COMPLETED")
            || capture["status"].as_str() == Some("DECLINED")
        {
            return None;
        }
        let currency = capture["amount"]["currency_code"].as_str().unwrap_or("USD");
        Some(ProviderEvent::Paid {
            lookup: order_id.to_string(),
            reference: capture["id"].as_str().unwrap_or(order_id).to_string(),
            amount: capture["amount"]["value"]
                .as_str()
                .and_then(|v| from_decimal(v, currency)),
            confirmed: capture["status"].as_str() != Some("PENDING"),
            saved: None,
        })
    }

    async fn verify(&self, headers: &HashMap<String, String>, body: &[u8]) -> Result<Value> {
        let webhook_id = self
            .webhook_id
            .as_deref()
            .context("paypal webhook_id not configured")?;
        let header = |k: &str| {
            headers
                .get(k)
                .cloned()
                .ok_or_else(|| anyhow!("missing {k} header"))
        };
        let time = header("paypal-transmission-time")?;
        let sent = chrono::DateTime::parse_from_rfc3339(&time)
            .map_err(|_| anyhow!("bad transmission time"))?;
        if !super::webhook_check::within_window(sent.timestamp(), chrono::Utc::now().timestamp()) {
            bail!("timestamp outside replay window");
        }
        let event: Value = serde_json::from_slice(body)?;
        let rsp = self
            .call(
                reqwest::Method::POST,
                "/v1/notifications/verify-webhook-signature",
                Some(json!({
                    "auth_algo": header("paypal-auth-algo")?,
                    "cert_url": header("paypal-cert-url")?,
                    "transmission_id": header("paypal-transmission-id")?,
                    "transmission_sig": header("paypal-transmission-sig")?,
                    "transmission_time": time,
                    "webhook_id": webhook_id,
                    "webhook_event": event,
                })),
                None,
            )
            .await?;
        if rsp["verification_status"].as_str() != Some("SUCCESS") {
            bail!("signature mismatch");
        }
        Ok(event)
    }

    async fn order_for_capture(&self, capture_id: &str) -> Result<Option<String>> {
        let capture = self
            .call(
                reqwest::Method::GET,
                &format!("/v2/payments/captures/{capture_id}"),
                None,
                None,
            )
            .await?;
        Ok(capture["supplementary_data"]["related_ids"]["order_id"]
            .as_str()
            .map(str::to_string))
    }

    async fn capture_id_for(&self, order_id: &str) -> Result<String> {
        let order = self
            .call(
                reqwest::Method::GET,
                &format!("/v2/checkout/orders/{order_id}"),
                None,
                None,
            )
            .await?;
        order["purchase_units"][0]["payments"]["captures"][0]["id"]
            .as_str()
            .map(str::to_string)
            .context("order has no capture to refund")
    }
}

#[async_trait]
impl Provider for PayPalProvider {
    fn method(&self) -> &'static str {
        "paypal"
    }

    async fn create(&self, req: AttemptRequest) -> Result<CreatedAttempt> {
        let pid = hex::encode(req.payment_id);
        let mut context = json!({
            "user_action": "PAY_NOW",
            "shipping_preference": "NO_SHIPPING",
        });
        if let Some(u) = &req.success_url {
            context["return_url"] = json!(u);
        }
        if let Some(u) = &req.cancel_url {
            context["cancel_url"] = json!(u);
        }
        let body = json!({
            "intent": "CAPTURE",
            "purchase_units": [{
                "reference_id": &pid[..16],
                "custom_id": pid,
                "description": req.description.chars().take(127).collect::<String>(),
                "amount": {
                    "currency_code": req.currency.to_uppercase(),
                    "value": to_decimal(req.amount, &req.currency),
                },
            }],
            "payment_source": {"paypal": {"experience_context": context}},
        });
        let order = self
            .call(
                reqwest::Method::POST,
                "/v2/checkout/orders",
                Some(body),
                Some(&format!("order-{pid}-{}", req.amount)),
            )
            .await?;
        let id = order["id"].as_str().context("order id")?.to_string();
        let url = order["links"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|l| matches!(l["rel"].as_str(), Some("payer-action") | Some("approve")))
            .and_then(|l| l["href"].as_str())
            .context("paypal returned no approval link")?
            .to_string();
        Ok(CreatedAttempt {
            lookup: id.clone(),
            external_id: Some(id),
            details: json!({"checkout_url": url}),
            expires: None,
            completed: false,
        })
    }

    async fn capture_return(&self, attempt: &AttemptRow) -> Result<Option<ProviderEvent>> {
        self.capture_order(&attempt.lookup).await
    }

    async fn reconcile(&self, attempt: &AttemptRow) -> Result<Option<ProviderEvent>> {
        let order = self
            .call(
                reqwest::Method::GET,
                &format!("/v2/checkout/orders/{}", attempt.lookup),
                None,
                None,
            )
            .await?;
        match order["status"].as_str() {
            Some("APPROVED") => self.capture_order(&attempt.lookup).await,
            Some("COMPLETED") => Ok(Self::paid_from_order(&attempt.lookup, &order)),
            _ => Ok(None),
        }
    }

    async fn handle_webhook(
        &self,
        headers: &HashMap<String, String>,
        body: &[u8],
    ) -> Result<Vec<ProviderEvent>> {
        let event = self.verify(headers, body).await?;
        let resource = &event["resource"];
        let order_ref = || {
            resource["supplementary_data"]["related_ids"]["order_id"]
                .as_str()
                .map(str::to_string)
        };
        match event["event_type"].as_str().unwrap_or_default() {
            "CHECKOUT.ORDER.APPROVED" => {
                let id = resource["id"].as_str().context("order id")?;
                Ok(self.capture_order(id).await?.into_iter().collect())
            }
            "PAYMENT.CAPTURE.COMPLETED" => {
                let Some(order) = order_ref() else {
                    return Ok(vec![]);
                };
                let currency = resource["amount"]["currency_code"]
                    .as_str()
                    .unwrap_or("USD");
                Ok(vec![ProviderEvent::Paid {
                    lookup: order,
                    reference: resource["id"].as_str().unwrap_or_default().to_string(),
                    amount: resource["amount"]["value"]
                        .as_str()
                        .and_then(|v| from_decimal(v, currency)),
                    confirmed: true,
                    saved: None,
                }])
            }
            "PAYMENT.CAPTURE.DENIED" | "PAYMENT.CAPTURE.DECLINED" => Ok(order_ref()
                .map(|lookup| ProviderEvent::Failed {
                    lookup,
                    reason: "payment denied".into(),
                })
                .into_iter()
                .collect()),
            "PAYMENT.CAPTURE.REFUNDED" => {
                let Some(lookup) = order_ref() else {
                    return Ok(vec![]);
                };
                let currency = resource["amount"]["currency_code"]
                    .as_str()
                    .unwrap_or("USD");
                Ok(vec![ProviderEvent::Refunded {
                    lookup,
                    external_id: resource["id"].as_str().context("refund id")?.to_string(),
                    amount: resource["amount"]["value"]
                        .as_str()
                        .and_then(|v| from_decimal(v, currency)),
                }])
            }
            "CUSTOMER.DISPUTE.CREATED"
            | "CUSTOMER.DISPUTE.RESOLVED"
            | "CUSTOMER.DISPUTE.UPDATED" => {
                let Some(capture) =
                    resource["disputed_transactions"][0]["seller_transaction_id"].as_str()
                else {
                    return Ok(vec![]);
                };
                let Some(lookup) = self.order_for_capture(capture).await? else {
                    return Ok(vec![]);
                };
                let state = match resource["status"].as_str() {
                    Some("RESOLVED") => {
                        match resource["dispute_outcome"]["outcome_code"].as_str() {
                            Some("RESOLVED_BUYER_FAVOUR") => DisputeState::Lost,
                            _ => DisputeState::Won,
                        }
                    }
                    _ => DisputeState::Open,
                };
                Ok(vec![ProviderEvent::Dispute {
                    lookup,
                    state,
                    reason: resource["reason"].as_str().map(str::to_string),
                }])
            }
            _ => Ok(vec![]),
        }
    }

    fn event_id(&self, body: &[u8]) -> Option<String> {
        serde_json::from_slice::<Value>(body)
            .ok()?
            .get("id")?
            .as_str()
            .map(str::to_string)
    }

    async fn refund(&self, attempt: &AttemptRow, req: RefundRequest) -> Result<RefundOutcome> {
        let capture = self.capture_id_for(&attempt.lookup).await?;
        let rsp = self
            .call(
                reqwest::Method::POST,
                &format!("/v2/payments/captures/{capture}/refund"),
                Some(json!({"amount": {
                    "value": to_decimal(req.amount, &req.currency),
                    "currency_code": req.currency.to_uppercase(),
                }})),
                Some(&format!("refund-{capture}-{:016x}", rand::random::<u64>())),
            )
            .await?;
        Ok(RefundOutcome {
            external_id: rsp["id"].as_str().map(str::to_string),
        })
    }

    async fn test_connection(&self) -> Result<String> {
        self.token().await?;
        let mode = if self.base == LIVE_BASE {
            "live"
        } else {
            "sandbox"
        };
        Ok(format!("Authenticated with PayPal ({mode})"))
    }
}
