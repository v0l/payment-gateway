use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::time::{SystemTime, UNIX_EPOCH};
use subtle::ConstantTimeEq;

pub const SIGNATURE_HEADER: &str = "x-gateway-signature";
pub const DEFAULT_TOLERANCE_SECONDS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebhookError {
    MissingHeader,
    MalformedHeader,
    SignatureMismatch,
    TimestampOutsideTolerance,
    InvalidBody,
}

impl std::fmt::Display for WebhookError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::MissingHeader => "missing x-gateway-signature header",
            Self::MalformedHeader => "malformed signature header",
            Self::SignatureMismatch => "signature does not match",
            Self::TimestampOutsideTolerance => "timestamp outside tolerance",
            Self::InvalidBody => "body is not valid json",
        })
    }
}

impl std::error::Error for WebhookError {}

pub fn sign_webhook(raw_body: &[u8], secret: &str, timestamp: i64) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key length");
    mac.update(format!("{timestamp}.").as_bytes());
    mac.update(raw_body);
    hex::encode(mac.finalize().into_bytes())
}

fn parse_header(header: &str) -> Result<(i64, Vec<&str>), WebhookError> {
    let mut timestamp = None;
    let mut signatures = vec![];
    for part in header.split(',') {
        match part.trim().split_once('=') {
            Some(("t", v)) if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) => {
                timestamp = v.parse().ok();
            }
            Some(("v1", v)) if !v.is_empty() => signatures.push(v),
            _ => {}
        }
    }
    match timestamp {
        Some(t) if !signatures.is_empty() => Ok((t, signatures)),
        _ => Err(WebhookError::MalformedHeader),
    }
}

pub fn verify_webhook_at(
    raw_body: &[u8],
    signature_header: Option<&str>,
    secret: &str,
    tolerance_seconds: i64,
    now: i64,
) -> Result<Value, WebhookError> {
    let header = signature_header.ok_or(WebhookError::MissingHeader)?;
    let (timestamp, signatures) = parse_header(header)?;
    let expected = sign_webhook(raw_body, secret, timestamp);
    let mut matched = false;
    for candidate in signatures {
        if bool::from(candidate.as_bytes().ct_eq(expected.as_bytes())) {
            matched = true;
        }
    }
    if !matched {
        return Err(WebhookError::SignatureMismatch);
    }
    if tolerance_seconds > 0 && (now - timestamp).abs() > tolerance_seconds {
        return Err(WebhookError::TimestampOutsideTolerance);
    }
    serde_json::from_slice(raw_body).map_err(|_| WebhookError::InvalidBody)
}

pub fn verify_webhook(
    raw_body: &[u8],
    signature_header: Option<&str>,
    secret: &str,
    tolerance_seconds: i64,
) -> Result<Value, WebhookError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    verify_webhook_at(raw_body, signature_header, secret, tolerance_seconds, now)
}
