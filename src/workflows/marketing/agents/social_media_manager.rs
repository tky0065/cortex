#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/social_media_manager.md");

pub async fn run(strategy: &str, copy: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "social_media_manager".into() });

    let model = crate::providers::model_for_role("social_media_manager", &options.config)?;
    let prompt = format!(
        "Create a 30-day content calendar.\n\nStrategy:\n{}\n\nCopy:\n{}",
        strategy, copy
    );
    let calendar = crate::providers::complete(model, PREAMBLE, &prompt).await
        .map_err(|e| anyhow::anyhow!("Social Media Manager agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "social_media_manager".into(),
        chunk: format!("calendar.md ready ({} chars)", calendar.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "social_media_manager".into() });

    Ok(calendar)
}
