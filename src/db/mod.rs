use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{Executor, FromRow, MySql, MySqlPool};

pub type PaymentId = [u8; 32];

pub fn parse_payment_id(s: &str) -> Option<PaymentId> {
    hex::decode(s).ok()?.try_into().ok()
}

pub fn payment_id_of(row: &[u8]) -> PaymentId {
    row.try_into().expect("payment id is 32 bytes")
}

#[derive(Clone, Debug, FromRow)]
pub struct Tenant {
    pub id: u64,
    pub name: String,
    pub api_key_hash: String,
    pub webhook_url: Option<String>,
    pub webhook_secret: Vec<u8>,
    pub enabled: bool,
    pub display_name: Option<String>,
    pub logo_url: Option<String>,
    pub accent_color: Option<String>,
    pub allowed_origins: Option<String>,
    pub default_success_url: Option<String>,
    pub default_cancel_url: Option<String>,
    pub trusted: bool,
    #[sqlx(flatten)]
    pub limits: crate::limits::LimitOverrides,
    pub require_2fa: bool,
}

impl Tenant {
    pub fn origins(&self) -> Vec<String> {
        self.allowed_origins
            .as_deref()
            .unwrap_or("")
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct ProviderConfigRow {
    pub id: u64,
    pub tenant_id: u64,
    pub method: String,
    pub provider_type: String,
    pub name: String,
    pub enabled: bool,
    #[serde(skip)]
    pub config: Vec<u8>,
    pub fee_rate: Option<f64>,
    pub fee_base: Option<u64>,
    pub fee_currency: Option<String>,
    pub shared_provider_id: Option<u64>,
}

#[derive(Clone, Debug, FromRow)]
pub struct PaymentRow {
    pub id: Vec<u8>,
    pub tenant_id: u64,
    pub reference: String,
    pub currency: String,
    pub amount: u64,
    pub tax: u64,
    pub status: String,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,
    pub description: Option<String>,
    pub customer_ref: Option<String>,
    pub customer_email: Option<String>,
    pub metadata: Option<String>,
    pub methods: Option<String>,
    pub idempotency_key: Option<String>,
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
}

impl PaymentRow {
    pub fn pid(&self) -> PaymentId {
        payment_id_of(&self.id)
    }
}

#[derive(Clone, Debug, FromRow)]
pub struct AttemptRow {
    pub id: u64,
    pub payment_id: Vec<u8>,
    pub provider_config_id: u64,
    pub watch_provider_id: u64,
    pub method: String,
    pub option_key: String,
    pub base_amount: u64,
    pub processing_fee: u64,
    pub currency: String,
    pub charge_amount: u64,
    pub charge_currency: String,
    pub rate: f64,
    pub lookup: String,
    pub external_id: Option<String>,
    pub details: String,
    pub status: String,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub paid_at: Option<DateTime<Utc>>,
    pub save_method: bool,
    pub shared_provider_id: Option<u64>,
    pub platform_fee_bps: Option<u32>,
    pub platform_fee_base: Option<u64>,
    pub platform_fee: Option<u64>,
    pub platform_net: Option<u64>,
}

#[derive(Clone, Debug, FromRow)]
pub struct ReceiptRow {
    pub id: u64,
    pub attempt_id: u64,
    pub r#ref: String,
    pub amount: u64,
    pub confirmed: bool,
}

#[derive(Clone, Debug, FromRow)]
pub struct SavedMethodRow {
    pub id: u64,
    pub tenant_id: u64,
    pub customer_ref: String,
    pub provider_config_id: u64,
    pub external_customer_id: String,
    pub external_id: String,
    pub brand: Option<String>,
    pub last4: Option<String>,
    pub exp_month: Option<u16>,
    pub exp_year: Option<u16>,
    pub enabled: bool,
    pub created: DateTime<Utc>,
}

#[derive(Clone, Debug, FromRow)]
pub struct RefundRow {
    pub id: u64,
    pub payment_id: Vec<u8>,
    pub attempt_id: u64,
    pub amount: u64,
    pub charge_amount: u64,
    pub currency: String,
    pub destination: Option<String>,
    pub status: String,
    pub external_id: Option<String>,
    pub error: Option<String>,
    pub created: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct DeliveryRow {
    pub id: u64,
    pub tenant_id: u64,
    pub event_id: String,
    pub event_type: String,
    pub attempts: u32,
    pub created: DateTime<Utc>,
    pub next_attempt: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub abandoned_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

#[derive(Clone, Debug, FromRow)]
pub struct DueDelivery {
    pub id: u64,
    pub tenant_id: u64,
    pub event_id: String,
    pub body: String,
    pub attempts: u32,
    pub created: DateTime<Utc>,
    pub endpoint_id: Option<u64>,
}

pub async fn migrate(pool: &MySqlPool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

pub fn expected_migrations() -> usize {
    sqlx::migrate!("./migrations").migrations.len()
}

pub async fn connect(url: &str) -> Result<MySqlPool> {
    connect_with(url, 10, std::time::Duration::from_secs(10)).await
}

pub async fn connect_with(
    url: &str,
    max_connections: u32,
    acquire_timeout: std::time::Duration,
) -> Result<MySqlPool> {
    let pool = sqlx::mysql::MySqlPoolOptions::new()
        .max_connections(max_connections)
        .acquire_timeout(acquire_timeout)
        .connect(url)
        .await?;
    migrate(&pool).await?;
    Ok(pool)
}

const TENANT_COLS: &str = "id, name, api_key_hash, (select e.url from webhook_endpoint e where e.tenant_id = tenant.id and e.is_default = 1 limit 1) as webhook_url, webhook_secret, enabled, display_name, logo_url, accent_color, allowed_origins, default_success_url, default_cancel_url, trusted, limit_payments_per_day, limit_pending_payments, limit_amount_per_payment, limit_volume_per_day, limit_providers, limit_webhook_endpoints, limit_attempts_per_payment, require_2fa";
const PROVIDER_COLS: &str = "id, tenant_id, method, provider_type, name, enabled, config, fee_rate, fee_base, fee_currency, shared_provider_id";
const PAYMENT_COLS: &str = "id, tenant_id, reference, currency, amount, tax, status, created, expires, paid_at, description, customer_ref, customer_email, metadata, methods, idempotency_key, success_url, cancel_url";
const ATTEMPT_COLS: &str = "id, payment_id, provider_config_id, watch_provider_id, method, option_key, base_amount, processing_fee, currency, charge_amount, charge_currency, rate, lookup, external_id, details, status, created, expires, paid_at, save_method, shared_provider_id, platform_fee_bps, platform_fee_base, platform_fee, platform_net";
const SAVED_COLS: &str = "id, tenant_id, customer_ref, provider_config_id, external_customer_id, external_id, brand, last4, exp_month, exp_year, enabled, created";
const REFUND_COLS: &str = "id, payment_id, attempt_id, amount, charge_amount, currency, destination, status, external_id, error, created";

mod attempts;
mod deliveries;
mod endpoints;
mod keys;
mod notify;
mod payments;
mod providers;
mod refunds;
mod saved;
mod shared;
mod stats;
mod tenants;
mod withdrawals;

pub use attempts::*;
pub use deliveries::*;
pub use endpoints::*;
pub use keys::*;
pub use notify::*;
pub use payments::*;
pub use providers::*;
pub use refunds::*;
pub use saved::*;
pub use shared::*;
pub use stats::*;
pub use tenants::*;
pub use withdrawals::*;
