#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{
    RunOptions, bus_agent_done, bus_agent_started, send_agent_progress, send_agent_summary,
};

const PREAMBLE_RAW: &str = include_str!("../prompts/performance.md");

pub async fn run(source_content: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "performance".into(),
    });
    send_agent_progress(options, "performance", "Analyse performance du code");
    bus_agent_started(options, "performance").await;

    let model = crate::providers::model_for_role("performance", &options.config)?;
    let response = crate::providers::complete(
        model,
        crate::custom_defs::prompt_body(PREAMBLE_RAW),
        source_content,
        options,
        "performance",
    )
    .await
    .map_err(|e| anyhow::anyhow!("Performance agent error: {e}"))?;

    send_agent_summary(options, "performance", &response);
    bus_agent_done(options, "performance", &response).await;
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "performance".into(),
        chunk: format!("performance report generated ({} chars)", response.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "performance".into(),
    });

    Ok(response)
}
