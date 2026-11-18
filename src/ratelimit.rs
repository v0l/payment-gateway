use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Clone, Copy, Debug)]
pub struct Rule {
    pub burst: u32,
    pub refill_per_sec: f64,
}

impl Rule {
    pub const fn new(burst: u32, refill_per_sec: f64) -> Self {
        Self {
            burst,
            refill_per_sec,
        }
    }
}

pub struct TokenBuckets {
    rule: Rule,
    buckets: Mutex<HashMap<String, (f64, Instant)>>,
}

const SWEEP_AT: usize = 10_000;

impl TokenBuckets {
    pub fn new(rule: Rule) -> Self {
        Self {
            rule,
            buckets: Default::default(),
        }
    }

    pub fn check(&self, key: &str) -> Result<(), u64> {
        self.check_at(key, Instant::now())
    }

    pub fn check_at(&self, key: &str, now: Instant) -> Result<(), u64> {
        let rule = self.rule;
        let mut map = self.buckets.lock().unwrap();
        if map.len() > SWEEP_AT {
            map.retain(|_, (tokens, at)| refilled(rule, *tokens, *at, now) < rule.burst as f64);
        }
        let entry = map
            .entry(key.to_string())
            .or_insert((rule.burst as f64, now));
        let tokens = refilled(rule, entry.0, entry.1, now);
        if tokens >= 1.0 {
            *entry = (tokens - 1.0, now);
            Ok(())
        } else {
            *entry = (tokens, now);
            let wait = (1.0 - tokens) / rule.refill_per_sec.max(f64::MIN_POSITIVE);
            Err(wait.ceil().clamp(1.0, 86_400.0) as u64)
        }
    }
}

fn refilled(rule: Rule, tokens: f64, at: Instant, now: Instant) -> f64 {
    let elapsed = now.saturating_duration_since(at).as_secs_f64();
    (tokens + elapsed * rule.refill_per_sec).min(rule.burst as f64)
}

pub struct RateLimits {
    pub api_key: TokenBuckets,
    pub api_ip: TokenBuckets,
    pub pay_ip: TokenBuckets,
    pub pay_attempt_ip: TokenBuckets,
    pub signup_ip: TokenBuckets,
    pub login_ip: TokenBuckets,
    pub hook_ip: TokenBuckets,
}

#[derive(Clone, Copy, Debug)]
pub struct RateConfig {
    pub api_key: Rule,
    pub api_ip: Rule,
    pub pay_ip: Rule,
    pub pay_attempt_ip: Rule,
    pub signup_ip: Rule,
    pub login_ip: Rule,
    pub hook_ip: Rule,
}

impl Default for RateConfig {
    fn default() -> Self {
        Self {
            api_key: Rule::new(200, 50.0),
            api_ip: Rule::new(400, 100.0),
            pay_ip: Rule::new(120, 2.0),
            pay_attempt_ip: Rule::new(10, 0.2),
            signup_ip: Rule::new(5, 5.0 / 3600.0),
            login_ip: Rule::new(20, 20.0 / 60.0),
            hook_ip: Rule::new(300, 50.0),
        }
    }
}

impl RateLimits {
    pub fn new(c: RateConfig) -> Self {
        Self {
            api_key: TokenBuckets::new(c.api_key),
            api_ip: TokenBuckets::new(c.api_ip),
            pay_ip: TokenBuckets::new(c.pay_ip),
            pay_attempt_ip: TokenBuckets::new(c.pay_attempt_ip),
            signup_ip: TokenBuckets::new(c.signup_ip),
            login_ip: TokenBuckets::new(c.login_ip),
            hook_ip: TokenBuckets::new(c.hook_ip),
        }
    }

    pub fn for_request(&self, method: &str, path: &str, ip: &str) -> Result<(), u64> {
        if path.starts_with("/v1/") {
            return self.api_ip.check(ip);
        }
        if path.starts_with("/pay/") && path.ends_with("/attempts") && method == "POST" {
            self.pay_attempt_ip.check(ip)?;
            return self.pay_ip.check(ip);
        }
        if path.starts_with("/pay/") || path.starts_with("/embed/") {
            return self.pay_ip.check(ip);
        }
        match path {
            "/console/api/signup" => self.signup_ip.check(ip),
            "/console/api/login" => self.login_ip.check(ip),
            _ if path.starts_with("/hook/") => self.hook_ip.check(ip),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn blocks_at_burst_and_refills() {
        let b = TokenBuckets::new(Rule::new(3, 1.0));
        let t = Instant::now();
        for _ in 0..3 {
            assert!(b.check_at("a", t).is_ok());
        }
        assert_eq!(b.check_at("a", t), Err(1));
        assert!(b.check_at("b", t).is_ok());
        assert!(b.check_at("a", t + Duration::from_millis(1100)).is_ok());
        assert!(b.check_at("a", t + Duration::from_millis(1100)).is_err());
    }

    #[test]
    fn retry_after_reflects_slow_refill() {
        let b = TokenBuckets::new(Rule::new(1, 0.1));
        let t = Instant::now();
        b.check_at("a", t).unwrap();
        assert_eq!(b.check_at("a", t), Err(10));
    }
}
