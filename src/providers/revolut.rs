use super::{
    AttemptRequest, CreatedAttempt, Provider, ProviderEvent, RefundOutcome, RefundRequest,
    SavedMethodInfo,
};
use crate::db::{AttemptRow, SavedMethodRow};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use payments_rs::currency::{Currency, CurrencyAmount};
use payments_rs::fiat::{
    RevolutApi, RevolutConfig, RevolutCustomer, RevolutOrderState, RevolutSavedPaymentMethodType,
    RevolutWebhookBody, RevolutWebhookEvent,
};
use payments_rs::webhook::WebhookMessage;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;

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
        let created = self
            .api
            .create_webhook(
                hook_url,
                vec![
                    RevolutWebhookEvent::OrderCompleted,
                    RevolutWebhookEvent::OrderCancelled,
                    RevolutWebhookEvent::OrderFailed,
                ],
            )
            .await?;
        let secret = created
            .signing_secret
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
        let order = self
            .api
            .create_order_ext(
                Self::amount(&req.currency, req.amount)?,
                Some(req.description.clone()),
                None,
                customer,
                req.save_method.then(|| "merchant".to_string()),
            )
            .await?;
        let url = order
            .checkout_url
            .clone()
            .context("revolut returned no checkout_url")?;
        Ok(CreatedAttempt {
            lookup: order.id.clone(),
            external_id: Some(order.id),
            details: json!({"checkout_url": url}),
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
        let msg = RevolutWebhookBody::verify(
            secret,
            &WebhookMessage {
                endpoint: "revolut".into(),
                body: body.to_vec(),
                headers: headers.clone(),
            },
        )?;
        match msg.event {
            RevolutWebhookEvent::OrderCompleted => {
                let order = self.api.get_order(&msg.order_id).await?;
                if !matches!(order.state, RevolutOrderState::Completed) {
                    return Ok(vec![]);
                }
                let saved = self
                    .saved_for_order(&msg.order_id)
                    .await
                    .unwrap_or_else(|e| {
                        log::warn!("could not read saved method: {e:#}");
                        None
                    });
                Ok(vec![ProviderEvent::Paid {
                    lookup: msg.order_id.clone(),
                    reference: msg.order_id,
                    amount: Some(order.amount),
                    confirmed: true,
                    saved,
                }])
            }
            RevolutWebhookEvent::OrderFailed | RevolutWebhookEvent::OrderCancelled => {
                Ok(vec![ProviderEvent::Failed {
                    lookup: msg.order_id,
                    reason: "order failed or cancelled".into(),
                }])
            }
            _ => Ok(vec![]),
        }
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
