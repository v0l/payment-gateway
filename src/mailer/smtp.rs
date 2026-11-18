use super::{Mailer, Message};
use anyhow::{Context, Result};
use async_trait::async_trait;
use lettre::message::{Mailbox, MultiPart};
use lettre::{AsyncSmtpTransport, AsyncTransport, Tokio1Executor};

pub struct SmtpMailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpMailer {
    pub fn new(url: &str, from: &str) -> Result<Self> {
        let transport = AsyncSmtpTransport::<Tokio1Executor>::from_url(url)
            .context("SMTP_URL")?
            .build();
        let from = from.parse().context("MAIL_FROM")?;
        Ok(Self { transport, from })
    }
}

#[async_trait]
impl Mailer for SmtpMailer {
    fn enabled(&self) -> bool {
        true
    }

    async fn send(&self, m: Message) -> Result<()> {
        let email = lettre::Message::builder()
            .from(self.from.clone())
            .to(m.to.parse().context("recipient")?)
            .subject(m.subject)
            .multipart(MultiPart::alternative_plain_html(m.text, m.html))?;
        self.transport.send(email).await?;
        Ok(())
    }
}
