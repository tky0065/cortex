#![allow(dead_code)]

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct EmailMessage {
    pub to:      String,
    pub subject: String,
    pub body:    String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendMode {
    DryRun,
    Send,
}

/// Sends (or simulates sending) an email.
/// DryRun: returns a formatted preview string, no network I/O.
/// Send: reads SMTP_HOST, SMTP_USER, SMTP_PASS from env and sends via STARTTLS.
pub async fn send(msg: &EmailMessage, mode: SendMode) -> Result<String> {
    match mode {
        SendMode::DryRun => {
            Ok(format!(
                "[DRY-RUN] Would send email:\n  To:      {}\n  Subject: {}\n  Body:\n{}",
                msg.to, msg.subject, msg.body
            ))
        }
        SendMode::Send => {
            let host = std::env::var("SMTP_HOST")
                .map_err(|_| anyhow::anyhow!("SMTP not configured: set SMTP_HOST, SMTP_USER, SMTP_PASS"))?;
            let user = std::env::var("SMTP_USER")
                .map_err(|_| anyhow::anyhow!("SMTP not configured: set SMTP_HOST, SMTP_USER, SMTP_PASS"))?;
            let pass = std::env::var("SMTP_PASS")
                .map_err(|_| anyhow::anyhow!("SMTP not configured: set SMTP_HOST, SMTP_USER, SMTP_PASS"))?;
            let port: u16 = std::env::var("SMTP_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(587);

            use lettre::{
                AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
                transport::smtp::authentication::Credentials,
            };

            let email_msg = Message::builder()
                .from(user.parse().map_err(|e| anyhow::anyhow!("Invalid from address: {e}"))?)
                .to(msg.to.parse().map_err(|e| anyhow::anyhow!("Invalid to address: {e}"))?)
                .subject(&msg.subject)
                .body(msg.body.clone())
                .map_err(|e| anyhow::anyhow!("Email build error: {e}"))?;

            let creds = Credentials::new(user, pass);
            let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
                .map_err(|e| anyhow::anyhow!("SMTP relay error: {e}"))?
                .port(port)
                .credentials(creds)
                .build();

            mailer.send(email_msg).await
                .map_err(|e| anyhow::anyhow!("SMTP send failed: {e}"))?;

            Ok(format!("Email sent to {} via {}:{}", msg.to, host, port))
        }
    }
}

/// Validates a minimal email address (contains '@' and a '.').
/// Used by agents before building an EmailMessage.
pub fn validate_address(addr: &str) -> bool {
    addr.contains('@') && addr.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dry_run_returns_preview() {
        let msg = EmailMessage {
            to:      "test@example.com".into(),
            subject: "Hello".into(),
            body:    "World".into(),
        };
        let result = send(&msg, SendMode::DryRun).await.unwrap();
        assert!(result.contains("[DRY-RUN]"));
        assert!(result.contains("test@example.com"));
    }

    #[tokio::test]
    async fn live_send_errors() {
        let msg = EmailMessage {
            to:      "test@example.com".into(),
            subject: "Hello".into(),
            body:    "World".into(),
        };
        assert!(send(&msg, SendMode::Send).await.is_err());
    }

    #[test]
    fn validates_address() {
        assert!(validate_address("user@example.com"));
        assert!(!validate_address("notanemail"));
        assert!(!validate_address("noatsign.com"));
    }
}
