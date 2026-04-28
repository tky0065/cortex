#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/reporter.md");

pub async fn run(combined: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "reporter".into() });

    let model = crate::providers::model_for_role("reporter", &options.config)?;
    let response = crate::providers::complete(model, PREAMBLE, combined).await
        .map_err(|e| anyhow::anyhow!("Reporter agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "reporter".into(),
        chunk: format!("final report ready ({} chars)", response.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "reporter".into() });

    Ok(response)
}
