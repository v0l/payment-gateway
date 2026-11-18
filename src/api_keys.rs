use rand::RngCore;

pub const SCOPE_PAYMENTS_WRITE: &str = "payments:write";
pub const SCOPE_PAYMENTS_READ: &str = "payments:read";
pub const SCOPE_REFUNDS_WRITE: &str = "refunds:write";
pub const SCOPE_SAVED_WRITE: &str = "saved:write";
pub const SCOPE_ADMIN: &str = "admin";

pub const SCOPES: [&str; 5] = [
    SCOPE_PAYMENTS_WRITE,
    SCOPE_PAYMENTS_READ,
    SCOPE_REFUNDS_WRITE,
    SCOPE_SAVED_WRITE,
    SCOPE_ADMIN,
];

pub fn parse_scopes(input: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = vec![];
    for s in input {
        let s = s.trim();
        if !SCOPES.contains(&s) {
            return Err(format!("unknown scope {s}"));
        }
        if !out.iter().any(|o| o == s) {
            out.push(s.to_string());
        }
    }
    if out.is_empty() {
        return Err("at least one scope is required".into());
    }
    Ok(out)
}

#[derive(Clone, Debug)]
pub struct Scopes(Vec<String>);

impl Scopes {
    pub fn parse(stored: &str) -> Self {
        Self(
            stored
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect(),
        )
    }

    pub fn allows(&self, needed: &str) -> bool {
        self.0.iter().any(|s| s == SCOPE_ADMIN || s == needed)
    }

    pub fn list(&self) -> &[String] {
        &self.0
    }
}

pub fn key_mode(trusted: bool) -> &'static str {
    if trusted { "live" } else { "test" }
}

pub fn new_scoped_key(mode: &str) -> String {
    let mut raw = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut raw);
    format!("pgw_{mode}_{}", hex::encode(raw))
}

pub fn key_prefix(key: &str) -> String {
    key.chars().take(14).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn admin_allows_everything_and_others_only_themselves() {
        let admin = Scopes::parse("admin");
        assert!(admin.allows(SCOPE_REFUNDS_WRITE));
        let read = Scopes::parse("payments:read");
        assert!(read.allows(SCOPE_PAYMENTS_READ));
        assert!(!read.allows(SCOPE_REFUNDS_WRITE));
    }

    #[test]
    fn unknown_and_empty_scopes_are_rejected() {
        assert!(parse_scopes(&["nope".into()]).is_err());
        assert!(parse_scopes(&[]).is_err());
        assert_eq!(
            parse_scopes(&["payments:read".into(), "payments:read".into()]).unwrap(),
            vec!["payments:read"]
        );
    }
}
