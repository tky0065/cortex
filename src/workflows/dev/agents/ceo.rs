#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/ceo.md");

pub async fn run(idea: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "ceo".into() });

    let model = crate::providers::model_for_role("ceo", &options.config)?;
    let response = crate::providers::complete(model, PREAMBLE, idea).await
        .map_err(|e| anyhow::anyhow!("CEO agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "ceo".into(),
        chunk: format!("brief ready ({} chars)", response.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "ceo".into() });

    Ok(response)
}
