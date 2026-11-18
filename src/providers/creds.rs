use anyhow::{Context, Result, bail};
use rand::RngCore;
use serde_json::Value;
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;

pub struct Creds {
    pub cert: PathBuf,
    pub macaroon: PathBuf,
    temp: Vec<PathBuf>,
}

impl Drop for Creds {
    fn drop(&mut self) {
        for p in &self.temp {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn write_private(bytes: &[u8]) -> Result<PathBuf> {
    let mut name = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut name);
    let path = std::env::temp_dir().join(format!("pgw-{}", hex::encode(name)));
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)?;
    std::io::Write::write_all(&mut f, bytes)?;
    Ok(path)
}

pub fn lnd_creds(config: &Value) -> Result<Creds> {
    let mut temp = vec![];
    let cert = match (
        config.get("cert").and_then(Value::as_str),
        config.get("cert_path").and_then(Value::as_str),
    ) {
        (Some(pem), _) => {
            if !pem.contains("BEGIN CERTIFICATE") {
                bail!("cert must be a PEM certificate");
            }
            let p = write_private(pem.as_bytes())?;
            temp.push(p.clone());
            p
        }
        (None, Some(path)) => PathBuf::from(path),
        _ => bail!("lnd config needs cert or cert_path"),
    };
    let macaroon = match (
        config.get("macaroon").and_then(Value::as_str),
        config.get("macaroon_path").and_then(Value::as_str),
    ) {
        (Some(hex_mac), _) => {
            let bytes = hex::decode(hex_mac.trim()).context("macaroon must be hex")?;
            if bytes.is_empty() {
                bail!("macaroon is empty");
            }
            let p = write_private(&bytes)?;
            temp.push(p.clone());
            p
        }
        (None, Some(path)) => PathBuf::from(path),
        _ => bail!("lnd config needs macaroon or macaroon_path"),
    };
    Ok(Creds {
        cert,
        macaroon,
        temp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn inline_creds_become_private_files_removed_on_drop() {
        let c = lnd_creds(&json!({"cert": "-----BEGIN CERTIFICATE-----\nabc\n-----END CERTIFICATE-----", "macaroon": "0a0b"})).unwrap();
        let (cert, mac) = (c.cert.clone(), c.macaroon.clone());
        assert_eq!(std::fs::read(&mac).unwrap(), vec![0x0a, 0x0b]);
        assert!(
            std::fs::read_to_string(&cert)
                .unwrap()
                .contains("BEGIN CERTIFICATE")
        );
        assert_eq!(
            std::fs::metadata(&mac).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(c);
        assert!(!cert.exists() && !mac.exists());
    }

    #[test]
    fn paths_are_used_as_is_and_bad_inline_values_are_rejected() {
        let c = lnd_creds(&json!({"cert_path": "/a", "macaroon_path": "/b"})).unwrap();
        assert_eq!(
            (c.cert.to_str(), c.macaroon.to_str()),
            (Some("/a"), Some("/b"))
        );
        drop(c);
        assert!(lnd_creds(&json!({"cert": "nope", "macaroon": "0a"})).is_err());
        assert!(
            lnd_creds(&json!({"cert": "-----BEGIN CERTIFICATE-----", "macaroon": "zz"})).is_err()
        );
        assert!(
            lnd_creds(&json!({"cert": "-----BEGIN CERTIFICATE-----", "macaroon": ""})).is_err()
        );
        assert!(lnd_creds(&json!({})).is_err());
    }
}
