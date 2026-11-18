use crate::crypto;
use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Clone)]
pub struct Config {
    pub database_url: String,
    pub listen: String,
    pub admin_token: String,
    pub metrics_token: Option<String>,
    pub encryption_keys: Vec<(String, [u8; 32])>,
    pub public_url: String,
    pub allow_mock: bool,
    pub operator_2fa_grace: bool,
    pub log_level: String,
    pub log_json: bool,
    pub body_limit: usize,
    pub request_timeout: Duration,
    pub db_acquire_timeout: Duration,
    pub db_max_connections: u32,
    pub shutdown_grace: Duration,
    pub lease_ttl: Duration,
    pub worker_tick: Duration,
    pub instance_id: String,
}

pub struct Env<'a> {
    get: &'a dyn Fn(&str) -> Option<String>,
    errors: Vec<String>,
}

impl<'a> Env<'a> {
    pub fn new(get: &'a dyn Fn(&str) -> Option<String>) -> Self {
        Self {
            get,
            errors: vec![],
        }
    }

    fn raw(&mut self, name: &str) -> Option<String> {
        let file_name = format!("{name}_FILE");
        let direct = (self.get)(name);
        match (direct, (self.get)(&file_name)) {
            (Some(_), Some(_)) => {
                self.errors
                    .push(format!("set only one of {name} and {file_name}"));
                None
            }
            (Some(v), None) => Some(v),
            (None, Some(path)) => match std::fs::read_to_string(&path) {
                Ok(v) => Some(v.trim_end_matches(['\n', '\r']).to_string()),
                Err(e) => {
                    self.errors
                        .push(format!("{file_name}: cannot read {path}: {e}"));
                    None
                }
            },
            (None, None) => None,
        }
    }

    pub fn required(&mut self, name: &str) -> String {
        match self.raw(name) {
            Some(v) if !v.is_empty() => v,
            Some(_) => {
                self.errors.push(format!("{name} is empty"));
                String::new()
            }
            None => {
                if !self.errors.iter().any(|e| e.contains(name)) {
                    self.errors
                        .push(format!("{name} is required (or {name}_FILE)"));
                }
                String::new()
            }
        }
    }

    pub fn optional(&mut self, name: &str) -> Option<String> {
        self.raw(name).filter(|v| !v.is_empty())
    }

    pub fn number<T: std::str::FromStr + PartialOrd + std::fmt::Display>(
        &mut self,
        name: &str,
        default: T,
        min: T,
        max: T,
    ) -> T {
        match self.optional(name) {
            None => default,
            Some(v) => match v.trim().parse::<T>() {
                Ok(n) if n >= min && n <= max => n,
                Ok(_) => {
                    self.errors
                        .push(format!("{name} must be between {min} and {max}"));
                    default
                }
                Err(_) => {
                    self.errors.push(format!("{name} must be a number"));
                    default
                }
            },
        }
    }

    pub fn flag(&mut self, name: &str) -> bool {
        match self.optional(name).as_deref() {
            None | Some("0") | Some("false") => false,
            Some("1") | Some("true") => true,
            Some(_) => {
                self.errors
                    .push(format!("{name} must be 0, 1, true or false"));
                false
            }
        }
    }

    pub fn fail(&mut self, msg: String) {
        self.errors.push(msg);
    }

    pub fn finish(self) -> Result<()> {
        if self.errors.is_empty() {
            return Ok(());
        }
        bail!("invalid configuration:\n  - {}", self.errors.join("\n  - "))
    }
}

pub const LOG_LEVELS: [&str; 6] = ["off", "error", "warn", "info", "debug", "trace"];

impl Config {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(&|k| std::env::var(k).ok())
    }

    fn keys(env: &mut Env) -> Vec<(String, [u8; 32])> {
        if let Some(list) = env.optional("GATEWAY_ENCRYPTION_KEYS") {
            return match crypto::parse_key_list(&list) {
                Ok(k) => k,
                Err(e) => {
                    env.fail(format!("GATEWAY_ENCRYPTION_KEYS: {e}"));
                    vec![]
                }
            };
        }
        let key_hex = env.required("GATEWAY_ENCRYPTION_KEY");
        if key_hex.is_empty() {
            return vec![];
        }
        match hex::decode(key_hex.trim()) {
            Ok(k) if k.len() == 32 => {
                let mut key = [0u8; 32];
                key.copy_from_slice(&k);
                vec![(crypto::DEFAULT_KID.to_string(), key)]
            }
            Ok(_) => {
                env.fail("GATEWAY_ENCRYPTION_KEY must be 32 bytes (64 hex chars)".into());
                vec![]
            }
            Err(_) => {
                env.fail("GATEWAY_ENCRYPTION_KEY must be hex".into());
                vec![]
            }
        }
    }

    pub fn from_lookup(get: &dyn Fn(&str) -> Option<String>) -> Result<Self> {
        let mut env = Env::new(get);
        let database_url = env.required("DATABASE_URL");
        if !database_url.is_empty() && !database_url.starts_with("mysql://") {
            env.fail("DATABASE_URL must start with mysql://".into());
        }
        let encryption_keys = Self::keys(&mut env);
        let admin_token = env.required("GATEWAY_ADMIN_TOKEN");
        if !admin_token.is_empty() && admin_token.len() < 24 {
            env.fail("GATEWAY_ADMIN_TOKEN must be at least 24 characters".into());
        }
        let metrics_token = env.optional("METRICS_TOKEN");
        if metrics_token.as_ref().is_some_and(|t| t.len() < 24) {
            env.fail("METRICS_TOKEN must be at least 24 characters".into());
        }
        let public_url = env.required("GATEWAY_PUBLIC_URL");
        if !public_url.is_empty()
            && !url::Url::parse(&public_url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
        {
            env.fail("GATEWAY_PUBLIC_URL must be an http(s) URL".into());
        }
        let listen = env
            .optional("GATEWAY_LISTEN")
            .unwrap_or_else(|| "0.0.0.0:8080".into());
        if listen.parse::<std::net::SocketAddr>().is_err() {
            env.fail(
                "GATEWAY_LISTEN must be host:port as an IP address, for example 0.0.0.0:8080"
                    .into(),
            );
        }
        let allow_mock = env.flag("GATEWAY_ALLOW_MOCK");
        let operator_2fa_grace = env.flag("GATEWAY_OPERATOR_2FA_GRACE");
        let log_level = env
            .optional("LOG_LEVEL")
            .or_else(|| env.optional("RUST_LOG"))
            .unwrap_or_else(|| "info".into())
            .to_lowercase();
        if !log_level.contains('=')
            && !log_level.contains(',')
            && !LOG_LEVELS.contains(&log_level.as_str())
        {
            env.fail(format!(
                "LOG_LEVEL must be one of {}",
                LOG_LEVELS.join(", ")
            ));
        }
        let log_json = match env.optional("LOG_FORMAT").as_deref() {
            None | Some("json") => true,
            Some("text") => false,
            Some(_) => {
                env.fail("LOG_FORMAT must be json or text".into());
                true
            }
        };
        let body_limit = env.number("GATEWAY_BODY_LIMIT_BYTES", 1_048_576usize, 1024, 64 << 20);
        let request_timeout = env.number("GATEWAY_REQUEST_TIMEOUT_SECS", 60u64, 1, 3600);
        let db_acquire_timeout = env.number("DB_ACQUIRE_TIMEOUT_SECS", 10u64, 1, 300);
        let db_max_connections = env.number("DB_MAX_CONNECTIONS", 10u32, 1, 500);
        let shutdown_grace = env.number("SHUTDOWN_GRACE_SECS", 25u64, 1, 600);
        let lease_ttl = env.number("LEASE_TTL_SECS", 30u64, 3, 600);
        let worker_tick = env.number("WORKER_TICK_SECS", 5u64, 1, 300);
        let instance_id = env.optional("GATEWAY_INSTANCE_ID").unwrap_or_else(|| {
            let host = get("HOSTNAME").unwrap_or_else(|| "gateway".into());
            format!("{host}-{}", std::process::id())
        });
        env.finish()?;
        Ok(Self {
            database_url,
            listen,
            admin_token,
            metrics_token,
            encryption_keys,
            public_url,
            allow_mock,
            operator_2fa_grace,
            log_level,
            log_json,
            body_limit,
            request_timeout: Duration::from_secs(request_timeout),
            db_acquire_timeout: Duration::from_secs(db_acquire_timeout),
            db_max_connections,
            shutdown_grace: Duration::from_secs(shutdown_grace),
            lease_ttl: Duration::from_secs(lease_ttl),
            worker_tick: Duration::from_secs(worker_tick),
            instance_id,
        })
    }
}

pub fn map_lookup(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |k| map.get(k).cloned()
}

pub fn encryption_keys_from_env() -> Result<Vec<(String, [u8; 32])>> {
    let get = |k: &str| std::env::var(k).ok();
    let mut env = Env::new(&get);
    let keys = Config::keys(&mut env);
    env.finish().context("encryption key configuration")?;
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000";
    const TOKEN: &str = "abcdefabcdefabcdefabcdefabcdef";

    fn base() -> Vec<(&'static str, &'static str)> {
        vec![
            ("DATABASE_URL", "mysql://u:p@h/db"),
            ("GATEWAY_ENCRYPTION_KEY", KEY),
            ("GATEWAY_ADMIN_TOKEN", TOKEN),
            ("GATEWAY_PUBLIC_URL", "https://pay.example.com"),
        ]
    }

    #[test]
    fn minimal_env_gets_defaults() {
        let c = Config::from_lookup(&map_lookup(&base())).unwrap();
        assert_eq!(c.listen, "0.0.0.0:8080");
        assert_eq!(c.body_limit, 1_048_576);
        assert!(c.log_json);
        assert_eq!(c.log_level, "info");
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let err = Config::from_lookup(&map_lookup(&[
            ("GATEWAY_ENCRYPTION_KEY", "zz"),
            ("GATEWAY_ADMIN_TOKEN", "short"),
            ("GATEWAY_PUBLIC_URL", "ftp://x"),
            ("GATEWAY_LISTEN", "nope"),
            ("LOG_LEVEL", "loud"),
            ("LEASE_TTL_SECS", "1"),
            ("DB_MAX_CONNECTIONS", "many"),
        ]))
        .err()
        .unwrap()
        .to_string();
        for needle in [
            "DATABASE_URL is required",
            "GATEWAY_ENCRYPTION_KEY must be hex",
            "GATEWAY_ADMIN_TOKEN must be at least 24",
            "GATEWAY_PUBLIC_URL must be an http(s) URL",
            "GATEWAY_LISTEN must be",
            "LOG_LEVEL must be one of",
            "LEASE_TTL_SECS must be between 3 and 600",
            "DB_MAX_CONNECTIONS must be a number",
        ] {
            assert!(err.contains(needle), "missing {needle:?} in {err}");
        }
    }

    #[test]
    fn secrets_can_come_from_files() {
        let dir = std::env::temp_dir().join(format!("pgw-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("admin");
        std::fs::write(&path, format!("{TOKEN}\n")).unwrap();
        let path = path.to_string_lossy().to_string();
        let mut pairs: Vec<(&str, &str)> = base()
            .into_iter()
            .filter(|(k, _)| *k != "GATEWAY_ADMIN_TOKEN")
            .collect();
        pairs.push(("GATEWAY_ADMIN_TOKEN_FILE", &path));
        let c = Config::from_lookup(&map_lookup(&pairs)).unwrap();
        assert_eq!(c.admin_token, TOKEN);

        pairs.push(("GATEWAY_ADMIN_TOKEN", TOKEN));
        let err = Config::from_lookup(&map_lookup(&pairs))
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("set only one of GATEWAY_ADMIN_TOKEN"));

        let mut missing = base();
        missing.retain(|(k, _)| *k != "GATEWAY_ADMIN_TOKEN");
        missing.push(("GATEWAY_ADMIN_TOKEN_FILE", "/nonexistent/pgw"));
        let err = Config::from_lookup(&map_lookup(&missing))
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("GATEWAY_ADMIN_TOKEN_FILE: cannot read"));
    }
}
