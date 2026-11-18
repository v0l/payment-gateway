use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Result, anyhow, ensure};
use rand::RngCore;
use sha2::{Digest, Sha256};

const NONCE_LEN: usize = 12;
const SEALED_PREFIX: &[u8] = b"pgw1:";
pub const DEFAULT_KID: &str = "default";

#[derive(Clone)]
struct Key {
    kid: String,
    cipher: Aes256Gcm,
}

#[derive(Clone)]
pub struct Sealer {
    keys: Vec<Key>,
}

pub fn parse_key_list(spec: &str) -> Result<Vec<(String, [u8; 32])>> {
    let mut keys: Vec<(String, [u8; 32])> = Vec::new();
    for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (kid, hex_key) = part
            .split_once(':')
            .ok_or_else(|| anyhow!("each key must be kid:hex"))?;
        ensure!(
            !kid.is_empty()
                && kid.len() <= 32
                && kid
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "key id must be 1-32 letters, digits, - or _"
        );
        ensure!(
            !keys.iter().any(|(k, _)| k == kid),
            "duplicate key id {kid}"
        );
        let raw: [u8; 32] = hex::decode(hex_key)
            .map_err(|_| anyhow!("key {kid} must be hex"))?
            .try_into()
            .map_err(|_| anyhow!("key {kid} must be 32 bytes"))?;
        keys.push((kid.to_string(), raw));
    }
    ensure!(!keys.is_empty(), "no encryption keys given");
    Ok(keys)
}

impl Sealer {
    pub fn new(key: &[u8; 32]) -> Self {
        Self::with_keys(&[(DEFAULT_KID.to_string(), *key)]).expect("one key")
    }

    pub fn with_keys(keys: &[(String, [u8; 32])]) -> Result<Self> {
        ensure!(!keys.is_empty(), "no encryption keys given");
        Ok(Self {
            keys: keys
                .iter()
                .map(|(kid, key)| Key {
                    kid: kid.clone(),
                    cipher: Aes256Gcm::new(key.into()),
                })
                .collect(),
        })
    }

    pub fn current_kid(&self) -> &str {
        &self.keys[0].kid
    }

    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>> {
        let key = &self.keys[0];
        let mut nonce = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce);
        let mut out = SEALED_PREFIX.to_vec();
        out.extend(key.kid.as_bytes());
        out.push(b':');
        out.extend(nonce);
        out.extend(
            key.cipher
                .encrypt(Nonce::from_slice(&nonce), plain)
                .map_err(|_| anyhow!("encrypt failed"))?,
        );
        Ok(out)
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>> {
        Ok(self.open_with_kid(sealed)?.0)
    }

    pub fn kid_of(sealed: &[u8]) -> Option<&str> {
        let rest = sealed.strip_prefix(SEALED_PREFIX)?;
        let end = rest.iter().position(|b| *b == b':')?;
        std::str::from_utf8(&rest[..end]).ok()
    }

    pub fn is_current(&self, sealed: &[u8]) -> bool {
        Self::kid_of(sealed) == Some(self.current_kid())
    }

    fn open_with_kid(&self, sealed: &[u8]) -> Result<(Vec<u8>, String)> {
        if let Some(kid) = Self::kid_of(sealed) {
            let body = &sealed[SEALED_PREFIX.len() + kid.len() + 1..];
            let key = self
                .keys
                .iter()
                .find(|k| k.kid == kid)
                .ok_or_else(|| anyhow!("no key for key id {kid}"))?;
            return Ok((Self::decrypt(&key.cipher, body)?, key.kid.clone()));
        }
        for key in &self.keys {
            if let Ok(plain) = Self::decrypt(&key.cipher, sealed) {
                return Ok((plain, key.kid.clone()));
            }
        }
        Err(anyhow!("decrypt failed"))
    }

    fn decrypt(cipher: &Aes256Gcm, raw: &[u8]) -> Result<Vec<u8>> {
        ensure!(raw.len() > NONCE_LEN, "sealed value too short");
        let (nonce, body) = raw.split_at(NONCE_LEN);
        cipher
            .decrypt(Nonce::from_slice(nonce), body)
            .map_err(|_| anyhow!("decrypt failed"))
    }

    pub fn reseal(&self, sealed: &[u8]) -> Result<Option<Vec<u8>>> {
        if self.is_current(sealed) {
            self.open(sealed)?;
            return Ok(None);
        }
        let plain = self.open(sealed)?;
        Ok(Some(self.seal(&plain)?))
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
    fn seal_carries_the_key_id_and_opens_with_any_key() {
        let old = [1u8; 32];
        let new = [2u8; 32];
        let only_old = Sealer::with_keys(&[("a".into(), old)]).unwrap();
        let both = Sealer::with_keys(&[("b".into(), new), ("a".into(), old)]).unwrap();
        let sealed = only_old.seal(b"x").unwrap();
        assert_eq!(Sealer::kid_of(&sealed), Some("a"));
        assert_eq!(both.open(&sealed).unwrap(), b"x");
        assert_eq!(Sealer::kid_of(&both.seal(b"x").unwrap()), Some("b"));
        let only_new = Sealer::with_keys(&[("b".into(), new)]).unwrap();
        assert!(only_new.open(&sealed).is_err());
    }

    #[test]
    fn legacy_unprefixed_values_open_and_reseal() {
        let key = [3u8; 32];
        let cipher = Aes256Gcm::new((&key).into());
        let nonce = [5u8; NONCE_LEN];
        let mut legacy = nonce.to_vec();
        legacy.extend(
            cipher
                .encrypt(Nonce::from_slice(&nonce), &b"old"[..])
                .unwrap(),
        );
        let s = Sealer::with_keys(&[("n".into(), [4u8; 32]), ("o".into(), key)]).unwrap();
        assert_eq!(s.open(&legacy).unwrap(), b"old");
        let resealed = s.reseal(&legacy).unwrap().unwrap();
        assert_eq!(Sealer::kid_of(&resealed), Some("n"));
        assert!(s.reseal(&resealed).unwrap().is_none());
    }

    #[test]
    fn key_list_parsing() {
        let h = "00".repeat(32);
        let keys = parse_key_list(&format!("k2:{h}, k1:{h}")).unwrap();
        assert_eq!(keys[0].0, "k2");
        assert_eq!(keys.len(), 2);
        assert!(parse_key_list(&format!("k1:{h},k1:{h}")).is_err());
        assert!(parse_key_list(&h).is_err());
        assert!(parse_key_list("k1:abcd").is_err());
        assert!(parse_key_list("").is_err());
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
