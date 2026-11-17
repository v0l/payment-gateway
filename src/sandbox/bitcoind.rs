use super::Bitcoind;
use anyhow::{Result, bail};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::time::Duration;

pub struct BitcoindRpc {
    client: reqwest::Client,
    url: String,
    user: String,
    pass: String,
    wallet: String,
}

impl BitcoindRpc {
    pub fn new(url: &str, user: &str, pass: &str, wallet: &str) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()?,
            url: url.trim_end_matches('/').to_string(),
            user: user.to_string(),
            pass: pass.to_string(),
            wallet: wallet.to_string(),
        })
    }

    async fn call(&self, in_wallet: bool, method: &str, params: Value) -> Result<Value> {
        let url = if in_wallet {
            format!("{}/wallet/{}", self.url, self.wallet)
        } else {
            self.url.clone()
        };
        let rsp = self
            .client
            .post(url)
            .basic_auth(&self.user, Some(&self.pass))
            .json(&json!({"jsonrpc": "1.0", "id": "pgw", "method": method, "params": params}))
            .send()
            .await?;
        let status = rsp.status();
        let body: Value = rsp.json().await.unwrap_or(Value::Null);
        if let Some(err) = body.get("error").filter(|e| !e.is_null()) {
            bail!(
                "bitcoind {method}: {}",
                err["message"].as_str().unwrap_or("error")
            );
        }
        if !status.is_success() {
            bail!("bitcoind {method}: http {status}");
        }
        Ok(body["result"].clone())
    }
}

pub fn btc_literal(sats: u64) -> Value {
    serde_json::from_str(&format!("{}.{:08}", sats / 100_000_000, sats % 100_000_000))
        .expect("decimal literal")
}

pub fn btc_to_sats(btc: f64) -> u64 {
    (btc * 100_000_000.0).round() as u64
}

#[async_trait]
impl Bitcoind for BitcoindRpc {
    async fn ensure_wallet(&self) -> Result<()> {
        let loaded = self.call(false, "listwallets", json!([])).await?;
        if loaded
            .as_array()
            .is_some_and(|w| w.iter().any(|n| n.as_str() == Some(self.wallet.as_str())))
        {
            return Ok(());
        }
        if self
            .call(false, "loadwallet", json!([self.wallet]))
            .await
            .is_ok()
        {
            return Ok(());
        }
        self.call(false, "createwallet", json!([self.wallet]))
            .await?;
        Ok(())
    }

    async fn new_address(&self) -> Result<String> {
        let a = self.call(true, "getnewaddress", json!([])).await?;
        Ok(a.as_str().unwrap_or_default().to_string())
    }

    async fn mine(&self, blocks: u32, to: &str) -> Result<u64> {
        self.call(false, "generatetoaddress", json!([blocks, to]))
            .await?;
        self.height().await
    }

    async fn send(&self, address: &str, sats: u64) -> Result<String> {
        let tx = self
            .call(true, "sendtoaddress", json!([address, btc_literal(sats)]))
            .await?;
        Ok(tx.as_str().unwrap_or_default().to_string())
    }

    async fn height(&self) -> Result<u64> {
        Ok(self
            .call(false, "getblockcount", json!([]))
            .await?
            .as_u64()
            .unwrap_or(0))
    }

    async fn balance_sats(&self) -> Result<u64> {
        let v = self.call(true, "getbalance", json!([])).await?;
        Ok(btc_to_sats(v.as_f64().unwrap_or(0.0)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_are_exact_decimals() {
        assert_eq!(btc_literal(100_000_000).to_string(), "1.0");
        assert_eq!(btc_literal(123_456).to_string(), "0.00123456");
        assert_eq!(btc_literal(1).to_string(), "1e-8");
        assert_eq!(btc_to_sats(0.00123456), 123_456);
        assert_eq!(btc_to_sats(50.0), 5_000_000_000);
    }
}
