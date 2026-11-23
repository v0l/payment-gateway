use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
pub struct Attempt {
    pub id: u64,
    pub provider_id: u64,
    pub method: String,
    #[serde(default)]
    pub option: Option<String>,
    pub status: String,
    #[serde(default)]
    pub amount: u64,
    #[serde(default)]
    pub currency: String,
    #[serde(default)]
    pub details: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Refund {
    pub id: u64,
    pub attempt_id: u64,
    pub amount: u64,
    pub currency: String,
    pub status: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub payment_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Payment {
    pub id: String,
    pub reference: String,
    pub amount: u64,
    pub currency: String,
    #[serde(default)]
    pub tax: u64,
    pub status: String,
    #[serde(default)]
    pub expires: Option<String>,
    #[serde(default)]
    pub paid_at: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub customer_ref: Option<String>,
    #[serde(default)]
    pub metadata: Value,
    pub pay_url: String,
    #[serde(default)]
    pub line_items: Vec<Value>,
    #[serde(default)]
    pub attempts: Vec<Attempt>,
    #[serde(default)]
    pub refunds: Vec<Refund>,
    #[serde(default)]
    pub methods: Vec<Value>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct LineItemInput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quantity: Option<u64>,
    pub unit_amount: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tax_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tax_amount: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sku: Option<String>,
}

impl LineItemInput {
    pub fn new(name: &str, unit_amount: u64) -> Self {
        Self {
            name: name.to_string(),
            unit_amount,
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SavedMethod {
    pub id: u64,
    pub customer_ref: String,
    pub provider_id: u64,
    #[serde(default)]
    pub brand: Option<String>,
    #[serde(default)]
    pub last4: Option<String>,
    #[serde(default)]
    pub exp_month: Option<u16>,
    #[serde(default)]
    pub exp_year: Option<u16>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CreatePayment {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
    pub amount: u64,
    pub currency: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tax: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_items: Option<Vec<LineItemInput>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_in: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub methods: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancel_url: Option<String>,
}

impl CreatePayment {
    pub fn new(amount: u64, currency: &str) -> Self {
        Self {
            amount,
            currency: currency.to_string(),
            ..Default::default()
        }
    }

    pub fn reference(mut self, reference: &str) -> Self {
        self.reference = Some(reference.to_string());
        self
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CreateAttempt {
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub option: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub save_method: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_method_id: Option<u64>,
}

impl CreateAttempt {
    pub fn new(method: &str) -> Self {
        Self {
            method: method.to_string(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct CreateWithdrawal {
    pub amount: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination_id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Withdrawal {
    pub id: u64,
    pub amount: u64,
    pub currency: String,
    pub status: String,
    #[serde(default)]
    pub fee: Option<u64>,
    #[serde(default)]
    pub external_id: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub destination_kind: String,
    #[serde(default)]
    pub destination_label: String,
    #[serde(default)]
    pub idempotency_key: Option<String>,
    #[serde(default)]
    pub provider_id: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RefundInput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destination: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct ListPayments {
    pub status: Option<String>,
    pub reference: Option<String>,
    pub customer_ref: Option<String>,
    pub limit: Option<u32>,
}

impl ListPayments {
    pub(crate) fn query(&self) -> Vec<(String, String)> {
        let mut q = vec![];
        if let Some(v) = &self.status {
            q.push(("status".into(), v.clone()));
        }
        if let Some(v) = &self.reference {
            q.push(("reference".into(), v.clone()));
        }
        if let Some(v) = &self.customer_ref {
            q.push(("customer_ref".into(), v.clone()));
        }
        if let Some(v) = self.limit {
            q.push(("limit".into(), v.to_string()));
        }
        q
    }
}
