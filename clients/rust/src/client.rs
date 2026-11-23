use crate::types::*;
use reqwest::{Method, StatusCode};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::marker::PhantomData;
use std::time::Duration;

#[derive(Debug)]
pub enum Error {
    Api {
        status: u16,
        code: String,
        message: String,
        field: Option<String>,
        retry_after: Option<u64>,
        body: Value,
    },
    Network(reqwest::Error),
    Decode(serde_json::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Api {
                status,
                code,
                message,
                ..
            } => write!(f, "gateway error {status} {code}: {message}"),
            Error::Network(e) => write!(f, "network error: {e}"),
            Error::Decode(e) => write!(f, "decode error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl Error {
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    pub fn code(&self) -> Option<&str> {
        match self {
            Error::Api { code, .. } => Some(code),
            _ => None,
        }
    }
}

#[derive(Clone)]
pub struct Gateway {
    base_url: String,
    api_key: String,
    http: reqwest::Client,
    max_retries: u32,
    retry_base_delay: Duration,
    retry_max_delay: Duration,
}

pub fn new_idempotency_key() -> String {
    let mut bytes = [0u8; 16];
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mix = seed ^ ((n as u128) << 64) ^ ((std::process::id() as u128) << 96);
    bytes.copy_from_slice(&mix.to_le_bytes());
    let digest = <sha2::Sha256 as sha2::Digest>::digest(bytes);
    hex::encode(&digest[..16])
}

struct Reply {
    body: Value,
    next_cursor: Option<String>,
}

impl Gateway {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .user_agent(concat!("payment-gateway-rust/", env!("CARGO_PKG_VERSION")))
                .build()
                .expect("http client"),
            max_retries: 3,
            retry_base_delay: Duration::from_millis(250),
            retry_max_delay: Duration::from_secs(8),
        }
    }

    pub fn with_retries(
        mut self,
        max_retries: u32,
        base_delay: Duration,
        max_delay: Duration,
    ) -> Self {
        self.max_retries = max_retries;
        self.retry_base_delay = base_delay;
        self.retry_max_delay = max_delay;
        self
    }

    pub fn with_http_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    fn delay(&self, attempt: u32, retry_after: Option<u64>) -> Duration {
        if let Some(s) = retry_after {
            return Duration::from_secs(s).min(self.retry_max_delay);
        }
        let exp = (self.retry_base_delay * 2u32.saturating_pow(attempt)).min(self.retry_max_delay);
        let jitter = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0) as u64)
            % 1000;
        exp / 2 + exp / 2 * jitter as u32 / 1000
    }

    async fn send(
        &self,
        method: Method,
        path: &str,
        query: &[(String, String)],
        body: Option<Vec<u8>>,
        idempotency_key: Option<&str>,
    ) -> Result<Reply, Error> {
        let mut attempt = 0;
        loop {
            let mut req = self
                .http
                .request(method.clone(), format!("{}{}", self.base_url, path))
                .bearer_auth(&self.api_key)
                .header("accept", "application/json");
            if !query.is_empty() {
                req = req.query(query);
            }
            if let Some(b) = &body {
                req = req
                    .header("content-type", "application/json")
                    .body(b.clone());
            }
            if let Some(k) = idempotency_key {
                req = req.header("idempotency-key", k);
            }
            let idempotent =
                method == Method::GET || method == Method::DELETE || idempotency_key.is_some();
            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => {
                    if idempotent && attempt < self.max_retries {
                        tokio::time::sleep(self.delay(attempt, None)).await;
                        attempt += 1;
                        continue;
                    }
                    return Err(Error::Network(e));
                }
            };
            let status = response.status();
            let next_cursor = response
                .headers()
                .get("x-next-cursor")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok());
            let bytes = response.bytes().await.map_err(Error::Network)?;
            let body_json: Value = if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes)
                    .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
            };
            if status.is_success() {
                return Ok(Reply {
                    body: body_json,
                    next_cursor,
                });
            }
            let retryable = status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error();
            let safe = idempotent || status == StatusCode::TOO_MANY_REQUESTS;
            if retryable && safe && attempt < self.max_retries {
                tokio::time::sleep(self.delay(attempt, retry_after)).await;
                attempt += 1;
                continue;
            }
            let detail = &body_json["error_detail"];
            return Err(Error::Api {
                status: status.as_u16(),
                code: detail["code"]
                    .as_str()
                    .or_else(|| body_json["code"].as_str())
                    .unwrap_or("error")
                    .to_string(),
                message: detail["message"]
                    .as_str()
                    .or_else(|| body_json["error"].as_str())
                    .unwrap_or("request failed")
                    .to_string(),
                field: detail["field"].as_str().map(str::to_string),
                retry_after,
                body: body_json,
            });
        }
    }

    async fn call<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&impl Serialize>,
        idempotency_key: Option<&str>,
    ) -> Result<T, Error> {
        let payload = body
            .map(|b| serde_json::to_vec(b).map_err(Error::Decode))
            .transpose()?;
        let reply = self
            .send(method, path, &[], payload, idempotency_key)
            .await?;
        serde_json::from_value(reply.body).map_err(Error::Decode)
    }

    pub async fn create_payment(&self, input: &CreatePayment) -> Result<Payment, Error> {
        self.create_payment_with_key(input, &new_idempotency_key())
            .await
    }

    pub async fn create_payment_with_key(
        &self,
        input: &CreatePayment,
        key: &str,
    ) -> Result<Payment, Error> {
        self.call(Method::POST, "/v1/payments", Some(input), Some(key))
            .await
    }

    pub async fn get_payment(&self, id: &str) -> Result<Payment, Error> {
        self.call(
            Method::GET,
            &format!("/v1/payments/{id}"),
            None::<&()>,
            None,
        )
        .await
    }

    pub async fn create_attempt(&self, id: &str, input: &CreateAttempt) -> Result<Payment, Error> {
        self.call(
            Method::POST,
            &format!("/v1/payments/{id}/attempts"),
            Some(input),
            None,
        )
        .await
    }

    pub async fn cancel_payment(&self, id: &str) -> Result<Payment, Error> {
        self.call(
            Method::POST,
            &format!("/v1/payments/{id}/cancel"),
            Some(&serde_json::json!({})),
            None,
        )
        .await
    }

    pub async fn refund_payment(&self, id: &str, input: &RefundInput) -> Result<Refund, Error> {
        self.call(
            Method::POST,
            &format!("/v1/payments/{id}/refund"),
            Some(input),
            None,
        )
        .await
    }

    pub async fn import_saved_method(
        &self,
        input: &ImportSavedMethod,
    ) -> Result<SavedMethod, Error> {
        self.call(Method::POST, "/v1/saved-methods", Some(input), None)
            .await
    }

    pub async fn delete_saved_method(&self, id: u64) -> Result<(), Error> {
        self.send(
            Method::DELETE,
            &format!("/v1/saved-methods/{id}"),
            &[],
            None,
            None,
        )
        .await
        .map(|_| ())
    }

    pub async fn charge_saved_method(
        &self,
        id: u64,
        input: &CreatePayment,
    ) -> Result<Payment, Error> {
        self.charge_saved_method_with_key(id, input, &new_idempotency_key())
            .await
    }

    pub async fn charge_saved_method_with_key(
        &self,
        id: u64,
        input: &CreatePayment,
        idempotency_key: &str,
    ) -> Result<Payment, Error> {
        self.call(
            Method::POST,
            &format!("/v1/saved-methods/{id}/charge"),
            Some(input),
            Some(idempotency_key),
        )
        .await
    }

    pub fn list_payments(&self, filter: &ListPayments) -> Pager<Payment> {
        Pager::new(self.clone(), "/v1/payments", filter.query())
    }

    pub async fn create_withdrawal(&self, input: &CreateWithdrawal) -> Result<Withdrawal, Error> {
        self.create_withdrawal_with_key(input, &new_idempotency_key())
            .await
    }

    pub async fn create_withdrawal_with_key(
        &self,
        input: &CreateWithdrawal,
        key: &str,
    ) -> Result<Withdrawal, Error> {
        self.call(Method::POST, "/v1/withdrawals", Some(input), Some(key))
            .await
    }

    pub async fn get_withdrawal(&self, id: u64) -> Result<Withdrawal, Error> {
        self.call(
            Method::GET,
            &format!("/v1/withdrawals/{id}"),
            None::<&()>,
            None,
        )
        .await
    }

    pub fn list_withdrawals(&self) -> Pager<Withdrawal> {
        Pager::new(self.clone(), "/v1/withdrawals", vec![])
    }

    pub fn list_refunds(&self) -> Pager<Refund> {
        Pager::new(self.clone(), "/v1/refunds", vec![])
    }

    pub fn list_events(&self) -> Pager<Value> {
        Pager::new(self.clone(), "/v1/events", vec![])
    }

    pub fn list_methods(&self) -> Pager<Value> {
        Pager::new(self.clone(), "/v1/methods", vec![])
    }

    pub fn list_saved_methods(&self, customer_ref: Option<&str>) -> Pager<SavedMethod> {
        let query = customer_ref
            .map(|c| vec![("customer_ref".to_string(), c.to_string())])
            .unwrap_or_default();
        Pager::new(self.clone(), "/v1/saved-methods", query)
    }
}

pub struct Pager<T> {
    gateway: Gateway,
    path: &'static str,
    query: Vec<(String, String)>,
    buffer: std::collections::VecDeque<T>,
    cursor: Option<String>,
    started: bool,
    done: bool,
    _marker: PhantomData<T>,
}

impl<T: DeserializeOwned> Pager<T> {
    fn new(gateway: Gateway, path: &'static str, query: Vec<(String, String)>) -> Self {
        Self {
            gateway,
            path,
            query,
            buffer: Default::default(),
            cursor: None,
            started: false,
            done: false,
            _marker: PhantomData,
        }
    }

    pub async fn next(&mut self) -> Result<Option<T>, Error> {
        loop {
            if let Some(item) = self.buffer.pop_front() {
                return Ok(Some(item));
            }
            if self.done {
                return Ok(None);
            }
            let mut query = self.query.clone();
            if let Some(c) = &self.cursor {
                query.push(("cursor".into(), c.clone()));
            }
            let reply = self
                .gateway
                .send(Method::GET, self.path, &query, None, None)
                .await?;
            let items: Vec<T> = serde_json::from_value(reply.body).map_err(Error::Decode)?;
            self.started = true;
            self.buffer.extend(items);
            self.cursor = reply.next_cursor;
            self.done = self.cursor.is_none();
        }
    }

    pub async fn collect(mut self) -> Result<Vec<T>, Error> {
        let mut all = vec![];
        while let Some(item) = self.next().await? {
            all.push(item);
        }
        Ok(all)
    }
}
