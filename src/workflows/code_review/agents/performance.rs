#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/performance.md");

pub async fn run(source_content: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "performance".into() });

    let model = crate::providers::model_for_role("performance", &options.config)?;
    let response = crate::providers::complete(model, PREAMBLE, source_content).await
        .map_err(|e| anyhow::anyhow!("Performance agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "performance".into(),
        chunk: format!("performance report ready ({} chars)", response.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "performance".into() });

    Ok(response)
}
