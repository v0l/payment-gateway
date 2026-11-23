use super::{
    AttemptRequest, CreatedAttempt, DisputeState, Provider, ProviderEvent, RefundOutcome,
    RefundRequest, SavedMethodInfo,
};
use crate::db::{AttemptRow, SavedMethodRow};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use payments_rs::currency::{Currency, CurrencyAmount};
use payments_rs::fiat::{
    LineItem, RevolutApi, RevolutConfig, RevolutCustomer, RevolutOrder, RevolutOrderState,
    RevolutSavedPaymentMethodType,
};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

const CHARGE_POLLS: usize = 20;
const CHARGE_POLL_INTERVAL: Duration = Duration::from_millis(500);

const WEBHOOK_EVENTS: &[&str] = &[
    "ORDER_COMPLETED",
    "ORDER_CANCELLED",
    "ORDER_FAILED",
    "ORDER_PAYMENT_DECLINED",
    "ORDER_PAYMENT_FAILED",
    "DISPUTE_ACTION_REQUIRED",
    "DISPUTE_UNDER_REVIEW",
    "DISPUTE_WON",
    "DISPUTE_LOST",
];

pub struct RevolutProvider {
    api: RevolutApi,
    http: reqwest::Client,
    base: String,
    token: String,
    api_version: String,
    webhook_secret: Option<String>,
}

impl RevolutProvider {
    pub fn new(config: &Value) -> Result<Self> {
        let field = |k: &str| {
            config
                .get(k)
                .and_then(Value::as_str)
                .map(str::to_string)
                .with_context(|| format!("revolut config needs {k}"))
        };
        let base = config
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("https://merchant.revolut.com")
            .trim_end_matches('/')
            .to_string();
        let token = field("token")?;
        let api_version = field("api_version")?;
        Ok(Self {
            api: RevolutApi::new(RevolutConfig {
                url: Some(base.clone()),
                api_version: api_version.clone(),
                token: token.clone(),
                public_key: config
                    .get("public_key")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            })?,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            base,
            token,
            api_version,
            webhook_secret: config
                .get("webhook_secret")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    fn amount(currency: &str, amount: u64) -> Result<CurrencyAmount> {
        let cur = Currency::from_str(currency).map_err(|_| anyhow!("bad currency {currency}"))?;
        Ok(CurrencyAmount::from_u64(cur, amount))
    }

    async fn raw(&self, method: reqwest::Method, path: &str) -> Result<Value> {
        let rsp = self
            .http
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .header("Revolut-Api-Version", &self.api_version)
            .send()
            .await?;
        let status = rsp.status();
        let body: Value = rsp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            bail!(
                "revolut {status}: {}",
                body["message"].as_str().unwrap_or("error")
            );
        }
        Ok(body)
    }

    async fn order_events(&self, event: &str, order_id: &str) -> Result<Vec<ProviderEvent>> {
        let order = match self
            .raw(reqwest::Method::GET, &format!("/api/orders/{order_id}"))
            .await
        {
            Ok(order) => order,
            Err(_) if event != "ORDER_COMPLETED" => {
                return Ok(vec![ProviderEvent::Failed {
                    lookup: order_id.to_string(),
                    reason: "order failed or cancelled".into(),
                }]);
            }
            Err(e) => return Err(e),
        };
        let state = order["state"].as_str().unwrap_or_default().to_lowercase();
        if order["type"].as_str() == Some("refund") {
            let Some(parent) = order["related_order_id"].as_str() else {
                return Ok(vec![]);
            };
            return Ok(match (event, state.as_str()) {
                ("ORDER_COMPLETED", "completed") => vec![ProviderEvent::Refunded {
                    lookup: parent.to_string(),
                    external_id: order_id.to_string(),
                    amount: order["amount"].as_u64(),
                }],
                ("ORDER_FAILED", _) | ("ORDER_CANCELLED", _) => vec![ProviderEvent::RefundFailed {
                    lookup: parent.to_string(),
                    external_id: order_id.to_string(),
                    reason: "refund order failed".into(),
                }],
                _ => vec![],
            });
        }
        if event != "ORDER_COMPLETED" {
            return Ok(vec![ProviderEvent::Failed {
                lookup: order_id.to_string(),
                reason: "order failed or cancelled".into(),
            }]);
        }
        if state != "completed" {
            return Ok(vec![]);
        }
        let saved = self.saved_for_order(order_id).await.unwrap_or_else(|e| {
            log::warn!("could not read saved method: {e:#}");
            None
        });
        Ok(vec![ProviderEvent::Paid {
            lookup: order_id.to_string(),
            reference: order_id.to_string(),
            amount: order["amount"].as_u64(),
            confirmed: true,
            saved,
        }])
    }

    async fn settled_order(&self, mut order: RevolutOrder) -> Result<RevolutOrder> {
        for _ in 0..CHARGE_POLLS {
            if !matches!(
                order.state,
                RevolutOrderState::Pending
                    | RevolutOrderState::Processing
                    | RevolutOrderState::Authorised
            ) {
                break;
            }
            tokio::time::sleep(CHARGE_POLL_INTERVAL).await;
            order = self.api.get_order(&order.id).await?;
        }
        Ok(order)
    }

    async fn saved_for_order(&self, order_id: &str) -> Result<Option<SavedMethodInfo>> {
        let order = self.api.get_order(order_id).await?;
        let Some(customer) = order.customer_id() else {
            return Ok(None);
        };
        let methods = self
            .api
            .get_customer_payment_methods(&customer, true)
            .await?;
        Ok(methods.into_iter().next().map(|m| {
            let card = m.method_details.as_ref();
            SavedMethodInfo {
                customer_id: customer.clone(),
                method_id: m.id.clone(),
                brand: card.and_then(|c| c.brand.clone()),
                last4: card.and_then(|c| c.last4.clone()),
                exp_month: card.and_then(|c| c.expiry_month),
                exp_year: card.and_then(|c| c.expiry_year),
            }
        }))
    }
}

#[async_trait]
impl Provider for RevolutProvider {
    fn method(&self) -> &'static str {
        "revolut"
    }

    async fn setup(&self, hook_url: &str) -> Result<Option<Value>> {
        let existing = self.api.list_webhooks().await?;
        let current = existing.iter().find(|w| w.url == hook_url);
        if current.is_some() && self.webhook_secret.is_some() {
            return Ok(None);
        }
        if let Some(w) = current {
            self.api.delete_webhook(&w.id).await?;
        }
        let rsp = self
            .http
            .post(format!("{}/api/1.0/webhooks", self.base))
            .bearer_auth(&self.token)
            .header("Revolut-Api-Version", &self.api_version)
            .json(&json!({"url": hook_url, "events": WEBHOOK_EVENTS}))
            .send()
            .await?;
        let status = rsp.status();
        let created: Value = rsp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            bail!("revolut webhook registration {status}");
        }
        let secret = created["signing_secret"]
            .as_str()
            .context("revolut returned no signing secret")?;
        Ok(Some(json!({ "webhook_secret": secret })))
    }

    async fn create(&self, req: AttemptRequest) -> Result<CreatedAttempt> {
        let customer = req.save_method.then(|| RevolutCustomer {
            email: req.customer_email.clone(),
            ..Default::default()
        });
        if req.save_method && customer.as_ref().and_then(|c| c.email.as_ref()).is_none() {
            bail!("customer_email is required to save a payment method");
        }
        let line_items = (!req.lines.is_empty()).then(|| {
            req.lines
                .iter()
                .map(|l| LineItem {
                    name: l.name.clone(),
                    description: l.description.clone(),
                    unit_amount: l.unit_amount,
                    quantity: l.quantity,
                    currency: req.currency.clone(),
                    images: None,
                    metadata: None,
                    tax_amount: None,
                    tax_name: None,
                })
                .collect::<Vec<_>>()
        });
        let order = self
            .api
            .create_order_ext(
                Self::amount(&req.currency, req.amount)?,
                Some(req.description.clone()),
                line_items,
                customer,
                req.save_method.then(|| "merchant".to_string()),
            )
            .await?;
        let url = order
            .checkout_url
            .clone()
            .context("revolut returned no checkout_url")?;
        let mut details = json!({"checkout_url": url});
        let mode = if self.base.contains("sandbox") {
            "sandbox"
        } else {
            "prod"
        };
        details["widget"] = json!({
            "provider": "revolut",
            "token": order.token,
            "mode": mode,
            "email": req.customer_email,
        });
        Ok(CreatedAttempt {
            lookup: order.id.clone(),
            external_id: Some(order.id),
            details,
            expires: None,
            completed: false,
        })
    }

    async fn cancel(&self, attempt: &AttemptRow) -> Result<()> {
        self.api.cancel_order(&attempt.lookup).await?;
        Ok(())
    }

    async fn handle_webhook(
        &self,
        headers: &HashMap<String, String>,
        body: &[u8],
    ) -> Result<Vec<ProviderEvent>> {
        let secret = self
            .webhook_secret
            .as_deref()
            .context("revolut webhook_secret not configured")?;
        let msg = super::webhook_check::revolut_verify(secret, headers, body)?;
        let event = msg["event"].as_str().unwrap_or_default();
        let order_id = msg["order_id"]
            .as_str()
            .or_else(|| msg["data"]["order_id"].as_str())
            .unwrap_or_default()
            .to_string();
        if order_id.is_empty() {
            return Ok(vec![]);
        }
        match event {
            "ORDER_COMPLETED" | "ORDER_FAILED" | "ORDER_CANCELLED" => {
                self.order_events(event, &order_id).await
            }
            "ORDER_PAYMENT_DECLINED" | "ORDER_PAYMENT_FAILED" => Ok(vec![ProviderEvent::Failed {
                lookup: order_id,
                reason: "payment declined".into(),
            }]),
            "DISPUTE_ACTION_REQUIRED" | "DISPUTE_UNDER_REVIEW" => {
                Ok(vec![ProviderEvent::Dispute {
                    lookup: order_id,
                    state: DisputeState::Open,
                    reason: None,
                }])
            }
            "DISPUTE_WON" => Ok(vec![ProviderEvent::Dispute {
                lookup: order_id,
                state: DisputeState::Won,
                reason: None,
            }]),
            "DISPUTE_LOST" => Ok(vec![ProviderEvent::Dispute {
                lookup: order_id,
                state: DisputeState::Lost,
                reason: None,
            }]),
            _ => Ok(vec![]),
        }
    }

    fn event_id(&self, body: &[u8]) -> Option<String> {
        let v: Value = serde_json::from_slice(body).ok()?;
        let event = v["event"].as_str()?;
        let order = v["order_id"]
            .as_str()
            .or_else(|| v["data"]["order_id"].as_str())?;
        let dispute = v["dispute_id"]
            .as_str()
            .or_else(|| v["data"]["id"].as_str());
        Some(match dispute {
            Some(d) => format!("{event}:{order}:{d}"),
            None => format!("{event}:{order}"),
        })
    }

    async fn test_connection(&self) -> Result<String> {
        let hooks = self.api.list_webhooks().await?;
        Ok(format!(
            "Connected to Revolut ({} webhooks registered)",
            hooks.len()
        ))
    }

    async fn list_saved(&self, customer_id: &str) -> Result<Vec<SavedMethodInfo>> {
        let rsp = self
            .raw(
                reqwest::Method::GET,
                &format!("/api/1.0/customers/{customer_id}/payment-methods"),
            )
            .await?;
        Ok(rsp
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|m| {
                let d = &m["method_details"];
                Some(SavedMethodInfo {
                    customer_id: customer_id.to_string(),
                    method_id: m["id"].as_str()?.to_string(),
                    brand: d["brand"].as_str().map(str::to_string),
                    last4: d["last4"].as_str().map(str::to_string),
                    exp_month: d["expiry_month"].as_u64().map(|v| v as u16),
                    exp_year: d["expiry_year"].as_u64().map(|v| v as u16),
                })
            })
            .collect())
    }

    async fn remove_saved(&self, saved: &SavedMethodRow) -> Result<()> {
        let rsp = self
            .http
            .delete(format!(
                "{}/api/1.0/customers/{}/payment-methods/{}",
                self.base, saved.external_customer_id, saved.external_id
            ))
            .bearer_auth(&self.token)
            .header("Revolut-Api-Version", &self.api_version)
            .send()
            .await?;
        if !rsp.status().is_success() && rsp.status() != reqwest::StatusCode::NOT_FOUND {
            bail!("revolut {}", rsp.status());
        }
        Ok(())
    }

    async fn refund(&self, attempt: &AttemptRow, req: RefundRequest) -> Result<RefundOutcome> {
        let rsp = self
            .http
            .post(format!(
                "{}/api/orders/{}/refund",
                self.base, attempt.lookup
            ))
            .bearer_auth(&self.token)
            .header("Revolut-Api-Version", &self.api_version)
            .json(&json!({"amount": req.amount, "currency": req.currency, "description": "refund"}))
            .send()
            .await?;
        let status = rsp.status();
        let body: Value = rsp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            bail!("revolut refund {status}: {}", body["message"]);
        }
        Ok(RefundOutcome {
            external_id: body["id"].as_str().map(str::to_string),
        })
    }

    async fn charge_saved(
        &self,
        saved: &SavedMethodRow,
        req: AttemptRequest,
    ) -> Result<CreatedAttempt> {
        let order = self
            .api
            .create_off_session_order(
                &saved.external_customer_id,
                &saved.external_id,
                RevolutSavedPaymentMethodType::Card,
                Self::amount(&req.currency, req.amount)?,
                Some(req.description),
            )
            .await?;
        let order = self.settled_order(order).await?;
        if !matches!(order.state, RevolutOrderState::Completed) {
            bail!("charge not completed: {:?}", order.state);
        }
        Ok(CreatedAttempt {
            lookup: order.id.clone(),
            external_id: Some(order.id),
            details: json!({"charged": saved.external_id}),
            expires: None,
            completed: true,
        })
    }
}
