#![allow(dead_code)]

use anyhow::Result;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/strategist.md");

pub async fn run(brief: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "strategist".into() });

    let client = rig_ollama::Client::new(Nothing)
        .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
    let model = crate::providers::model_for_role("developer", &options.config)?;
    let agent = client.agent(model).preamble(PREAMBLE).build();

    let prompt = format!("Create a complete marketing strategy for:\n\n{}", brief);
    let strategy = agent
        .prompt(prompt.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("Strategist agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "strategist".into(),
        chunk: format!("strategy.md ready ({} chars)", strategy.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "strategist".into() });

    Ok(strategy)
}
