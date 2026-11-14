use async_trait::async_trait;

pub mod recording;
pub mod smtp;
pub mod templates;

/// One outgoing message with a plain-text part and an HTML part.
#[derive(Debug, Clone, PartialEq)]
pub struct Email {
    pub to: String,
    pub subject: String,
    pub text: String,
    pub html: String,
}

#[async_trait]
pub trait EmailSender: Send + Sync {
    async fn send(&self, email: Email) -> anyhow::Result<()>;
}
