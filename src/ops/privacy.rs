use anyhow::{Result, ensure};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sqlx::{FromRow, MySqlPool};

#[derive(Clone, Debug)]
pub enum Subject {
    Email(String),
    CustomerRef(String),
}

impl Subject {
    fn clause(&self) -> (&'static str, &str) {
        match self {
            Subject::Email(v) => ("customer_email = ?", v),
            Subject::CustomerRef(v) => ("customer_ref = ?", v),
        }
    }
}

#[derive(Debug, Serialize, FromRow)]
pub struct ExportedPayment {
    pub id: String,
    pub reference: String,
    pub currency: String,
    pub amount: u64,
    pub status: String,
    pub created: DateTime<Utc>,
    pub description: Option<String>,
    pub customer_ref: Option<String>,
    pub customer_email: Option<String>,
    pub metadata: Option<String>,
}

#[derive(Debug, Serialize, FromRow)]
pub struct ExportedSavedMethod {
    pub id: u64,
    pub customer_ref: String,
    pub brand: Option<String>,
    pub last4: Option<String>,
    pub created: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
pub struct Export {
    pub tenant_id: u64,
    pub payments: Vec<ExportedPayment>,
    pub saved_methods: Vec<ExportedSavedMethod>,
}

#[derive(Debug, Default, Serialize, PartialEq, Eq)]
pub struct DeleteReport {
    pub payments_anonymised: u64,
    pub delivery_bodies_cleared: u64,
    pub saved_methods_deleted: u64,
}

pub async fn export(pool: &MySqlPool, tenant_id: u64, subject: &Subject) -> Result<Export> {
    let (clause, value) = subject.clause();
    let payments = sqlx::query_as(&format!(
        "select lower(hex(id)) as id, reference, currency, amount, status, created, description, customer_ref, customer_email, metadata from payment where tenant_id = ? and {clause} order by created"
    ))
    .bind(tenant_id)
    .bind(value)
    .fetch_all(pool)
    .await?;
    let saved_methods = match subject {
        Subject::CustomerRef(r) => {
            sqlx::query_as("select id, customer_ref, brand, last4, created from saved_method where tenant_id = ? and customer_ref = ? order by id")
                .bind(tenant_id)
                .bind(r)
                .fetch_all(pool)
                .await?
        }
        Subject::Email(_) => vec![],
    };
    Ok(Export {
        tenant_id,
        payments,
        saved_methods,
    })
}

pub fn export_json(e: &Export) -> Result<Value> {
    Ok(serde_json::to_value(e)?)
}

pub async fn delete(pool: &MySqlPool, tenant_id: u64, subject: &Subject) -> Result<DeleteReport> {
    let (clause, value) = subject.clause();
    ensure!(!value.trim().is_empty(), "subject must not be empty");
    let mut tx = pool.begin().await?;
    let ids: Vec<Vec<u8>> = sqlx::query_scalar(&format!(
        "select id from payment where tenant_id = ? and {clause} for update"
    ))
    .bind(tenant_id)
    .bind(value)
    .fetch_all(&mut *tx)
    .await?;
    let mut report = DeleteReport::default();
    for id in &ids {
        let r = sqlx::query(
            "update webhook_delivery set body = '' where payment_id = ? and tenant_id = ? and body <> '' and (delivered_at is not null or abandoned_at is not null)",
        )
        .bind(id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
        report.delivery_bodies_cleared += r.rows_affected();
        let r = sqlx::query(
            "update payment set customer_email = null, customer_ref = null, metadata = null where id = ? and tenant_id = ?",
        )
        .bind(id)
        .bind(tenant_id)
        .execute(&mut *tx)
        .await?;
        report.payments_anonymised += r.rows_affected();
    }
    if let Subject::CustomerRef(r) = subject {
        let r = sqlx::query("delete from saved_method where tenant_id = ? and customer_ref = ?")
            .bind(tenant_id)
            .bind(r)
            .execute(&mut *tx)
            .await?;
        report.saved_methods_deleted = r.rows_affected();
    }
    tx.commit().await?;
    Ok(report)
}
