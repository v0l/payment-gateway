use super::*;

impl Service {
    pub async fn deliver_due(&self) -> Result<usize> {
        let now = Utc::now();
        let due = db::due_deliveries(&self.pool, now).await?;
        let mut sent = 0;
        for d in due {
            if !db::claim_delivery(&self.pool, d.id, now, 60).await? {
                continue;
            }
            let tenant = self.tenant(d.tenant_id).await?;
            let outcome = self.post_webhook(&tenant, &d.event_id, &d.body).await;
            let now = Utc::now();
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

    async fn post_webhook(
        &self,
        tenant: &Tenant,
        event_id: &str,
        body: &str,
    ) -> anyhow::Result<()> {
        let url = tenant
            .webhook_url
            .as_deref()
            .ok_or_else(|| anyhow!("tenant has no webhook_url"))?;
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
        let secret = String::from_utf8(self.sealer.open(&tenant.webhook_secret)?)?;
        let ts = Utc::now().timestamp();
        let sig = webhook_signature(&secret, ts, body);
        let rsp = client
            .post(url)
            .header("content-type", "application/json")
            .header("x-gateway-event-id", event_id)
            .header("x-gateway-signature", format!("t={ts},v1={sig}"))
            .body(body.to_string())
            .send()
            .await?;
        if !rsp.status().is_success() {
            return Err(anyhow!("webhook returned {}", rsp.status()));
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
