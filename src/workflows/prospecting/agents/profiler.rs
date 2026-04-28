#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, send_agent_progress, send_agent_summary};

const PREAMBLE: &str = include_str!("../prompts/profiler.md");

pub async fn run(prospect_entry: &str, options: &RunOptions) -> Result<String> {
    let agent_name = "profiler".to_string();
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: agent_name.clone(),
    });
    send_agent_progress(options, agent_name.clone(), "Analyse du profil prospect");

    let model = crate::providers::model_for_role("profiler", &options.config)?;
    let prompt = format!("Profile this prospect:\n\n{}", prospect_entry);
    let profile = crate::providers::complete(model, PREAMBLE, &prompt, options, "profiler")
        .await
        .map_err(|e| anyhow::anyhow!("Profiler agent error: {e}"))?;

    send_agent_summary(options, agent_name.clone(), &profile);
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: agent_name.clone(),
        chunk: format!("profile generated ({} chars)", profile.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: agent_name });

    Ok(profile)
}
