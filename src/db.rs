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
}

pub async fn migrate(pool: &MySqlPool) -> Result<()> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}

pub async fn connect(url: &str) -> Result<MySqlPool> {
    let pool = MySqlPool::connect(url).await?;
    migrate(&pool).await?;
    Ok(pool)
}

const TENANT_COLS: &str = "id, name, api_key_hash, webhook_url, webhook_secret, enabled, display_name, logo_url, accent_color, allowed_origins, default_success_url, default_cancel_url, trusted";
const PROVIDER_COLS: &str =
    "id, tenant_id, method, provider_type, name, enabled, config, fee_rate, fee_base, fee_currency";
const PAYMENT_COLS: &str = "id, tenant_id, reference, currency, amount, tax, status, created, expires, paid_at, description, customer_ref, customer_email, metadata, methods, idempotency_key, success_url, cancel_url";
const ATTEMPT_COLS: &str = "id, payment_id, provider_config_id, watch_provider_id, method, option_key, base_amount, processing_fee, currency, charge_amount, charge_currency, rate, lookup, external_id, details, status, created, expires, paid_at, save_method";
const SAVED_COLS: &str = "id, tenant_id, customer_ref, provider_config_id, external_customer_id, external_id, brand, last4, exp_month, exp_year, enabled, created";
const REFUND_COLS: &str = "id, payment_id, attempt_id, amount, charge_amount, currency, destination, status, external_id, error, created";

pub async fn insert_tenant(
    pool: &MySqlPool,
    name: &str,
    api_key_hash: &str,
    webhook_url: Option<&str>,
    sealed_webhook_secret: &[u8],
) -> Result<u64> {
    let r = sqlx::query(
        "insert into tenant (name, api_key_hash, webhook_url, webhook_secret) values (?, ?, ?, ?)",
    )
    .bind(name)
    .bind(api_key_hash)
    .bind(webhook_url)
    .bind(sealed_webhook_secret)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn list_tenants(pool: &MySqlPool) -> Result<Vec<Tenant>> {
    Ok(
        sqlx::query_as(&format!("select {TENANT_COLS} from tenant order by id"))
            .fetch_all(pool)
            .await?,
    )
}

pub async fn get_tenant(pool: &MySqlPool, id: u64) -> Result<Option<Tenant>> {
    Ok(
        sqlx::query_as(&format!("select {TENANT_COLS} from tenant where id = ?"))
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn tenant_by_key_hash(pool: &MySqlPool, hash: &str) -> Result<Option<Tenant>> {
    Ok(sqlx::query_as(&format!(
        "select {TENANT_COLS} from tenant where api_key_hash = ?"
    ))
    .bind(hash)
    .fetch_optional(pool)
    .await?)
}

pub async fn set_tenant_enabled(pool: &MySqlPool, id: u64, enabled: bool) -> Result<bool> {
    let r = sqlx::query("update tenant set enabled = ? where id = ?")
        .bind(enabled)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn rotate_tenant_key(pool: &MySqlPool, id: u64, api_key_hash: &str) -> Result<bool> {
    let r = sqlx::query("update tenant set api_key_hash = ? where id = ?")
        .bind(api_key_hash)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub struct TenantSettings {
    pub webhook_url: Option<String>,
    pub display_name: Option<String>,
    pub logo_url: Option<String>,
    pub accent_color: Option<String>,
    pub allowed_origins: Option<String>,
    pub default_success_url: Option<String>,
    pub default_cancel_url: Option<String>,
}

pub async fn update_tenant_settings(pool: &MySqlPool, id: u64, s: &TenantSettings) -> Result<bool> {
    let r = sqlx::query(
        "update tenant set webhook_url = ?, display_name = ?, logo_url = ?, accent_color = ?, allowed_origins = ?, default_success_url = ?, default_cancel_url = ? where id = ?",
    )
    .bind(&s.webhook_url)
    .bind(&s.display_name)
    .bind(&s.logo_url)
    .bind(&s.accent_color)
    .bind(&s.allowed_origins)
    .bind(&s.default_success_url)
    .bind(&s.default_cancel_url)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub struct NewProviderConfig<'a> {
    pub tenant_id: u64,
    pub method: &'a str,
    pub provider_type: &'a str,
    pub name: &'a str,
    pub sealed_config: &'a [u8],
    pub fee_rate: Option<f64>,
    pub fee_base: Option<u64>,
    pub fee_currency: Option<&'a str>,
}

pub async fn insert_provider_config(pool: &MySqlPool, c: NewProviderConfig<'_>) -> Result<u64> {
    let r = sqlx::query(
        "insert into provider_config (tenant_id, method, provider_type, name, config, fee_rate, fee_base, fee_currency) values (?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(c.tenant_id)
    .bind(c.method)
    .bind(c.provider_type)
    .bind(c.name)
    .bind(c.sealed_config)
    .bind(c.fee_rate)
    .bind(c.fee_base)
    .bind(c.fee_currency)
    .execute(pool)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn get_provider_config(pool: &MySqlPool, id: u64) -> Result<Option<ProviderConfigRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PROVIDER_COLS} from provider_config where id = ?"
    ))
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn list_provider_configs(
    pool: &MySqlPool,
    tenant_id: u64,
    only_enabled: bool,
) -> Result<Vec<ProviderConfigRow>> {
    let filter = if only_enabled { " and enabled = 1" } else { "" };
    Ok(sqlx::query_as(&format!(
        "select {PROVIDER_COLS} from provider_config where tenant_id = ?{filter} order by id"
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn list_all_enabled_provider_configs(pool: &MySqlPool) -> Result<Vec<ProviderConfigRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PROVIDER_COLS} from provider_config where enabled = 1 and tenant_id in (select id from tenant where enabled = 1) order by id"
    ))
    .fetch_all(pool)
    .await?)
}

pub async fn set_provider_config_enabled(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
    enabled: bool,
) -> Result<bool> {
    let r = sqlx::query("update provider_config set enabled = ? where id = ? and tenant_id = ?")
        .bind(enabled)
        .bind(id)
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn update_provider_config_blob(pool: &MySqlPool, id: u64, sealed: &[u8]) -> Result<()> {
    sqlx::query("update provider_config set config = ? where id = ?")
        .bind(sealed)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub struct NewPayment<'a> {
    pub id: PaymentId,
    pub tenant_id: u64,
    pub reference: &'a str,
    pub currency: &'a str,
    pub amount: u64,
    pub tax: u64,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub description: Option<&'a str>,
    pub customer_ref: Option<&'a str>,
    pub customer_email: Option<&'a str>,
    pub metadata: Option<&'a str>,
    pub methods: Option<&'a str>,
    pub idempotency_key: Option<&'a str>,
    pub success_url: Option<&'a str>,
    pub cancel_url: Option<&'a str>,
}

pub async fn insert_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    p: &NewPayment<'_>,
) -> Result<()> {
    sqlx::query(
        "insert into payment (id, tenant_id, reference, currency, amount, tax, status, created, expires, description, customer_ref, customer_email, metadata, methods, idempotency_key, success_url, cancel_url) values (?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&p.id[..])
    .bind(p.tenant_id)
    .bind(p.reference)
    .bind(p.currency)
    .bind(p.amount)
    .bind(p.tax)
    .bind(p.created)
    .bind(p.expires)
    .bind(p.description)
    .bind(p.customer_ref)
    .bind(p.customer_email)
    .bind(p.metadata)
    .bind(p.methods)
    .bind(p.idempotency_key)
    .bind(p.success_url)
    .bind(p.cancel_url)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn get_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
) -> Result<Option<PaymentRow>> {
    Ok(
        sqlx::query_as(&format!("select {PAYMENT_COLS} from payment where id = ?"))
            .bind(&id[..])
            .fetch_optional(ex)
            .await?,
    )
}

pub async fn lock_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
) -> Result<Option<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where id = ? for update"
    ))
    .bind(&id[..])
    .fetch_optional(ex)
    .await?)
}

pub async fn payment_by_idempotency(
    pool: &MySqlPool,
    tenant_id: u64,
    key: &str,
) -> Result<Option<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where tenant_id = ? and idempotency_key = ?"
    ))
    .bind(tenant_id)
    .bind(key)
    .fetch_optional(pool)
    .await?)
}

pub async fn payments_by_reference(
    pool: &MySqlPool,
    tenant_id: u64,
    reference: &str,
) -> Result<Vec<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where tenant_id = ? and reference = ? order by created desc limit 100"
    ))
    .bind(tenant_id)
    .bind(reference)
    .fetch_all(pool)
    .await?)
}

pub async fn set_payment_status<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
    from: &[&str],
    to: &str,
) -> Result<bool> {
    let marks = vec!["?"; from.len()].join(",");
    let sql = format!("update payment set status = ? where id = ? and status in ({marks})");
    let mut q = sqlx::query(&sql).bind(to).bind(&id[..]);
    for f in from {
        q = q.bind(*f);
    }
    Ok(q.execute(ex).await?.rows_affected() > 0)
}

pub async fn mark_payment_paid<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
    at: DateTime<Utc>,
) -> Result<()> {
    sqlx::query("update payment set status = 'paid', paid_at = ? where id = ?")
        .bind(at)
        .bind(&id[..])
        .execute(ex)
        .await?;
    Ok(())
}

pub async fn due_pending_payments(pool: &MySqlPool, now: DateTime<Utc>) -> Result<Vec<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where status = 'pending' and expires <= ? order by expires limit 200"
    ))
    .bind(now)
    .fetch_all(pool)
    .await?)
}

pub struct NewAttempt<'a> {
    pub payment_id: PaymentId,
    pub provider_config_id: u64,
    pub watch_provider_id: u64,
    pub method: &'a str,
    pub option_key: &'a str,
    pub base_amount: u64,
    pub processing_fee: u64,
    pub currency: &'a str,
    pub charge_amount: u64,
    pub charge_currency: &'a str,
    pub rate: f64,
    pub lookup: &'a str,
    pub external_id: Option<&'a str>,
    pub details: &'a str,
    pub created: DateTime<Utc>,
    pub expires: DateTime<Utc>,
    pub save_method: bool,
}

pub async fn insert_attempt<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    a: &NewAttempt<'_>,
) -> Result<u64> {
    let r = sqlx::query(
        "insert into payment_attempt (payment_id, provider_config_id, watch_provider_id, method, option_key, base_amount, processing_fee, currency, charge_amount, charge_currency, rate, lookup, external_id, details, status, created, expires, save_method) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?, ?)",
    )
    .bind(&a.payment_id[..])
    .bind(a.provider_config_id)
    .bind(a.watch_provider_id)
    .bind(a.method)
    .bind(a.option_key)
    .bind(a.base_amount)
    .bind(a.processing_fee)
    .bind(a.currency)
    .bind(a.charge_amount)
    .bind(a.charge_currency)
    .bind(a.rate)
    .bind(a.lookup)
    .bind(a.external_id)
    .bind(a.details)
    .bind(a.created)
    .bind(a.expires)
    .bind(a.save_method)
    .execute(ex)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn get_attempt<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<Option<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where id = ?"
    ))
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn lock_attempt<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<Option<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where id = ? for update"
    ))
    .bind(id)
    .fetch_optional(ex)
    .await?)
}

pub async fn attempts_for_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
) -> Result<Vec<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where payment_id = ? order by id"
    ))
    .bind(&id[..])
    .fetch_all(ex)
    .await?)
}

pub async fn find_attempt_by_lookup(
    pool: &MySqlPool,
    watch_provider_id: u64,
    lookup: &str,
) -> Result<Option<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where watch_provider_id = ? and lookup = ? order by id desc limit 1"
    ))
    .bind(watch_provider_id)
    .bind(lookup)
    .fetch_optional(pool)
    .await?)
}

pub async fn set_attempt_status<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    from: &[&str],
    to: &str,
) -> Result<bool> {
    let marks = vec!["?"; from.len()].join(",");
    let sql = format!("update payment_attempt set status = ? where id = ? and status in ({marks})");
    let mut q = sqlx::query(&sql).bind(to).bind(id);
    for f in from {
        q = q.bind(*f);
    }
    Ok(q.execute(ex).await?.rows_affected() > 0)
}

pub async fn mark_attempt_paid<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    at: DateTime<Utc>,
) -> Result<()> {
    sqlx::query("update payment_attempt set status = 'paid', paid_at = ? where id = ?")
        .bind(at)
        .bind(id)
        .execute(ex)
        .await?;
    Ok(())
}

pub async fn due_pending_attempts(pool: &MySqlPool, now: DateTime<Utc>) -> Result<Vec<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where status = 'pending' and expires <= ? order by expires limit 200"
    ))
    .bind(now)
    .fetch_all(pool)
    .await?)
}

pub async fn upsert_receipt<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    attempt_id: u64,
    reference: &str,
    amount: u64,
    confirmed: bool,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "insert into attempt_receipt (attempt_id, ref, amount, confirmed, created) values (?, ?, ?, ?, ?) on duplicate key update confirmed = if(values(confirmed) = 1, 1, confirmed), amount = values(amount)",
    )
    .bind(attempt_id)
    .bind(reference)
    .bind(amount)
    .bind(confirmed)
    .bind(now)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn receipts_for_attempt<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    attempt_id: u64,
) -> Result<Vec<ReceiptRow>> {
    Ok(sqlx::query_as(
        "select id, attempt_id, ref, amount, confirmed from attempt_receipt where attempt_id = ? order by id",
    )
    .bind(attempt_id)
    .fetch_all(ex)
    .await?)
}

pub async fn get_cursor(pool: &MySqlPool, provider_id: u64) -> Result<Option<String>> {
    Ok(
        sqlx::query_scalar("select cursor_value from provider_cursor where provider_config_id = ?")
            .bind(provider_id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn set_cursor(pool: &MySqlPool, provider_id: u64, value: &str) -> Result<()> {
    sqlx::query(
        "insert into provider_cursor (provider_config_id, cursor_value, updated) values (?, ?, ?) on duplicate key update cursor_value = values(cursor_value), updated = values(updated)",
    )
    .bind(provider_id)
    .bind(value)
    .bind(Utc::now())
    .execute(pool)
    .await?;
    Ok(())
}

pub struct NewSavedMethod<'a> {
    pub tenant_id: u64,
    pub customer_ref: &'a str,
    pub provider_config_id: u64,
    pub external_customer_id: &'a str,
    pub external_id: &'a str,
    pub brand: Option<&'a str>,
    pub last4: Option<&'a str>,
    pub exp_month: Option<u16>,
    pub exp_year: Option<u16>,
}

pub async fn upsert_saved_method<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    m: &NewSavedMethod<'_>,
) -> Result<()> {
    sqlx::query(
        "insert into saved_method (tenant_id, customer_ref, provider_config_id, external_customer_id, external_id, brand, last4, exp_month, exp_year, created) values (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) on duplicate key update enabled = 1",
    )
    .bind(m.tenant_id)
    .bind(m.customer_ref)
    .bind(m.provider_config_id)
    .bind(m.external_customer_id)
    .bind(m.external_id)
    .bind(m.brand)
    .bind(m.last4)
    .bind(m.exp_month)
    .bind(m.exp_year)
    .bind(Utc::now())
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn list_saved_methods(
    pool: &MySqlPool,
    tenant_id: u64,
    customer_ref: &str,
) -> Result<Vec<SavedMethodRow>> {
    Ok(sqlx::query_as(&format!(
        "select {SAVED_COLS} from saved_method where tenant_id = ? and customer_ref = ? and enabled = 1 order by id"
    ))
    .bind(tenant_id)
    .bind(customer_ref)
    .fetch_all(pool)
    .await?)
}

pub async fn get_saved_method(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
) -> Result<Option<SavedMethodRow>> {
    Ok(sqlx::query_as(&format!(
        "select {SAVED_COLS} from saved_method where tenant_id = ? and id = ? and enabled = 1"
    ))
    .bind(tenant_id)
    .bind(id)
    .fetch_optional(pool)
    .await?)
}

pub async fn disable_saved_method(pool: &MySqlPool, tenant_id: u64, id: u64) -> Result<bool> {
    let r = sqlx::query("update saved_method set enabled = 0 where tenant_id = ? and id = ?")
        .bind(tenant_id)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn insert_refund<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    payment_id: &PaymentId,
    attempt_id: u64,
    amount: u64,
    charge_amount: u64,
    currency: &str,
    destination: Option<&str>,
) -> Result<u64> {
    let r = sqlx::query(
        "insert into refund (payment_id, attempt_id, amount, charge_amount, currency, destination, status, created) values (?, ?, ?, ?, ?, ?, 'pending', ?)",
    )
    .bind(&payment_id[..])
    .bind(attempt_id)
    .bind(amount)
    .bind(charge_amount)
    .bind(currency)
    .bind(destination)
    .bind(Utc::now())
    .execute(ex)
    .await?;
    Ok(r.last_insert_id())
}

pub async fn finish_refund<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
    status: &str,
    external_id: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    sqlx::query("update refund set status = ?, external_id = ?, error = ? where id = ?")
        .bind(status)
        .bind(external_id)
        .bind(error)
        .bind(id)
        .execute(ex)
        .await?;
    Ok(())
}

pub async fn refunds_for_payment<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: &PaymentId,
) -> Result<Vec<RefundRow>> {
    Ok(sqlx::query_as(&format!(
        "select {REFUND_COLS} from refund where payment_id = ? order by id"
    ))
    .bind(&id[..])
    .fetch_all(ex)
    .await?)
}

pub async fn get_refund<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    id: u64,
) -> Result<Option<RefundRow>> {
    Ok(
        sqlx::query_as(&format!("select {REFUND_COLS} from refund where id = ?"))
            .bind(id)
            .fetch_optional(ex)
            .await?,
    )
}

pub async fn insert_delivery<'e, E: Executor<'e, Database = MySql>>(
    ex: E,
    tenant_id: u64,
    payment_id: &PaymentId,
    event_id: &str,
    event_type: &str,
    body: &str,
    now: DateTime<Utc>,
) -> Result<()> {
    sqlx::query(
        "insert into webhook_delivery (tenant_id, payment_id, event_id, event_type, body, created, next_attempt) values (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(tenant_id)
    .bind(&payment_id[..])
    .bind(event_id)
    .bind(event_type)
    .bind(body)
    .bind(now)
    .bind(now)
    .execute(ex)
    .await?;
    Ok(())
}

pub async fn due_deliveries(pool: &MySqlPool, now: DateTime<Utc>) -> Result<Vec<DueDelivery>> {
    Ok(sqlx::query_as(
        "select id, tenant_id, event_id, body, attempts, created from webhook_delivery where delivered_at is null and abandoned_at is null and next_attempt <= ? order by next_attempt limit 50",
    )
    .bind(now)
    .fetch_all(pool)
    .await?)
}

pub async fn delivery_succeeded(pool: &MySqlPool, id: u64, now: DateTime<Utc>) -> Result<()> {
    sqlx::query(
        "update webhook_delivery set delivered_at = ?, attempts = attempts + 1, last_error = null where id = ?",
    )
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delivery_failed(
    pool: &MySqlPool,
    id: u64,
    next: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
    error: &str,
) -> Result<()> {
    let error: String = error.chars().take(500).collect();
    match next {
        Some(next) => {
            sqlx::query(
                "update webhook_delivery set attempts = attempts + 1, next_attempt = ?, last_error = ? where id = ?",
            )
            .bind(next)
            .bind(error)
            .bind(id)
            .execute(pool)
            .await?
        }
        None => {
            sqlx::query(
                "update webhook_delivery set attempts = attempts + 1, abandoned_at = ?, last_error = ? where id = ?",
            )
            .bind(now)
            .bind(error)
            .bind(id)
            .execute(pool)
            .await?
        }
    };
    Ok(())
}

pub async fn replay_delivery(pool: &MySqlPool, id: u64, now: DateTime<Utc>) -> Result<bool> {
    let r = sqlx::query(
        "update webhook_delivery set delivered_at = null, abandoned_at = null, next_attempt = ? where id = ?",
    )
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn list_deliveries(
    pool: &MySqlPool,
    tenant_id: Option<u64>,
    undelivered_only: bool,
) -> Result<Vec<DeliveryRow>> {
    let mut sql = String::from(
        "select id, tenant_id, event_id, event_type, attempts, created, next_attempt, delivered_at, abandoned_at, last_error from webhook_delivery where 1=1",
    );
    if tenant_id.is_some() {
        sql.push_str(" and tenant_id = ?");
    }
    if undelivered_only {
        sql.push_str(" and delivered_at is null");
    }
    sql.push_str(" order by id desc limit 200");
    let mut q = sqlx::query_as(&sql);
    if let Some(t) = tenant_id {
        q = q.bind(t);
    }
    Ok(q.fetch_all(pool).await?)
}

pub async fn delivery_tenant(pool: &MySqlPool, id: u64) -> Result<Option<u64>> {
    Ok(
        sqlx::query_scalar("select tenant_id from webhook_delivery where id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn has_unconfirmed_receipts(pool: &MySqlPool, id: &PaymentId) -> Result<bool> {
    let n: i64 = sqlx::query_scalar(
        "select count(*) from attempt_receipt r join payment_attempt a on a.id = r.attempt_id where a.payment_id = ? and r.confirmed = 0 and a.status = 'pending'",
    )
    .bind(&id[..])
    .fetch_one(pool)
    .await?;
    Ok(n > 0)
}

pub async fn claim_delivery(
    pool: &MySqlPool,
    id: u64,
    now: DateTime<Utc>,
    lease_secs: i64,
) -> Result<bool> {
    let r = sqlx::query(
        "update webhook_delivery set next_attempt = ? where id = ? and delivered_at is null and abandoned_at is null and next_attempt <= ?",
    )
    .bind(now + chrono::Duration::seconds(lease_secs))
    .bind(id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() == 1)
}

pub async fn tenant_by_name(pool: &MySqlPool, name: &str) -> Result<Option<Tenant>> {
    Ok(
        sqlx::query_as(&format!("select {TENANT_COLS} from tenant where name = ?"))
            .bind(name)
            .fetch_optional(pool)
            .await?,
    )
}

pub async fn recent_payments(
    pool: &MySqlPool,
    tenant_id: u64,
    limit: u32,
) -> Result<Vec<PaymentRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PAYMENT_COLS} from payment where tenant_id = ? order by created desc limit ?"
    ))
    .bind(tenant_id)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}

pub async fn pending_attempts_of_provider(
    pool: &MySqlPool,
    provider_id: u64,
) -> Result<Vec<AttemptRow>> {
    Ok(sqlx::query_as(&format!(
        "select {ATTEMPT_COLS} from payment_attempt where provider_config_id = ? and status = 'pending' order by id limit 500"
    ))
    .bind(provider_id)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct StatRow {
    pub status: String,
    pub currency: String,
    pub count: i64,
    pub total: Option<i64>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct DayRow {
    pub day: String,
    pub currency: String,
    pub count: i64,
    pub total: Option<i64>,
}

pub async fn list_payments(
    pool: &MySqlPool,
    tenant_id: u64,
    status: Option<&str>,
    query: Option<&str>,
    before: Option<(DateTime<Utc>, Vec<u8>)>,
    limit: u32,
) -> Result<Vec<PaymentRow>> {
    let mut sql = format!("select {PAYMENT_COLS} from payment where tenant_id = ?");
    if status.is_some() {
        sql.push_str(" and status = ?");
    }
    if query.is_some() {
        sql.push_str(" and (reference like ? or hex(id) like ?)");
    }
    if before.is_some() {
        sql.push_str(" and (created < ? or (created = ? and id < ?))");
    }
    sql.push_str(" order by created desc, id desc limit ?");
    let mut q = sqlx::query_as(&sql).bind(tenant_id);
    if let Some(s) = status {
        q = q.bind(s);
    }
    if let Some(text) = query {
        let escaped = text
            .replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_");
        q = q
            .bind(format!("{escaped}%"))
            .bind(format!("{}%", escaped.to_uppercase()));
    }
    if let Some((at, id)) = before {
        q = q.bind(at).bind(at).bind(id);
    }
    Ok(q.bind(limit).fetch_all(pool).await?)
}

pub async fn payment_stats(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<StatRow>> {
    Ok(sqlx::query_as(
        "select status, currency, count(*) as count, cast(sum(amount) as signed) as total from payment where tenant_id = ? and created >= ? group by status, currency order by status, currency",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

pub async fn paid_by_day(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<DayRow>> {
    Ok(sqlx::query_as(
        "select cast(date(paid_at) as char) as day, currency, count(*) as count, cast(sum(amount) as signed) as total from payment where tenant_id = ? and paid_at >= ? and status in ('paid','partially_refunded','refunded') group by date(paid_at), currency order by date(paid_at)",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

pub async fn method_counts(
    pool: &MySqlPool,
    tenant_id: u64,
    since: DateTime<Utc>,
) -> Result<Vec<(String, i64)>> {
    Ok(sqlx::query_as(
        "select a.method, count(*) from payment_attempt a join payment p on p.id = a.payment_id where p.tenant_id = ? and a.status = 'paid' and a.paid_at >= ? group by a.method order by 2 desc",
    )
    .bind(tenant_id)
    .bind(since)
    .fetch_all(pool)
    .await?)
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct DeliveryDetail {
    pub id: u64,
    pub event_id: String,
    pub event_type: String,
    pub attempts: u32,
    pub created: DateTime<Utc>,
    pub next_attempt: DateTime<Utc>,
    pub delivered_at: Option<DateTime<Utc>>,
    pub abandoned_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub body: String,
}

pub async fn deliveries_for_tenant(
    pool: &MySqlPool,
    tenant_id: u64,
    undelivered_only: bool,
) -> Result<Vec<DeliveryDetail>> {
    let extra = if undelivered_only {
        " and delivered_at is null"
    } else {
        ""
    };
    Ok(sqlx::query_as(&format!(
        "select id, event_id, event_type, attempts, created, next_attempt, delivered_at, abandoned_at, last_error, body from webhook_delivery where tenant_id = ?{extra} order by id desc limit 100"
    ))
    .bind(tenant_id)
    .fetch_all(pool)
    .await?)
}

pub async fn replay_tenant_delivery(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
    now: DateTime<Utc>,
) -> Result<bool> {
    let r = sqlx::query(
        "update webhook_delivery set delivered_at = null, abandoned_at = null, next_attempt = ? where id = ? and tenant_id = ?",
    )
    .bind(now)
    .bind(id)
    .bind(tenant_id)
    .execute(pool)
    .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn get_tenant_provider(
    pool: &MySqlPool,
    tenant_id: u64,
    id: u64,
) -> Result<Option<ProviderConfigRow>> {
    Ok(sqlx::query_as(&format!(
        "select {PROVIDER_COLS} from provider_config where id = ? and tenant_id = ?"
    ))
    .bind(id)
    .bind(tenant_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn update_provider_fees(
    pool: &MySqlPool,
    id: u64,
    name: &str,
    fee_rate: Option<f64>,
    fee_base: Option<u64>,
    fee_currency: Option<&str>,
) -> Result<()> {
    sqlx::query("update provider_config set name = ?, fee_rate = ?, fee_base = ?, fee_currency = ? where id = ?")
        .bind(name)
        .bind(fee_rate)
        .bind(fee_base)
        .bind(fee_currency)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn provider_attempt_count(pool: &MySqlPool, id: u64) -> Result<i64> {
    Ok(sqlx::query_scalar("select count(*) from payment_attempt where provider_config_id = ? or watch_provider_id = ?")
        .bind(id)
        .bind(id)
        .fetch_one(pool)
        .await?)
}

pub async fn delete_provider(pool: &MySqlPool, tenant_id: u64, id: u64) -> Result<bool> {
    sqlx::query("delete from provider_cursor where provider_config_id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    let r = sqlx::query("delete from provider_config where id = ? and tenant_id = ?")
        .bind(id)
        .bind(tenant_id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn set_tenant_trusted(pool: &MySqlPool, id: u64, trusted: bool) -> Result<bool> {
    let r = sqlx::query("update tenant set trusted = ? where id = ?")
        .bind(trusted)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(r.rows_affected() > 0)
}

pub async fn count_all<'e, E: Executor<'e, Database = MySql>>(ex: E, table: &str) -> Result<i64> {
    Ok(sqlx::query_scalar(&format!("select count(*) from {table}"))
        .fetch_one(ex)
        .await?)
}

pub async fn deliveries_for_payment(pool: &MySqlPool, id: &PaymentId) -> Result<Vec<DeliveryRow>> {
    Ok(sqlx::query_as(
        "select id, tenant_id, event_id, event_type, attempts, created, next_attempt, delivered_at, abandoned_at, last_error from webhook_delivery where payment_id = ? order by id",
    )
    .bind(&id[..])
    .fetch_all(pool)
    .await?)
}

pub async fn pending_delivery_count(pool: &MySqlPool, tenant_id: u64) -> Result<i64> {
    Ok(sqlx::query_scalar(
        "select count(*) from webhook_delivery where tenant_id = ? and delivered_at is null and abandoned_at is null",
    )
    .bind(tenant_id)
    .fetch_one(pool)
    .await?)
}
