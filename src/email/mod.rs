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

/// Send an email in the background. A failure is logged, and the request does not fail.
/// Call this only after the database transaction commits.
pub fn spawn_send(state: &crate::AppState, email: Email) {
    let sender = state.email.clone();
    state.tasks.spawn(async move {
        if let Err(err) = sender.send(email).await {
            tracing::error!("email send failed: {err:#}");
        }
    });
}
