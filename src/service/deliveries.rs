use super::*;

impl Service {
    pub async fn deliver_due(&self) -> Result<usize> {
        let now = self.clock.now();
        let due = db::due_deliveries(&self.pool, now).await?;
        let mut sent = 0;
        for d in due {
            if !db::claim_delivery(&self.pool, d.id, now, 60).await? {
                continue;
            }
            let tenant = self.tenant(d.tenant_id).await?;
            let (capture, outcome) = self.post_to_target(&tenant, &d).await;
            let now = self.clock.now();
            db::record_delivery_capture(&self.pool, d.id, now, &capture).await?;
            self.metrics.inc(
                "gateway_webhook_deliveries_total",
                &[(
                    "outcome",
                    if outcome.is_ok() {
                        "success"
                    } else {
                        "failure"
                    },
                )],
            );
            match outcome {
                Ok(()) => {
                    db::delivery_succeeded(&self.pool, d.id, now).await?;
                    sent += 1;
                }
                Err(e) => {
                    let give_up = d.created + Duration::seconds(WEBHOOK_GIVE_UP_SECS);
                    let delay = backoff_secs(d.attempts + 1);
                    let next = (now + Duration::seconds(delay)).min(give_up + Duration::seconds(1));
                    let next = if now >= give_up { None } else { Some(next) };
                    if next.is_none() {
                        self.metrics.inc(
                            "gateway_webhook_deliveries_total",
                            &[("outcome", "abandoned")],
                        );
                    }
                    log::warn!(
                        "webhook {} for tenant {} failed: {e}",
                        d.event_id,
                        d.tenant_id
                    );
                    db::delivery_failed(&self.pool, d.id, next, now, &e.to_string()).await?;
                    if next.is_none() {
                        self.notify_webhook_abandoned(d.tenant_id, &e.to_string())
                            .await;
                    }
                }
            }
        }
        Ok(sent)
    }

    async fn post_to_target(
        &self,
        tenant: &Tenant,
        d: &DueDelivery,
    ) -> (db::DeliveryCapture, anyhow::Result<()>) {
        let Some(id) = d.endpoint_id else {
            return (
                db::DeliveryCapture::default(),
                Err(anyhow!("delivery has no webhook endpoint")),
            );
        };
        let endpoint = match db::get_endpoint(&self.pool, tenant.id, id).await {
            Ok(Some(e)) => e,
            Ok(None) => {
                return (
                    db::DeliveryCapture::default(),
                    Err(anyhow!("webhook endpoint was deleted")),
                );
            }
            Err(e) => return (db::DeliveryCapture::default(), Err(e.into())),
        };
        self.post_webhook_captured(
            tenant,
            &endpoint.url,
            &endpoint.secret,
            &d.event_id,
            &d.body,
        )
        .await
    }

    pub async fn send_test_event(
        &self,
        tenant: &Tenant,
        endpoint: &EndpointRow,
    ) -> anyhow::Result<String> {
        let event_id = random_hex(16);
        let body = serde_json::json!({
            "id": event_id,
            "type": "webhook.test",
            "created": self.clock.now().timestamp(),
            "data": {"message": "test event from the payment gateway"},
        })
        .to_string();
        self.post_webhook_captured(tenant, &endpoint.url, &endpoint.secret, &event_id, &body)
            .await
            .1?;
        Ok(event_id)
    }

    pub(super) async fn post_webhook_captured(
        &self,
        tenant: &Tenant,
        url: &str,
        sealed_secret: &[u8],
        event_id: &str,
        body: &str,
    ) -> (db::DeliveryCapture, anyhow::Result<()>) {
        let started = std::time::Instant::now();
        let mut capture = db::DeliveryCapture::default();
        let result = self
            .send_signed(tenant, url, sealed_secret, event_id, body, &mut capture)
            .await;
        capture.duration_ms = Some(started.elapsed().as_millis().min(i32::MAX as u128) as i32);
        (capture, result)
    }

    async fn send_signed(
        &self,
        tenant: &Tenant,
        url: &str,
        sealed_secret: &[u8],
        event_id: &str,
        body: &str,
        capture: &mut db::DeliveryCapture,
    ) -> anyhow::Result<()> {
        let client = if tenant.trusted {
            self.http.clone()
        } else {
            let target = crate::ssrf::resolve_public(url).await?;
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .redirect(reqwest::redirect::Policy::none())
                .resolve(&target.host, target.addr)
                .build()?
        };
        let secret = String::from_utf8(self.sealer.open(sealed_secret)?)?;
        let ts = self.clock.now().timestamp();
        let sig = webhook_signature(&secret, ts, body);
        let rsp = client
            .post(url)
            .header("content-type", "application/json")
            .header("x-gateway-event-id", event_id)
            .header("x-gateway-signature", format!("t={ts},v1={sig}"))
            .body(body.to_string())
            .send()
            .await?;
        let status = rsp.status();
        capture.status = Some(status.as_u16() as i32);
        let text = rsp.text().await.unwrap_or_default();
        capture.response = Some(text.chars().take(500).collect());
        if !status.is_success() {
            return Err(anyhow!("webhook returned {status}"));
        }
        Ok(())
    }
}

pub fn backoff_secs(attempt: u32) -> i64 {
    (15i64 << attempt.min(11)).min(6 * 3600)
}

pub fn webhook_signature(secret: &str, ts: i64, body: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(format!("{ts}.{body}").as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn endpoint_wants(endpoint: &EndpointRow, event_type: &str) -> bool {
    match endpoint.events.as_deref() {
        None | Some("") => true,
        Some(filter) => filter.split(',').map(str::trim).any(|f| {
            f == "*"
                || f == event_type
                || f.strip_suffix(".*")
                    .is_some_and(|p| event_type.starts_with(&format!("{p}.")))
        }),
    }
}
