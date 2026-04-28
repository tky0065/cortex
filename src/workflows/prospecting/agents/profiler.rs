#![allow(dead_code)]

use anyhow::Result;
use rig::client::{CompletionClient, Nothing};
use rig::completion::Prompt;
use rig::providers::ollama as rig_ollama;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/profiler.md");

pub async fn run(prospect_entry: &str, options: &RunOptions) -> Result<String> {
    let agent_name = "profiler".to_string();
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: agent_name.clone() });

    let client = rig_ollama::Client::new(Nothing)
        .map_err(|e| anyhow::anyhow!("Ollama init failed: {e}"))?;
    let model = crate::providers::model_for_role("developer", &options.config)?;
    let agent = client.agent(model).preamble(PREAMBLE).build();

    let prompt = format!("Profile this prospect:\n\n{}", prospect_entry);
    let profile = agent
        .prompt(prompt.as_str())
        .await
        .map_err(|e| anyhow::anyhow!("Profiler agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: agent_name.clone(),
        chunk: format!("profile ready ({} chars)", profile.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: agent_name });

    Ok(profile)
}
