#![allow(dead_code)]

use anyhow::Result;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/researcher.md");

pub async fn run(criteria: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "researcher".into() });

    let client = rig_ollama::Client::new(Nothing)
        .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
    let model = crate::providers::model_for_role("developer", &options.config)?;
    let agent = client.agent(model).preamble(PREAMBLE).build();

    let prompt = format!(
        "Find 10 potential prospects matching these criteria:\n\n{}",
        criteria
    );
    let prospects = agent
        .prompt(prompt.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("Researcher agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "researcher".into(),
        chunk: format!("prospects.md ready ({} chars)", prospects.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "researcher".into() });

    Ok(prospects)
}
