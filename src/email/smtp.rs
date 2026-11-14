use anyhow::{Context, Result};
use async_trait::async_trait;
use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, MultiPart},
    transport::smtp::authentication::Credentials,
};

use super::{Email, EmailSender};
use crate::config::SmtpConfig;

/// Send email over SMTP. Implicit TLS when `tls` is true, plain SMTP otherwise.
pub struct SmtpEmailSender {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl SmtpEmailSender {
    pub fn new(smtp: &SmtpConfig, from: &str) -> Result<Self> {
        let builder = if smtp.tls {
            AsyncSmtpTransport::<Tokio1Executor>::relay(&smtp.host)
                .context("SMTP_HOST is not valid for TLS")?
        } else {
            AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(&smtp.host)
        };
        let mut builder = builder.port(smtp.port);
        if let (Some(username), Some(password)) = (&smtp.username, &smtp.password) {
            builder = builder.credentials(Credentials::new(username.clone(), password.clone()));
        }
        Ok(Self {
            transport: builder.build(),
            from: from.parse().context("EMAIL_FROM is not a valid mailbox")?,
        })
    }
}

#[async_trait]
impl EmailSender for SmtpEmailSender {
    async fn send(&self, email: Email) -> Result<()> {
        let message = Message::builder()
            .from(self.from.clone())
            .to(email
                .to
                .parse()
                .context("recipient is not a valid mailbox")?)
            .subject(email.subject)
            .multipart(MultiPart::alternative_plain_html(email.text, email.html))?;
        self.transport.send(message).await?;
        Ok(())
    }
}
