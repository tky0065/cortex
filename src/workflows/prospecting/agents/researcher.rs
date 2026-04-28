#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/researcher.md");

pub async fn run(criteria: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "researcher".into() });

    let model = crate::providers::model_for_role("researcher", &options.config)?;
    let prompt = format!(
        "Find 10 potential prospects matching these criteria:\n\n{}",
        criteria
    );
    let prospects = crate::providers::complete(model, PREAMBLE, &prompt).await
        .map_err(|e| anyhow::anyhow!("Researcher agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "researcher".into(),
        chunk: format!("prospects.md ready ({} chars)", prospects.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "researcher".into() });

    Ok(prospects)
}
