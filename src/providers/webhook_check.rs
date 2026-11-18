use anyhow::{Result, anyhow, bail};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::collections::HashMap;

pub const REPLAY_WINDOW_SECS: i64 = 300;

pub fn within_window(timestamp_secs: i64, now_secs: i64) -> bool {
    (now_secs - timestamp_secs).abs() <= REPLAY_WINDOW_SECS
}

pub fn revolut_verify(
    secret: &str,
    headers: &HashMap<String, String>,
    body: &[u8],
) -> Result<Value> {
    let timestamp = headers
        .get("revolut-request-timestamp")
        .ok_or_else(|| anyhow!("missing timestamp header"))?;
    let signature = headers
        .get("revolut-signature")
        .ok_or_else(|| anyhow!("missing signature header"))?;
    let ts_ms: i64 = timestamp
        .parse()
        .map_err(|_| anyhow!("bad timestamp header"))?;
    if !within_window(ts_ms / 1000, chrono::Utc::now().timestamp()) {
        bail!("timestamp outside replay window");
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())?;
    mac.update(b"v1.");
    mac.update(timestamp.as_bytes());
    mac.update(b".");
    mac.update(body);
    let expected = hex::encode(mac.finalize().into_bytes());
    let ok = signature
        .split(',')
        .filter_map(|s| s.trim().strip_prefix("v1="))
        .any(|s| {
            bool::from(subtle::ConstantTimeEq::ct_eq(
                s.as_bytes(),
                expected.as_bytes(),
            ))
        });
    if !ok {
        bail!("signature mismatch");
    }
    Ok(serde_json::from_slice(body)?)
}
