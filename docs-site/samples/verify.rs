use hmac::{Hmac, Mac};
use sha2::Sha256;

pub fn verify_webhook(secret: &str, header: &str, body: &str, now: i64, tolerance: i64) -> bool {
    let mut timestamp = None;
    let mut signature = None;
    for part in header.split(',') {
        match part.split_once('=') {
            Some(("t", v)) => timestamp = v.parse::<i64>().ok(),
            Some(("v1", v)) => signature = hex::decode(v).ok(),
            _ => {}
        }
    }
    let (Some(timestamp), Some(signature)) = (timestamp, signature) else {
        return false;
    };
    if (now - timestamp).abs() > tolerance {
        return false;
    }
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac accepts any key length");
    mac.update(format!("{timestamp}.{body}").as_bytes());
    mac.verify_slice(&signature).is_ok()
}
