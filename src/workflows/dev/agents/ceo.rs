#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{
    RunOptions, bus_agent_done, bus_agent_started, send_agent_progress, send_agent_summary,
};

const PREAMBLE_RAW: &str = include_str!("../prompts/ceo.md");

pub async fn run(idea: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "ceo".into(),
    });
    send_agent_progress(options, "ceo", "Analyse du besoin et cadrage MVP");
    bus_agent_started(options, "ceo").await;

    let model = crate::providers::model_for_role("ceo", &options.config)?;
    let response = crate::providers::complete(
        model,
        crate::custom_defs::prompt_body(PREAMBLE_RAW),
        idea,
        options,
        "ceo",
    )
    .await
    .map_err(|e| anyhow::anyhow!("CEO agent error: {e}"))?;

    send_agent_summary(options, "ceo", &response);
    bus_agent_done(options, "ceo", &response).await;
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "ceo".into(),
        chunk: format!("brief generated ({} chars)", response.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "ceo".into(),
    });

    Ok(response)
}
