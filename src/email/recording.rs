use std::sync::Mutex;

use async_trait::async_trait;

use super::{Email, EmailSender};

/// An `EmailSender` that keeps each message in memory. Tests use it.
#[derive(Default)]
pub struct RecordingEmailSender {
    messages: Mutex<Vec<Email>>,
}

impl RecordingEmailSender {
    pub fn sent(&self) -> Vec<Email> {
        self.messages.lock().expect("lock").clone()
    }

    pub fn last_to(&self, to: &str) -> Option<Email> {
        self.messages
            .lock()
            .expect("lock")
            .iter()
            .rev()
            .find(|email| email.to == to)
            .cloned()
    }
}

#[async_trait]
impl EmailSender for RecordingEmailSender {
    async fn send(&self, email: Email) -> anyhow::Result<()> {
        self.messages.lock().expect("lock").push(email);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::{Email, EmailSender};

    fn email(to: &str, subject: &str) -> Email {
        Email {
            to: to.into(),
            subject: subject.into(),
            text: "t".into(),
            html: "h".into(),
        }
    }

    #[tokio::test]
    async fn recording_sender_keeps_messages() {
        let sender = RecordingEmailSender::default();
        sender.send(email("a@example.com", "one")).await.unwrap();
        sender.send(email("a@example.com", "two")).await.unwrap();
        assert_eq!(sender.sent().len(), 2);
        assert_eq!(sender.last_to("a@example.com").unwrap().subject, "two");
        assert!(sender.last_to("b@example.com").is_none());
    }
}
