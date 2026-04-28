#![allow(dead_code)]

use anyhow::Result;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/analyst.md");

pub async fn run(strategy: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "analyst".into() });

    let client = rig_ollama::Client::new(Nothing)
        .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
    let model = crate::providers::model_for_role("developer", &options.config)?;
    let agent = client.agent(model).preamble(PREAMBLE).build();

    let prompt = format!("Define KPIs and A/B tests for this marketing strategy:\n\n{}", strategy);
    let metrics = agent
        .prompt(prompt.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("Analyst agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "analyst".into(),
        chunk: format!("metrics.md ready ({} chars)", metrics.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "analyst".into() });

    Ok(metrics)
}
