#![allow(dead_code)]

use anyhow::Result;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/security.md");

pub async fn run(source_content: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "security".into() });

    let client = rig_ollama::Client::new(Nothing)
        .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
    let model = crate::providers::model_for_role("security", &options.config)?;
    let agent = client.agent(model).preamble(PREAMBLE).build();

    let response = agent
        .prompt(source_content)
        .await
        .map_err(|e| anyhow::anyhow!("Security agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "security".into(),
        chunk: format!("security report ready ({} chars)", response.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "security".into() });

    Ok(response)
}
