use hmac::{Hmac, Mac};
use rand::RngCore;
use sha1::Sha1;
use subtle::ConstantTimeEq;

pub const STEP_SECONDS: u64 = 30;
pub const DIGITS: u32 = 6;
pub const SECRET_BYTES: usize = 20;
pub const RECOVERY_CODES: usize = 10;

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

pub fn new_secret() -> Vec<u8> {
    let mut b = vec![0u8; SECRET_BYTES];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

pub fn base32(data: &[u8]) -> String {
    let mut out = String::new();
    let mut buffer = 0u32;
    let mut bits = 0;
    for &byte in data {
        buffer = (buffer << 8) | byte as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(BASE32[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(BASE32[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub fn hotp(secret: &[u8], counter: u64, digits: u32) -> u32 {
    let mut mac = Hmac::<Sha1>::new_from_slice(secret).expect("hmac accepts any key length");
    mac.update(&counter.to_be_bytes());
    let digest = mac.finalize().into_bytes();
    let offset = (digest[19] & 0x0f) as usize;
    let bin = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    bin % 10u32.pow(digits)
}

pub fn step_at(unix_seconds: u64) -> u64 {
    unix_seconds / STEP_SECONDS
}

pub fn code_for_step(secret: &[u8], step: u64) -> String {
    format!(
        "{:0width$}",
        hotp(secret, step, DIGITS),
        width = DIGITS as usize
    )
}

pub fn matching_step(secret: &[u8], submitted: &str, unix_seconds: u64) -> Option<u64> {
    let submitted: String = submitted.chars().filter(|c| !c.is_whitespace()).collect();
    if submitted.len() != DIGITS as usize || !submitted.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let now = step_at(unix_seconds);
    let mut found = None;
    for step in [now.saturating_sub(1), now, now + 1] {
        let expected = code_for_step(secret, step);
        if bool::from(expected.as_bytes().ct_eq(submitted.as_bytes())) {
            found = Some(step);
        }
    }
    found
}

pub fn otpauth_uri(issuer: &str, account: &str, secret: &[u8]) -> String {
    let label = format!("{}:{}", encode(issuer), encode(account));
    format!(
        "otpauth://totp/{label}?secret={}&issuer={}&algorithm=SHA1&digits={DIGITS}&period={STEP_SECONDS}",
        base32(secret),
        encode(issuer)
    )
}

fn encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn qr_svg(data: &str) -> anyhow::Result<String> {
    let code = qrcode::QrCode::new(data.as_bytes())?;
    Ok(code
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .build())
}

pub fn new_recovery_code() -> String {
    let mut b = [0u8; 8];
    rand::thread_rng().fill_bytes(&mut b);
    let h = hex::encode(b);
    format!("{}-{}-{}-{}", &h[0..4], &h[4..8], &h[8..12], &h[12..16])
}

pub fn normalize_recovery_code(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RFC_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn rfc_4226_hotp_vectors() {
        let expected = [
            755224, 287082, 359152, 969429, 338314, 254676, 287922, 162583, 399871, 520489,
        ];
        for (counter, want) in expected.iter().enumerate() {
            assert_eq!(hotp(RFC_SECRET, counter as u64, 6), *want);
        }
    }

    #[test]
    fn rfc_6238_sha1_vectors() {
        let vectors = [
            (59u64, 94287082u32),
            (1111111109, 7081804),
            (1111111111, 14050471),
            (1234567890, 89005924),
            (2000000000, 69279037),
            (20000000000, 65353130),
        ];
        for (time, want) in vectors {
            assert_eq!(hotp(RFC_SECRET, step_at(time), 8), want, "t={time}");
        }
    }

    #[test]
    fn base32_matches_rfc_4648() {
        assert_eq!(base32(b""), "");
        assert_eq!(base32(b"f"), "MY");
        assert_eq!(base32(b"fo"), "MZXQ");
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
        assert_eq!(base32(RFC_SECRET), "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ");
    }

    #[test]
    fn drift_of_one_step_is_accepted_and_two_rejected() {
        let t = 1_700_000_015u64;
        let now = step_at(t);
        assert_eq!(
            matching_step(RFC_SECRET, &code_for_step(RFC_SECRET, now), t),
            Some(now)
        );
        assert_eq!(
            matching_step(RFC_SECRET, &code_for_step(RFC_SECRET, now - 1), t),
            Some(now - 1)
        );
        assert_eq!(
            matching_step(RFC_SECRET, &code_for_step(RFC_SECRET, now + 1), t),
            Some(now + 1)
        );
        assert_eq!(
            matching_step(RFC_SECRET, &code_for_step(RFC_SECRET, now - 2), t),
            None
        );
        assert_eq!(
            matching_step(RFC_SECRET, &code_for_step(RFC_SECRET, now + 2), t),
            None
        );
    }

    #[test]
    fn malformed_codes_never_match() {
        for bad in ["", "12345", "1234567", "abcdef", "12 34"] {
            assert_eq!(matching_step(RFC_SECRET, bad, 59), None, "{bad:?}");
        }
    }

    #[test]
    fn otpauth_uri_carries_the_parameters() {
        let uri = otpauth_uri("Payment gateway", "a@b.co", RFC_SECRET);
        assert!(uri.starts_with("otpauth://totp/Payment%20gateway:a%40b.co?"));
        assert!(uri.contains("secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"));
        assert!(uri.contains("digits=6&period=30"));
    }

    #[test]
    fn recovery_codes_normalise() {
        let c = new_recovery_code();
        assert_eq!(c.len(), 19);
        assert_eq!(normalize_recovery_code(&c.to_uppercase()).len(), 16);
        assert_ne!(c, new_recovery_code());
    }
}
