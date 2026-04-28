#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::RunOptions;

const PREAMBLE: &str = include_str!("../prompts/security.md");

pub async fn run(source_content: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted { agent: "security".into() });

    let model = crate::providers::model_for_role("security", &options.config)?;
    let response = crate::providers::complete(model, PREAMBLE, source_content).await
        .map_err(|e| anyhow::anyhow!("Security agent error: {e}"))?;

    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "security".into(),
        chunk: format!("security report ready ({} chars)", response.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone { agent: "security".into() });

    Ok(response)
}
