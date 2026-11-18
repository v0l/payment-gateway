mod destination;
mod lnd;
mod lnurl;
mod policy;
mod view;

pub use destination::{Destination, DestinationKind, mask, normalize, signed_by_address};
pub use lnd::LndPayoutNode;
pub use lnurl::check_invoice_amount;
pub use lnurl::resolve_invoice;
pub use policy::PayoutPolicy;
pub use view::{DestinationView, WithdrawalView, destination_view};

use anyhow::Result;
use async_trait::async_trait;

pub const CURRENCY: &str = "BTC";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaymentState {
    NotFound,
    InFlight,
    Succeeded { fee_msat: u64, reference: String },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OnchainState {
    NotFound,
    Sent { txid: String, fee_sat: u64 },
}

#[async_trait]
pub trait PayoutNode: Send + Sync {
    async fn pay_invoice(&self, invoice: &str, fee_limit_msat: u64) -> Result<()>;
    async fn payment_state(&self, payment_hash: &str) -> Result<PaymentState>;
    async fn send_coins(&self, address: &str, sats: u64, label: &str) -> Result<()>;
    async fn onchain_state(&self, label: &str) -> Result<OnchainState>;
}

pub fn onchain_label(withdrawal_id: u64) -> String {
    format!("pgw-withdrawal-{withdrawal_id}")
}

pub struct Runtime {
    node: std::sync::RwLock<Option<std::sync::Arc<dyn PayoutNode>>>,
    policy: std::sync::RwLock<PayoutPolicy>,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            node: Default::default(),
            policy: std::sync::RwLock::new(PayoutPolicy::default()),
        }
    }
}

impl Runtime {
    pub fn node(&self) -> Option<std::sync::Arc<dyn PayoutNode>> {
        self.node.read().unwrap().clone()
    }

    pub fn set_node(&self, node: Option<std::sync::Arc<dyn PayoutNode>>) {
        *self.node.write().unwrap() = node;
    }

    pub fn policy(&self) -> PayoutPolicy {
        self.policy.read().unwrap().clone()
    }

    pub fn set_policy(&self, policy: PayoutPolicy) {
        *self.policy.write().unwrap() = policy;
    }
}
