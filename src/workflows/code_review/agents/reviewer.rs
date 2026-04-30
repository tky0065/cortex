#![allow(dead_code)]

use anyhow::Result;

use crate::tui::events::TuiEvent;
use crate::workflows::{RunOptions, bus_agent_started, bus_agent_done, send_agent_progress, send_agent_summary};

const PREAMBLE: &str = include_str!("../prompts/reviewer.md");

pub async fn run(source_content: &str, options: &RunOptions) -> Result<String> {
    let _ = options.tx.send(TuiEvent::AgentStarted {
        agent: "reviewer".into(),
    });
    send_agent_progress(options, "reviewer", "Revue generale du code");
    bus_agent_started(options, "reviewer").await;

    let model = crate::providers::model_for_role("reviewer", &options.config)?;
    let response = crate::providers::complete(model, PREAMBLE, source_content, options, "reviewer")
        .await
        .map_err(|e| anyhow::anyhow!("Reviewer agent error: {e}"))?;

    send_agent_summary(options, "reviewer", &response);
    bus_agent_done(options, "reviewer", &response).await;
    let _ = options.tx.send(TuiEvent::TokenChunk {
        agent: "reviewer".into(),
        chunk: format!("review notes generated ({} chars)", response.len()),
    });
    let _ = options.tx.send(TuiEvent::AgentDone {
        agent: "reviewer".into(),
    });

    Ok(response)
}
