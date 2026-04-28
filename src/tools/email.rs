#![allow(dead_code)]

use anyhow::{bail, Result};

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
/// Send: returns Err("SMTP not yet configured") — placeholder until smtp crate is added.
pub async fn send(msg: &EmailMessage, mode: SendMode) -> Result<String> {
    match mode {
        SendMode::DryRun => {
            Ok(format!(
                "[DRY-RUN] Would send email:\n  To:      {}\n  Subject: {}\n  Body:\n{}",
                msg.to, msg.subject, msg.body
            ))
        }
        SendMode::Send => {
            bail!(
                "SMTP live sending not yet configured. Re-run without --send or configure an SMTP provider."
            )
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
