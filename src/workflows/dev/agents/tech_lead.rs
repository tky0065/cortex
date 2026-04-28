#![allow(dead_code)]

use anyhow::Result;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/tech_lead.md");

pub async fn run(specs: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "tech_lead".into() });

    let client = rig_ollama::Client::new(Nothing)
        .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
    let model = crate::providers::model_for_role("tech_lead", &options.config)?;
    let agent = client.agent(model).preamble(PREAMBLE).build();

    let prompt = format!(
        "Generate a complete architecture.md for these specifications:\n\n{}",
        specs
    );
    let arch = agent
        .prompt(prompt.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("Tech Lead agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "tech_lead".into(),
        chunk: format!("architecture.md ready ({} chars)", arch.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "tech_lead".into() });

    Ok(arch)
}
