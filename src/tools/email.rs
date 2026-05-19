#![allow(dead_code)]

use anyhow::Result;

#[derive(Debug, Clone)]
pub struct EmailMessage {
    pub to: String,
    pub subject: String,
    pub body: String,
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
            let redactor = crate::secrets::SecretRedactor::from_config_and_env(
                &crate::config::Config::default(),
            );
            let preview = format!(
                "[DRY-RUN] Would send email:\n  To:      {}\n  Subject: {}\n  Body:\n{}",
                msg.to, msg.subject, msg.body
            );
            Ok(redactor.redact_text(&preview))
        }
        SendMode::Send => {
            let host = std::env::var("SMTP_HOST").map_err(|_| {
                anyhow::anyhow!("SMTP not configured: set SMTP_HOST, SMTP_USER, SMTP_PASS")
            })?;
            let user = std::env::var("SMTP_USER").map_err(|_| {
                anyhow::anyhow!("SMTP not configured: set SMTP_HOST, SMTP_USER, SMTP_PASS")
            })?;
            let pass = std::env::var("SMTP_PASS").map_err(|_| {
                anyhow::anyhow!("SMTP not configured: set SMTP_HOST, SMTP_USER, SMTP_PASS")
            })?;
            let port: u16 = std::env::var("SMTP_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(587);
            let redactor = crate::secrets::SecretRedactor::from_config_and_env(
                &crate::config::Config::default(),
            );

            use lettre::{
                AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
                transport::smtp::authentication::Credentials,
            };

            let email_msg =
                Message::builder()
                    .from(user.parse().map_err(|e| {
                        redact_error(&redactor, format!("Invalid from address: {e}"))
                    })?)
                    .to(msg
                        .to
                        .parse()
                        .map_err(|e| redact_error(&redactor, format!("Invalid to address: {e}")))?)
                    .subject(&msg.subject)
                    .body(msg.body.clone())
                    .map_err(|e| redact_error(&redactor, format!("Email build error: {e}")))?;

            let creds = Credentials::new(user, pass);
            let mailer = AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(&host)
                .map_err(|e| redact_error(&redactor, format!("SMTP relay error: {e}")))?
                .port(port)
                .credentials(creds)
                .build();

            mailer
                .send(email_msg)
                .await
                .map_err(|e| redact_error(&redactor, format!("SMTP send failed: {e}")))?;

            Ok(format!("Email sent to {} via {}:{}", msg.to, host, port))
        }
    }
}

fn redact_error(redactor: &crate::secrets::SecretRedactor, message: String) -> anyhow::Error {
    anyhow::anyhow!("{}", redactor.redact_text(&message))
}

/// Validates a minimal email address (contains '@' and a '.').
/// Used by agents before building an EmailMessage.
pub fn validate_address(addr: &str) -> bool {
    addr.contains('@') && addr.contains('.')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvVarGuard {
        values: Vec<(&'static str, Option<String>)>,
    }

    impl EnvVarGuard {
        fn set(values: &[(&'static str, &'static str)]) -> (MutexGuard<'static, ()>, Self) {
            let lock = ENV_LOCK.lock().expect("env lock poisoned");
            let previous = values
                .iter()
                .map(|(name, _)| (*name, std::env::var(name).ok()))
                .collect();

            for (name, value) in values {
                unsafe {
                    std::env::set_var(name, value);
                }
            }

            (lock, Self { values: previous })
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            for (name, previous) in &self.values {
                unsafe {
                    if let Some(previous) = previous {
                        std::env::set_var(name, previous);
                    } else {
                        std::env::remove_var(name);
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn dry_run_returns_preview() {
        let msg = EmailMessage {
            to: "test@example.com".into(),
            subject: "Hello".into(),
            body: "World".into(),
        };
        let result = send(&msg, SendMode::DryRun).await.unwrap();
        assert!(result.contains("[DRY-RUN]"));
        assert!(result.contains("test@example.com"));
    }

    #[tokio::test]
    async fn dry_run_redacts_secret_like_body() {
        let msg = EmailMessage {
            to: "test@example.com".into(),
            subject: "Hello".into(),
            body: "password=super-secret-value".into(),
        };

        let result = send(&msg, SendMode::DryRun).await.unwrap();

        assert!(result.contains("password=[REDACTED]"));
        assert!(!result.contains("super-secret-value"));
    }

    #[tokio::test]
    async fn live_send_errors() {
        let msg = EmailMessage {
            to: "test@example.com".into(),
            subject: "Hello".into(),
            body: "World".into(),
        };
        assert!(send(&msg, SendMode::Send).await.is_err());
    }

    #[tokio::test]
    async fn live_send_error_does_not_expose_smtp_pass() {
        let (_env_lock, _env_guard) = EnvVarGuard::set(&[
            ("SMTP_HOST", "invalid.localhost"),
            ("SMTP_USER", "sender@example.com"),
            ("SMTP_PASS", "smtp-secret-123456"),
        ]);

        let msg = EmailMessage {
            to: "test@example.com".into(),
            subject: "Hello".into(),
            body: "World".into(),
        };

        let err = send(&msg, SendMode::Send).await.unwrap_err().to_string();
        assert!(!err.contains("smtp-secret-123456"));
    }

    #[test]
    fn validates_address() {
        assert!(validate_address("user@example.com"));
        assert!(!validate_address("notanemail"));
        assert!(!validate_address("noatsign.com"));
    }
}
