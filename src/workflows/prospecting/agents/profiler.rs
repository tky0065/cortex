#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/profiler.md");

pub async fn run(prospect_entry: &str, options: &RunOptions) -> Result<String> {
    let agent_name = "profiler".to_string();
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: agent_name.clone() });

    let model = crate::providers::model_for_role("profiler", &options.config)?;
    let prompt = format!("Profile this prospect:\n\n{}", prospect_entry);
    let profile = crate::providers::complete(model, PREAMBLE, &prompt).await
        .map_err(|e| anyhow::anyhow!("Profiler agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: agent_name.clone(),
        chunk: format!("profile ready ({} chars)", profile.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: agent_name });

    Ok(profile)
}
