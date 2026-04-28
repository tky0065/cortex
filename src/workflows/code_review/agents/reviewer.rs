#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/reviewer.md");

pub async fn run(source_content: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "reviewer".into() });

    let model = crate::providers::model_for_role("reviewer", &options.config)?;
    let response = crate::providers::complete(model, PREAMBLE, source_content).await
        .map_err(|e| anyhow::anyhow!("Reviewer agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "reviewer".into(),
        chunk: format!("review notes ready ({} chars)", response.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "reviewer".into() });

    Ok(response)
}
