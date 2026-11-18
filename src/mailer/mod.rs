mod smtp;
pub mod templates;

use anyhow::Result;
use async_trait::async_trait;
use std::sync::Mutex;
use std::time::Duration;

pub use smtp::SmtpMailer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
}

#[async_trait]
pub trait Mailer: Send + Sync {
    fn enabled(&self) -> bool;
    async fn send(&self, message: Message) -> Result<()>;
}

pub struct DisabledMailer;

#[async_trait]
impl Mailer for DisabledMailer {
    fn enabled(&self) -> bool {
        false
    }

    async fn send(&self, _message: Message) -> Result<()> {
        Ok(())
    }
}

#[derive(Default)]
pub struct RecordingMailer {
    sent: Mutex<Vec<Message>>,
    delay: Mutex<Duration>,
}

impl RecordingMailer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_delay(&self, delay: Duration) {
        *self.delay.lock().unwrap() = delay;
    }

    pub fn sent(&self) -> Vec<Message> {
        self.sent.lock().unwrap().clone()
    }

    pub fn clear(&self) {
        self.sent.lock().unwrap().clear();
    }

    pub async fn wait_for(&self, count: usize) -> Vec<Message> {
        for _ in 0..200 {
            let sent = self.sent();
            if sent.len() >= count {
                return sent;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        self.sent()
    }
}

#[async_trait]
impl Mailer for RecordingMailer {
    fn enabled(&self) -> bool {
        true
    }

    async fn send(&self, message: Message) -> Result<()> {
        let delay = *self.delay.lock().unwrap();
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        self.sent.lock().unwrap().push(message);
        Ok(())
    }
}

pub fn from_env() -> Result<Option<std::sync::Arc<dyn Mailer>>> {
    let url = std::env::var("SMTP_URL")
        .ok()
        .filter(|v| !v.trim().is_empty());
    let from = std::env::var("MAIL_FROM")
        .ok()
        .filter(|v| !v.trim().is_empty());
    match (url, from) {
        (Some(url), Some(from)) => Ok(Some(std::sync::Arc::new(SmtpMailer::new(&url, &from)?))),
        (None, None) => Ok(None),
        _ => anyhow::bail!("SMTP_URL and MAIL_FROM must be set together"),
    }
}
