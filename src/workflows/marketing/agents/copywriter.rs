#![allow(dead_code)]

use anyhow::Result;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/copywriter.md");

pub async fn run(strategy: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "copywriter".into() });

    let client = rig_ollama::Client::new(Nothing)
        .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
    let model = crate::providers::model_for_role("developer", &options.config)?;
    let agent = client.agent(model).preamble(PREAMBLE).build();

    let prompt = format!("Write marketing copy based on this strategy:\n\n{}", strategy);
    let copy = agent
        .prompt(prompt.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("Copywriter agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "copywriter".into(),
        chunk: format!("copy ready ({} chars)", copy.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "copywriter".into() });

    Ok(copy)
}
