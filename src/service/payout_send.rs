use super::*;
use crate::db::WithdrawalRow;
use crate::ledger::{self, AccountKind, AccountRef, Entry, Posting, TxKind};
use crate::payouts::{
    self, CURRENCY, DestinationKind, OnchainState, PaymentState, PayoutNode, onchain_label,
};

enum Outcome {
    Completed { fee: u64, external_id: String },
    Failed(String),
    Unknown(String),
    Retry(String),
}

impl Service {
    pub async fn run_payouts(&self) -> Result<usize> {
        let Some(node) = self.payouts.node() else {
            return Ok(0);
        };
        let handled = self.run_payouts_with(node).await;
        self.publish_payout_gauges().await;
        handled
    }

    async fn publish_payout_gauges(&self) {
        let age = db::oldest_sending(&self.pool)
            .await
            .ok()
            .flatten()
            .map(|t| (Utc::now() - t).num_seconds().max(0) as f64)
            .unwrap_or(0.0);
        self.metrics
            .set("gateway_payout_oldest_sending_age_seconds", &[], age);
    }

    async fn run_payouts_with(&self, node: Arc<dyn PayoutNode>) -> Result<usize> {
        let policy = self.payout_policy();
        let now = Utc::now();
        let mut handled = 0;
        let stale = db::stale_sending(&self.pool, now - policy.sending_lease, 50).await?;
        for w in stale {
            if !db::reclaim_stale(&self.pool, w.id, now - policy.sending_lease, now).await? {
                continue;
            }
            let w = WithdrawalRow {
                attempts: w.attempts + 1,
                ..w
            };
            let outcome = self.reconcile_sending(node.as_ref(), &w).await;
            self.apply_outcome(&w, outcome).await?;
            handled += 1;
        }
        for w in db::due_withdrawals(&self.pool, now - policy.retry_delay, 50).await? {
            if !db::claim_withdrawal(&self.pool, w.id, Utc::now()).await? {
                continue;
            }
            let w = WithdrawalRow {
                attempts: w.attempts + 1,
                ..w
            };
            let outcome = self.send_withdrawal(node.as_ref(), &w).await;
            self.apply_outcome(&w, outcome).await?;
            handled += 1;
        }
        Ok(handled)
    }

    async fn apply_outcome(&self, w: &WithdrawalRow, outcome: Outcome) -> Result<()> {
        let outcome = match outcome {
            Outcome::Retry(error) if w.attempts >= self.payout_policy().max_attempts => {
                Outcome::Failed(format!("gave up after {} attempts: {error}", w.attempts))
            }
            o => o,
        };
        let tenant = self.tenant(w.tenant_id).await?;
        match outcome {
            Outcome::Completed { fee, external_id } => {
                self.metrics
                    .inc("gateway_payout_sends_total", &[("result", "completed")]);
                self.complete_withdrawal(&tenant, w.id, fee, &external_id)
                    .await?;
            }
            Outcome::Failed(reason) => {
                self.metrics
                    .inc("gateway_payout_sends_total", &[("result", "failed")]);
                self.metrics.inc("gateway_payout_failures_total", &[]);
                self.release(&tenant, w.id, &["sending"], "failed", &reason, true)
                    .await?;
                if w.purpose == "payout" {
                    let detail = format!("Withdrawal {} failed: {reason}", w.id);
                    self.notify_quietly(
                        w.tenant_id,
                        withdrawals::EVENT_WITHDRAWAL_FAILED,
                        "Withdrawal failed",
                        &detail,
                    )
                    .await;
                }
            }
            Outcome::Retry(error) => {
                self.metrics.inc("gateway_payout_failures_total", &[]);
                db::requeue_withdrawal(&self.pool, w.id, &error).await?;
            }
            Outcome::Unknown(note) => {
                db::note_withdrawal_error(&self.pool, w.id, &note).await?;
            }
        }
        Ok(())
    }

    async fn send_withdrawal(&self, node: &dyn PayoutNode, w: &WithdrawalRow) -> Outcome {
        match self.try_send(node, w).await {
            Ok(o) => o,
            Err(e) => Outcome::Unknown(format!("{e:#}")),
        }
    }

    async fn try_send(&self, node: &dyn PayoutNode, w: &WithdrawalRow) -> anyhow::Result<Outcome> {
        let kind = DestinationKind::parse(&w.destination_kind)
            .ok_or_else(|| anyhow!("unknown destination kind"))?;
        if kind == DestinationKind::Onchain {
            return self.send_onchain(node, w).await;
        }
        let (invoice, hash) = match self.invoice_for(w, kind).await {
            Ok(v) => v,
            Err(e) => return Ok(Outcome::Failed(format!("{e:#}"))),
        };
        let fee_limit = w.max_fee;
        let call = node.pay_invoice(&invoice, fee_limit).await;
        let state = node.payment_state(&hash).await;
        Ok(match (state, call) {
            (
                Ok(PaymentState::Succeeded {
                    fee_msat,
                    reference,
                }),
                _,
            ) => Outcome::Completed {
                fee: fee_msat.min(w.max_fee),
                external_id: reference_or_hash(&reference, &hash),
            },
            (Ok(PaymentState::Failed(reason)), _) => Outcome::Failed(reason),
            (Ok(PaymentState::InFlight), _) => Outcome::Unknown("payment in flight".into()),
            (Ok(PaymentState::NotFound), Err(e)) => Outcome::Retry(format!("{e:#}")),
            (Ok(PaymentState::NotFound), Ok(())) => {
                Outcome::Unknown("node accepted the payment but does not know it".into())
            }
            (Err(e), _) => Outcome::Unknown(format!("lookup failed: {e:#}")),
        })
    }

    async fn invoice_for(
        &self,
        w: &WithdrawalRow,
        kind: DestinationKind,
    ) -> anyhow::Result<(String, String)> {
        if let (Some(sealed), Some(hash)) = (&w.invoice, &w.payment_hash) {
            return Ok((String::from_utf8(self.sealer.open(sealed)?)?, hash.clone()));
        }
        let value = String::from_utf8(self.sealer.open(&w.destination)?)?;
        let invoice = payouts::resolve_invoice(
            kind,
            &value,
            w.amount,
            self.payout_policy().allow_private_lnurl,
        )
        .await?;
        let parsed = crate::providers::lightning::parse_invoice(&invoice)?;
        let hash = parsed.payment_hash();
        let sealed = self.sealer.seal(invoice.as_bytes())?;
        if !db::store_withdrawal_invoice(&self.pool, w.id, &sealed, &hash).await? {
            anyhow::bail!("withdrawal is no longer being sent");
        }
        Ok((invoice, hash))
    }

    async fn send_onchain(
        &self,
        node: &dyn PayoutNode,
        w: &WithdrawalRow,
    ) -> anyhow::Result<Outcome> {
        let label = onchain_label(w.id);
        if let OnchainState::Sent { txid, fee_sat } = node.onchain_state(&label).await? {
            return Ok(Outcome::Completed {
                fee: (fee_sat * 1000).min(w.max_fee),
                external_id: txid,
            });
        }
        let address = String::from_utf8(self.sealer.open(&w.destination)?)?;
        let call = node.send_coins(&address, w.amount / 1000, &label).await;
        Ok(match (node.onchain_state(&label).await, call) {
            (Ok(OnchainState::Sent { txid, fee_sat }), _) => Outcome::Completed {
                fee: (fee_sat * 1000).min(w.max_fee),
                external_id: txid,
            },
            (Ok(OnchainState::NotFound), Err(e)) => Outcome::Retry(format!("{e:#}")),
            (Ok(OnchainState::NotFound), Ok(())) => {
                Outcome::Unknown("node accepted the transaction but does not list it".into())
            }
            (Err(e), _) => Outcome::Unknown(format!("lookup failed: {e:#}")),
        })
    }

    async fn reconcile_sending(&self, node: &dyn PayoutNode, w: &WithdrawalRow) -> Outcome {
        let looked = async {
            let kind = DestinationKind::parse(&w.destination_kind)
                .ok_or_else(|| anyhow!("unknown destination kind"))?;
            if kind == DestinationKind::Onchain {
                return Ok(match node.onchain_state(&onchain_label(w.id)).await? {
                    OnchainState::Sent { txid, fee_sat } => Outcome::Completed {
                        fee: (fee_sat * 1000).min(w.max_fee),
                        external_id: txid,
                    },
                    OnchainState::NotFound => Outcome::Retry("not sent before restart".into()),
                });
            }
            let Some(hash) = &w.payment_hash else {
                return Ok(Outcome::Retry("no payment was started".into()));
            };
            Ok::<_, anyhow::Error>(match node.payment_state(hash).await? {
                PaymentState::Succeeded {
                    fee_msat,
                    reference,
                } => Outcome::Completed {
                    fee: fee_msat.min(w.max_fee),
                    external_id: reference_or_hash(&reference, hash),
                },
                PaymentState::Failed(reason) => Outcome::Failed(reason),
                PaymentState::InFlight => Outcome::Unknown("payment still in flight".into()),
                PaymentState::NotFound => Outcome::Retry("payment not known to the node".into()),
            })
        }
        .await;
        looked.unwrap_or_else(|e| Outcome::Unknown(format!("reconcile failed: {e:#}")))
    }

    pub(super) async fn complete_withdrawal(
        &self,
        tenant: &Tenant,
        id: u64,
        fee: u64,
        external_id: &str,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let w = db::lock_withdrawal(&mut *tx, id)
            .await?
            .filter(|w| w.tenant_id == tenant.id)
            .ok_or(Error::NotFound)?;
        if w.status == "completed" {
            return Ok(());
        }
        if w.status != "sending" {
            return Err(Error::Conflict(format!("withdrawal is {}", w.status)));
        }
        let fee = fee.min(w.max_fee);
        let reserve = (w.amount + w.max_fee) as i64;
        let mut entries = vec![
            Entry::new(
                AccountRef::tenant(w.tenant_id, AccountKind::PendingPayout, CURRENCY),
                -reserve,
            ),
            Entry::new(
                AccountRef::platform(AccountKind::PlatformWallet, CURRENCY),
                (w.amount + fee) as i64,
            ),
        ];
        if w.max_fee > fee {
            entries.push(Entry::new(
                AccountRef::tenant(w.tenant_id, AccountKind::TenantBalance, CURRENCY),
                (w.max_fee - fee) as i64,
            ));
        }
        let posting = Posting::new(TxKind::PayoutCompleted, entries).reference("withdrawal", id);
        ledger::post(&mut tx, &posting, &format!("withdrawal:{id}:completed")).await?;
        db::finish_withdrawal(
            &mut *tx,
            id,
            "completed",
            Some(fee),
            Some(external_id),
            None,
            Utc::now(),
        )
        .await?;
        if w.purpose == "verify"
            && let Some(did) = w.destination_id
        {
            db::mark_destination_verified(&mut *tx, did, Utc::now()).await?;
        }
        if w.purpose == "payout" {
            self.emit_withdrawal(&mut tx, tenant, "withdrawal.completed", id)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}

fn reference_or_hash(_preimage: &str, hash: &str) -> String {
    hash.to_string()
}
