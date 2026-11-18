use log::{Level, LevelFilter, Log, Metadata, Record};
use serde_json::{Map, Value, json};
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

const MAX_MESSAGE: usize = 2000;
const MASK: &str = "[redacted]";
const SECRET_PREFIXES: [&str; 5] = ["pgw_", "whsec_", "sk_", "rk_", "whsk_"];
const SECRET_KEYS: [&str; 7] = [
    "secret",
    "token",
    "password",
    "macaroon",
    "authorization",
    "api_key",
    "apikey",
];

static SECRETS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

fn secrets() -> &'static Mutex<Vec<String>> {
    SECRETS.get_or_init(Default::default)
}

pub fn register_secret(value: &str) {
    if value.len() >= 8 {
        secrets().lock().unwrap().push(value.to_string());
    }
}

#[derive(Default, Clone, Debug)]
pub struct RequestContext {
    pub request_id: String,
    pub tenant_id: Option<u64>,
}

tokio::task_local! {
    pub static CONTEXT: Arc<Mutex<RequestContext>>;
}

pub fn set_tenant(tenant_id: u64) {
    let _ = CONTEXT.try_with(|c| c.lock().unwrap().tenant_id = Some(tenant_id));
}

pub fn current_context() -> Option<RequestContext> {
    CONTEXT.try_with(|c| c.lock().unwrap().clone()).ok()
}

fn is_secret_token(token: &str) -> bool {
    if SECRET_PREFIXES.iter().any(|p| token.starts_with(p)) && token.len() > 8 {
        return true;
    }
    token.len() >= 40 && token.bytes().all(|b| b.is_ascii_hexdigit())
}

fn is_secret_key(key: &str) -> bool {
    let key = key.to_lowercase();
    SECRET_KEYS.iter().any(|k| key.contains(k))
}

fn mask_word(word: &str) -> String {
    let trimmed =
        word.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | ')' | '(' | '}' | '{'));
    if trimmed.is_empty() {
        return word.to_string();
    }
    if let Some((k, v)) = trimmed.split_once(['=', ':'])
        && is_secret_key(k)
        && !v.is_empty()
    {
        return word.replace(v, MASK);
    }
    if is_secret_token(trimmed) {
        return word.replace(trimmed, MASK);
    }
    word.to_string()
}

pub fn redact(message: &str) -> String {
    let mut out = message.to_string();
    for s in secrets().lock().unwrap().iter() {
        out = out.replace(s.as_str(), MASK);
    }
    let mut words = vec![];
    let mut after_bearer = false;
    for w in out.split(' ') {
        if after_bearer && !w.is_empty() {
            words.push(MASK.to_string());
            after_bearer = false;
            continue;
        }
        if w == "Bearer" {
            after_bearer = true;
        }
        words.push(mask_word(w));
    }
    let out = words.join(" ");
    if out.len() > MAX_MESSAGE {
        let cut = out
            .char_indices()
            .nth(MAX_MESSAGE)
            .map(|(i, _)| i)
            .unwrap_or(out.len());
        format!("{}...[truncated]", &out[..cut])
    } else {
        out
    }
}

pub fn format_line(level: Level, target: &str, message: &str, json_format: bool) -> String {
    let ctx = current_context();
    let message = redact(message);
    if json_format {
        let mut m = Map::new();
        m.insert("ts".into(), json!(chrono::Utc::now().to_rfc3339()));
        m.insert("level".into(), json!(level.as_str().to_lowercase()));
        m.insert("target".into(), json!(target));
        m.insert("msg".into(), json!(message));
        if let Some(c) = ctx {
            if !c.request_id.is_empty() {
                m.insert("request_id".into(), json!(c.request_id));
            }
            if let Some(t) = c.tenant_id {
                m.insert("tenant_id".into(), json!(t));
            }
        }
        Value::Object(m).to_string()
    } else {
        let mut extra = String::new();
        if let Some(c) = ctx {
            if !c.request_id.is_empty() {
                extra.push_str(&format!(" request_id={}", c.request_id));
            }
            if let Some(t) = c.tenant_id {
                extra.push_str(&format!(" tenant_id={t}"));
            }
        }
        format!(
            "{} {level} {target}{extra} {message}",
            chrono::Utc::now().to_rfc3339()
        )
    }
}

pub struct Logger {
    filter: LevelFilter,
    json: bool,
    sink: Box<dyn Fn(&str) + Send + Sync>,
}

impl Logger {
    pub fn new(filter: LevelFilter, json: bool, sink: Box<dyn Fn(&str) + Send + Sync>) -> Self {
        Self { filter, json, sink }
    }
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata) -> bool {
        m.level() <= self.filter
    }

    fn log(&self, r: &Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        let line = format_line(r.level(), r.target(), &r.args().to_string(), self.json);
        (self.sink)(&line);
    }

    fn flush(&self) {}
}

pub fn parse_level(s: &str) -> LevelFilter {
    let simple = s.rsplit(',').next().unwrap_or(s);
    let simple = simple.rsplit('=').next().unwrap_or(simple);
    simple.parse().unwrap_or(LevelFilter::Info)
}

pub fn init(level: &str, json_format: bool) {
    let logger = Logger::new(
        parse_level(level),
        json_format,
        Box::new(|line| {
            let _ = writeln!(std::io::stderr().lock(), "{line}");
        }),
    );
    let filter = logger.filter;
    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(filter);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_secrets_never_reach_the_line() {
        register_secret("MARKER-ADMIN-TOKEN-VALUE");
        let line = format_line(
            Level::Warn,
            "t",
            "call failed token=abcd1234 key pgw_0123456789abcdef header Bearer ZZZSECRETZZZ and MARKER-ADMIN-TOKEN-VALUE, password=hunter22 whsec_abcdefghijklmnop",
            true,
        );
        for leak in [
            "abcd1234",
            "pgw_0123456789abcdef",
            "ZZZSECRETZZZ",
            "MARKER-ADMIN-TOKEN-VALUE",
            "hunter22",
            "whsec_abcdefghijklmnop",
        ] {
            assert!(!line.contains(leak), "leaked {leak}: {line}");
        }
        assert!(line.contains("call failed"));
    }

    #[test]
    fn long_bodies_are_truncated_and_long_hex_masked() {
        let hex = "ab".repeat(32);
        let body = format!("x{}", "y".repeat(5000));
        let line = format_line(Level::Info, "t", &format!("{hex} {body}"), true);
        assert!(!line.contains(&hex));
        assert!(line.len() < 2400);
        assert!(line.contains("[truncated]"));
    }

    #[tokio::test]
    async fn request_context_is_added() {
        let ctx = Arc::new(Mutex::new(RequestContext {
            request_id: "rid-1".into(),
            tenant_id: None,
        }));
        let line = CONTEXT
            .scope(ctx, async {
                set_tenant(42);
                format_line(Level::Info, "t", "hello", true)
            })
            .await;
        let v: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(v["request_id"], "rid-1");
        assert_eq!(v["tenant_id"], 42);
        assert_eq!(v["level"], "info");
        assert_eq!(v["msg"], "hello");
    }

    #[test]
    fn level_parsing() {
        assert_eq!(parse_level("debug"), LevelFilter::Debug);
        assert_eq!(parse_level("payment_gateway=warn"), LevelFilter::Warn);
        assert_eq!(parse_level("garbage"), LevelFilter::Info);
    }
}
