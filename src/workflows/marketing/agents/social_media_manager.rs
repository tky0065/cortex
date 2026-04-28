#![allow(dead_code)]

use anyhow::Result;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/social_media_manager.md");

pub async fn run(strategy: &str, copy: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "social_media_manager".into() });

    let client = rig_ollama::Client::new(Nothing)
        .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
    let model = crate::providers::model_for_role("developer", &options.config)?;
    let agent = client.agent(model).preamble(PREAMBLE).build();

    let prompt = format!(
        "Create a 30-day content calendar.\n\nStrategy:\n{}\n\nCopy:\n{}",
        strategy, copy
    );
    let calendar = agent
        .prompt(prompt.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("Social Media Manager agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "social_media_manager".into(),
        chunk: format!("calendar.md ready ({} chars)", calendar.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "social_media_manager".into() });

    Ok(calendar)
}
