use anyhow::{Result, bail};
use bitcoin::Network;
use sha2::{Digest, Sha256};
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DestinationKind {
    LightningAddress,
    Lnurl,
    Onchain,
    Bolt11,
}

impl DestinationKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LightningAddress => "lightning_address",
            Self::Lnurl => "lnurl",
            Self::Onchain => "onchain",
            Self::Bolt11 => "bolt11",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "lightning_address" => Self::LightningAddress,
            "lnurl" => Self::Lnurl,
            "onchain" => Self::Onchain,
            "bolt11" => Self::Bolt11,
            _ => return None,
        })
    }

    pub fn storable(&self) -> bool {
        !matches!(self, Self::Bolt11)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub kind: DestinationKind,
    pub value: String,
}

impl Destination {
    pub fn hash(&self) -> String {
        hex::encode(Sha256::digest(
            format!("{}:{}", self.kind.as_str(), self.value).as_bytes(),
        ))
    }
}

pub fn normalize(kind: DestinationKind, raw: &str, network: Network) -> Result<Destination> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > 1500 {
        bail!("destination is empty or too long");
    }
    let value = match kind {
        DestinationKind::LightningAddress => {
            let v = raw.to_ascii_lowercase();
            let Some((user, domain)) = v.split_once('@') else {
                bail!("a lightning address looks like name@domain");
            };
            let user_ok = !user.is_empty()
                && user
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+'));
            let domain_ok = !domain.is_empty()
                && domain.len() <= 253
                && domain
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':'));
            if !user_ok || !domain_ok {
                bail!("a lightning address looks like name@domain");
            }
            v
        }
        DestinationKind::Lnurl => {
            let v = raw
                .trim_start_matches("lightning:")
                .trim()
                .to_ascii_lowercase();
            super::lnurl::decode_lnurl(&v)?;
            v
        }
        DestinationKind::Onchain => {
            let addr = bitcoin::Address::from_str(raw)
                .map_err(|e| anyhow::anyhow!("invalid bitcoin address: {e}"))?
                .require_network(network)
                .map_err(|_| anyhow::anyhow!("address is for a different bitcoin network"))?;
            addr.to_string()
        }
        DestinationKind::Bolt11 => {
            bail!("a bolt11 invoice is supplied per withdrawal, not stored")
        }
    };
    Ok(Destination { kind, value })
}

pub fn signed_by_address(address: &str, message: &str, signature_b64: &str) -> bool {
    use bitcoin::secp256k1::Secp256k1;
    use bitcoin::sign_message::{MessageSignature, signed_msg_hash};
    let Ok(addr) = bitcoin::Address::from_str(address) else {
        return false;
    };
    let addr = addr.assume_checked();
    let Ok(sig) = MessageSignature::from_base64(signature_b64.trim()) else {
        return false;
    };
    sig.is_signed_by_address(&Secp256k1::new(), &addr, signed_msg_hash(message))
        .unwrap_or(false)
}

pub fn mask(kind: DestinationKind, value: &str) -> String {
    match kind {
        DestinationKind::LightningAddress => match value.split_once('@') {
            Some((user, domain)) => {
                let head: String = user.chars().take(2).collect();
                format!("{head}***@{domain}")
            }
            None => "***".into(),
        },
        _ => {
            let n = value.chars().count();
            if n <= 14 {
                return "***".into();
            }
            let head: String = value.chars().take(8).collect();
            let tail: String = value.chars().skip(n - 6).collect();
            format!("{head}...{tail}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_hide_the_middle() {
        assert_eq!(
            mask(DestinationKind::LightningAddress, "alice@example.com"),
            "al***@example.com"
        );
        assert_eq!(
            mask(
                DestinationKind::Onchain,
                "bcrt1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080"
            ),
            "bcrt1qw5...ygt080"
        );
    }

    #[test]
    fn lightning_address_is_lowercased_and_checked() {
        let d = normalize(
            DestinationKind::LightningAddress,
            " Alice@Example.COM ",
            Network::Bitcoin,
        )
        .unwrap();
        assert_eq!(d.value, "alice@example.com");
        assert!(normalize(DestinationKind::LightningAddress, "nope", Network::Bitcoin).is_err());
        assert!(
            normalize(
                DestinationKind::LightningAddress,
                "a b@x.io",
                Network::Bitcoin
            )
            .is_err()
        );
    }

    #[test]
    fn onchain_address_must_match_network() {
        let main = "bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4";
        assert!(normalize(DestinationKind::Onchain, main, Network::Bitcoin).is_ok());
        assert!(normalize(DestinationKind::Onchain, main, Network::Regtest).is_err());
        assert!(normalize(DestinationKind::Onchain, "garbage", Network::Bitcoin).is_err());
    }
}
