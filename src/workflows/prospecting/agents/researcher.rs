#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{
    RunOptions, bus_agent_done, bus_agent_started, send_agent_progress, send_agent_summary,
};

const PREAMBLE_RAW: &str = include_str!("../prompts/researcher.md");

pub async fn run(criteria: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "researcher".into(),
    });
    send_agent_progress(options, "researcher", "Recherche des prospects cibles");
    bus_agent_started(options, "researcher").await;

    let model = crate::providers::model_for_role("researcher", &options.config)?;
    let prompt = format!(
        "Find 10 potential prospects matching these criteria:\n\n{}",
        criteria
    );
    let prospects = crate::providers::complete(
        model,
        crate::custom_defs::prompt_body(PREAMBLE_RAW),
        &prompt,
        options,
        "researcher",
    )
    .await
    .map_err(|e| anyhow::anyhow!("Researcher agent error: {e}"))?;

    send_agent_summary(options, "researcher", &prospects);
    bus_agent_done(options, "researcher", &prospects).await;
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "researcher".into(),
        chunk: format!("prospects.md generated ({} chars)", prospects.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "researcher".into(),
    });

    Ok(prospects)
}
