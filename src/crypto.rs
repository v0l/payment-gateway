use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Result, anyhow, ensure};
use rand::RngCore;
use sha2::{Digest, Sha256};

const NONCE_LEN: usize = 12;

#[derive(Clone)]
pub struct Sealer {
    cipher: Aes256Gcm,
}

impl Sealer {
    pub fn new(key: &[u8; 32]) -> Self {
        Self {
            cipher: Aes256Gcm::new(key.into()),
        }
    }

    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        let mut nonce = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce);
        let mut out = nonce.to_vec();
        out.extend(
            self.cipher
                .encrypt(Nonce::from_slice(&nonce), plain)
                .map_err(|_| anyhow!("encrypt failed"))?,
        );
        Ok(out)
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        ensure!(sealed.len() > NONCE_LEN, "sealed value too short");
        let (nonce, body) = sealed.split_at(NONCE_LEN);
        self.cipher
            .decrypt(Nonce::from_slice(nonce), body)
            .map_err(|_| anyhow!("decrypt failed"))
    }
}

pub fn new_api_key() -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    format!("pgw_{}", hex::encode(raw))
}

pub fn new_webhook_secret() -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    format!("whsec_{}", hex::encode(raw))
}

pub fn hash_api_key(key: &str) -> String {
    hex::encode(Sha256::digest(key.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_round_trips() {
        let s = Sealer::new(&[7u8; 32]);
        let sealed = s.seal(b"macaroon").unwrap();
        assert_ne!(sealed, b"macaroon");
        assert_eq!(s.open(&sealed).unwrap(), b"macaroon");
    }

    #[test]
    fn seal_uses_fresh_nonce() {
        let s = Sealer::new(&[7u8; 32]);
        assert_ne!(s.seal(b"x").unwrap(), s.seal(b"x").unwrap());
    }

    #[test]
    fn open_rejects_tampering_and_wrong_key() {
        let s = Sealer::new(&[7u8; 32]);
        let mut sealed = s.seal(b"secret").unwrap();
        assert!(Sealer::new(&[8u8; 32]).open(&sealed).is_err());
        *sealed.last_mut().unwrap() ^= 1;
        assert!(s.open(&sealed).is_err());
        assert!(s.open(&[0u8; 4]).is_err());
    }

    #[test]
    fn api_keys_are_unique_and_hash_stably() {
        let a = new_api_key();
        assert!(a.starts_with("pgw_"));
        assert_ne!(a, new_api_key());
        assert_eq!(hash_api_key(&a), hash_api_key(&a));
        assert_eq!(hash_api_key(&a).len(), 64);
    }
}
