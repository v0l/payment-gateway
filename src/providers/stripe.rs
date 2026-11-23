use super::{
    AttemptRequest, CreatedAttempt, DisputeState, Provider, ProviderEvent, RefundOutcome,
    RefundRequest, SavedMethodInfo,
};
use crate::db::{AttemptRow, SavedMethodRow};
use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use payments_rs::fiat::StripeWebhookEvent;
use payments_rs::webhook::WebhookMessage;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

pub struct StripeProvider {
    client: reqwest::Client,
    base: String,
    secret_key: String,
    webhook_secret: Option<String>,
}

impl StripeProvider {
    pub fn new(config: &Value) -> Result<Self> {
        let secret_key = config
            .get("secret_key")
            .and_then(Value::as_str)
            .context("stripe config needs secret_key")?
            .to_string();
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()?,
            base: config
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("https://api.stripe.com")
                .trim_end_matches('/')
                .to_string(),
            secret_key,
            webhook_secret: config
                .get("webhook_secret")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    async fn call(
        &self,
        method: reqwest::Method,
        path: &str,
        form: &[(String, String)],
    ) -> Result<Value> {
        let method_is_get = method == reqwest::Method::GET;
        let mut req = self
            .client
            .request(method, format!("{}{path}", self.base))
            .bearer_auth(&self.secret_key);
        if !form.is_empty() {
            req = if method_is_get {
                req.query(form)
            } else {
                req.form(form)
            };
        }
        let rsp = req.send().await?;
        let status = rsp.status();
        let body: Value = rsp.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let msg = body["error"]["message"].as_str().unwrap_or("stripe error");
            bail!("stripe {status}: {msg}");
        }
        Ok(body)
    }

    async fn lookup_for_intent(&self, intent: &str) -> String {
        let found = self
            .call(
                reqwest::Method::GET,
                "/v1/checkout/sessions",
                &[kv("payment_intent", intent), kv("limit", 1)],
            )
            .await
            .ok()
            .and_then(|v| v["data"][0]["id"].as_str().map(str::to_string));
        found.unwrap_or_else(|| intent.to_string())
    }

    async fn charge_events(&self, charge: &Value) -> Result<Vec<ProviderEvent>> {
        let (Some(charge_id), Some(intent)) =
            (charge["id"].as_str(), charge["payment_intent"].as_str())
        else {
            return Ok(vec![]);
        };
        let lookup = self.lookup_for_intent(intent).await;
        let refunds = self
            .call(
                reqwest::Method::GET,
                "/v1/refunds",
                &[kv("charge", charge_id), kv("limit", 100)],
            )
            .await?;
        let mut out = vec![];
        for r in refunds["data"].as_array().into_iter().flatten() {
            let Some(id) = r["id"].as_str() else { continue };
            match r["status"].as_str() {
                Some("succeeded") => out.push(ProviderEvent::Refunded {
                    lookup: lookup.clone(),
                    external_id: id.to_string(),
                    amount: r["amount"].as_u64(),
                }),
                Some("failed") | Some("canceled") => out.push(ProviderEvent::RefundFailed {
                    lookup: lookup.clone(),
                    external_id: id.to_string(),
                    reason: r["failure_reason"].as_str().unwrap_or("failed").to_string(),
                }),
                _ => {}
            }
        }
        Ok(out)
    }

    async fn saved_from_session(&self, session: &Value) -> Result<Option<SavedMethodInfo>> {
        let (Some(customer), Some(intent)) = (
            session["customer"].as_str(),
            session["payment_intent"].as_str(),
        ) else {
            return Ok(None);
        };
        let pi = self
            .call(
                reqwest::Method::GET,
                &format!("/v1/payment_intents/{intent}"),
                &[],
            )
            .await?;
        let Some(pm_id) = pi["payment_method"].as_str() else {
            return Ok(None);
        };
        let pm = self
            .call(
                reqwest::Method::GET,
                &format!("/v1/payment_methods/{pm_id}"),
                &[],
            )
            .await?;
        let card = &pm["card"];
        Ok(Some(SavedMethodInfo {
            customer_id: customer.to_string(),
            method_id: pm_id.to_string(),
            brand: card["brand"].as_str().map(str::to_string),
            last4: card["last4"].as_str().map(str::to_string),
            exp_month: card["exp_month"].as_u64().map(|v| v as u16),
            exp_year: card["exp_year"].as_u64().map(|v| v as u16),
        }))
    }
}

fn line_item_fields(req: &AttemptRequest) -> Vec<(String, String)> {
    if req.lines.is_empty() {
        return vec![
            kv("line_items[0][quantity]", 1),
            kv(
                "line_items[0][price_data][currency]",
                req.currency.to_lowercase(),
            ),
            kv("line_items[0][price_data][unit_amount]", req.amount),
            kv(
                "line_items[0][price_data][product_data][name]",
                &req.description,
            ),
        ];
    }
    let mut out = vec![];
    for (i, l) in req.lines.iter().enumerate() {
        out.push(kv(&format!("line_items[{i}][quantity]"), l.quantity));
        out.push(kv(
            &format!("line_items[{i}][price_data][currency]"),
            req.currency.to_lowercase(),
        ));
        out.push(kv(
            &format!("line_items[{i}][price_data][unit_amount]"),
            l.unit_amount,
        ));
        out.push(kv(
            &format!("line_items[{i}][price_data][product_data][name]"),
            &l.name,
        ));
        if let Some(d) = &l.description {
            out.push(kv(
                &format!("line_items[{i}][price_data][product_data][description]"),
                d,
            ));
        }
    }
    out
}

fn kv(k: &str, v: impl ToString) -> (String, String) {
    (k.to_string(), v.to_string())
}

#[async_trait]
impl Provider for StripeProvider {
    fn method(&self) -> &'static str {
        "stripe"
    }

    async fn create(&self, req: AttemptRequest) -> Result<CreatedAttempt> {
        let success = req.success_url.clone().context("success_url required")?;
        let cancel = req.cancel_url.clone().context("cancel_url required")?;
        let pid = hex::encode(req.payment_id);
        let mut form = vec![
            kv("mode", "payment"),
            kv("success_url", success),
            kv("cancel_url", cancel),
            kv("client_reference_id", &pid),
            kv(
                "expires_at",
                req.expires
                    .timestamp()
                    .max(chrono::Utc::now().timestamp() + 1800),
            ),
            kv("metadata[payment_id]", &pid),
        ];
        form.extend(line_item_fields(&req));
        if let Some(email) = &req.customer_email {
            form.push(kv("customer_email", email));
        }
        if req.save_method {
            form.push(kv("customer_creation", "always"));
            form.push(kv("payment_intent_data[setup_future_usage]", "off_session"));
            form.push(kv("metadata[save_method]", "1"));
        }
        let session = self
            .call(reqwest::Method::POST, "/v1/checkout/sessions", &form)
            .await?;
        let id = session["id"].as_str().context("session id")?.to_string();
        let url = session["url"].as_str().context("session url")?.to_string();
        Ok(CreatedAttempt {
            lookup: id.clone(),
            external_id: Some(id),
            details: json!({"checkout_url": url, "session_id": session["id"]}),
            expires: None,
            completed: false,
        })
    }

    async fn cancel(&self, attempt: &AttemptRow) -> Result<()> {
        if attempt.lookup.starts_with("cs_") {
            self.call(
                reqwest::Method::POST,
                &format!("/v1/checkout/sessions/{}/expire", attempt.lookup),
                &[],
            )
            .await?;
        }
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
            .context("stripe webhook_secret not configured")?;
        let event = StripeWebhookEvent::verify(
            secret,
            &WebhookMessage {
                endpoint: "stripe".into(),
                body: body.to_vec(),
                headers: headers.clone(),
            },
        )?;
        let session = &event.data.object;
        let lookup = session["id"].as_str().unwrap_or_default().to_string();
        match event.event_type.as_str() {
            "checkout.session.completed" | "checkout.session.async_payment_succeeded"
                if session["payment_status"].as_str() == Some("paid") =>
            {
                let wants_save = session["metadata"]["save_method"].as_str() == Some("1");
                let saved = if wants_save {
                    self.saved_from_session(session).await.unwrap_or_else(|e| {
                        log::warn!("could not read saved method: {e:#}");
                        None
                    })
                } else {
                    None
                };
                Ok(vec![ProviderEvent::Paid {
                    reference: lookup.clone(),
                    lookup,
                    amount: session["amount_total"].as_u64(),
                    confirmed: true,
                    saved,
                }])
            }
            "checkout.session.async_payment_failed" => Ok(vec![ProviderEvent::Failed {
                lookup,
                reason: "payment failed".into(),
            }]),
            "payment_intent.succeeded" => Ok(vec![ProviderEvent::Paid {
                reference: lookup.clone(),
                lookup,
                amount: session["amount_received"].as_u64(),
                confirmed: true,
                saved: None,
            }]),
            "payment_intent.payment_failed" => Ok(vec![ProviderEvent::Failed {
                lookup,
                reason: session["last_payment_error"]["message"]
                    .as_str()
                    .unwrap_or("payment failed")
                    .to_string(),
            }]),
            "charge.refunded" | "charge.refund.updated" => self.charge_events(session).await,
            "charge.dispute.created" | "charge.dispute.closed" => {
                let Some(intent) = session["payment_intent"].as_str() else {
                    return Ok(vec![]);
                };
                let state = match (event.event_type.as_str(), session["status"].as_str()) {
                    ("charge.dispute.created", _) => DisputeState::Open,
                    (_, Some("won")) | (_, Some("warning_closed")) => DisputeState::Won,
                    _ => DisputeState::Lost,
                };
                Ok(vec![ProviderEvent::Dispute {
                    lookup: self.lookup_for_intent(intent).await,
                    state,
                    reason: session["reason"].as_str().map(str::to_string),
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

    async fn test_connection(&self) -> Result<String> {
        let account = self.call(reqwest::Method::GET, "/v1/account", &[]).await?;
        let id = account["id"].as_str().unwrap_or("unknown");
        Ok(format!("Connected to Stripe account {id}"))
    }

    async fn remove_saved(&self, saved: &SavedMethodRow) -> Result<()> {
        self.call(
            reqwest::Method::POST,
            &format!("/v1/payment_methods/{}/detach", saved.external_id),
            &[],
        )
        .await?;
        Ok(())
    }

    async fn refund(&self, attempt: &AttemptRow, req: RefundRequest) -> Result<RefundOutcome> {
        let intent = if attempt.lookup.starts_with("pi_") {
            attempt.lookup.clone()
        } else {
            let session = self
                .call(
                    reqwest::Method::GET,
                    &format!("/v1/checkout/sessions/{}", attempt.lookup),
                    &[],
                )
                .await?;
            session["payment_intent"]
                .as_str()
                .ok_or_else(|| anyhow!("session has no payment intent"))?
                .to_string()
        };
        let refund = self
            .call(
                reqwest::Method::POST,
                "/v1/refunds",
                &[kv("payment_intent", intent), kv("amount", req.amount)],
            )
            .await?;
        Ok(RefundOutcome {
            external_id: refund["id"].as_str().map(str::to_string),
        })
    }

    async fn charge_saved(
        &self,
        saved: &SavedMethodRow,
        req: AttemptRequest,
    ) -> Result<CreatedAttempt> {
        let form = vec![
            kv("amount", req.amount),
            kv("currency", req.currency.to_lowercase()),
            kv("customer", &saved.external_customer_id),
            kv("payment_method", &saved.external_id),
            kv("off_session", "true"),
            kv("confirm", "true"),
            kv(
                "return_url",
                req.success_url.clone().unwrap_or_else(|| self.base.clone()),
            ),
            kv("description", &req.description),
            kv("metadata[payment_id]", hex::encode(req.payment_id)),
        ];
        let pi = self
            .call(reqwest::Method::POST, "/v1/payment_intents", &form)
            .await?;
        let id = pi["id"].as_str().context("intent id")?.to_string();
        if pi["status"].as_str() == Some("requires_action") {
            let link = pi["next_action"]["redirect_to_url"]["url"]
                .as_str()
                .or_else(|| pi["next_action"]["use_stripe_sdk"]["stripe_js"].as_str())
                .context("charge needs customer authentication but no link was provided")?;
            return Ok(CreatedAttempt {
                lookup: id.clone(),
                external_id: Some(id),
                details: json!({"checkout_url": link, "requires_action": true}),
                expires: None,
                completed: false,
            });
        }
        if pi["status"].as_str() != Some("succeeded") {
            bail!("charge not completed: {}", pi["status"]);
        }
        Ok(CreatedAttempt {
            lookup: id.clone(),
            external_id: Some(id),
            details: json!({"charged": saved.external_id}),
            expires: None,
            completed: true,
        })
    }
}
